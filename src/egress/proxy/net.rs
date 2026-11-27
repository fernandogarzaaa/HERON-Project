use super::tests::client_hello;
use super::*;

fn allow() -> Vec<String> {
    vec!["api.anthropic.com".into(), "*.example.org".into()]
}

async fn connect_through(proxy: &EgressProxy, host: &str) -> TcpStream {
    let mut s = TcpStream::connect(("127.0.0.1", proxy.port)).await.unwrap();
    s.write_all(format!("CONNECT {host}:443 HTTP/1.1\r\nHost: {host}\r\n\r\n").as_bytes()).await.unwrap();
    s
}

async fn read_some(s: &mut TcpStream) -> String {
    let mut buf = vec![0u8; 256];
    let n = tokio::time::timeout(Duration::from_secs(5), s.read(&mut buf)).await.unwrap().unwrap_or(0);
    String::from_utf8_lossy(&buf[..n]).into_owned()
}

#[tokio::test]
async fn refuses_disallowed_tunnels_over_the_wire() {
    let proxy = start(allow()).await.unwrap();
    let mut s = connect_through(&proxy, "evil.com").await;
    assert!(read_some(&mut s).await.starts_with("HTTP/1.1 403"));
    let d = proxy.decisions();
    assert_eq!(d, vec![Decision { target: "evil.com:443".into(), refusal: Some(Refusal::NotListed) }]);
    let mut s = TcpStream::connect(("127.0.0.1", proxy.port)).await.unwrap();
    s.write_all(b"GET http://api.anthropic.com/ HTTP/1.1\r\nHost: api.anthropic.com\r\n\r\n").await.unwrap();
    assert!(read_some(&mut s).await.starts_with("HTTP/1.1 403"));
    assert_eq!(proxy.decisions()[1], Decision { target: "GET http://api.anthropic.com/".into(), refusal: Some(Refusal::NotHttps) });
    let mut s = TcpStream::connect(("127.0.0.1", proxy.port)).await.unwrap();
    s.write_all(b"garbage\r\n\r\n").await.unwrap();
    assert!(read_some(&mut s).await.starts_with("HTTP/1.1 403"));
    assert_eq!(proxy.decisions()[2], Decision { target: "garbage".into(), refusal: Some(Refusal::NotHttps) });
}

#[tokio::test]
async fn dropping_the_proxy_closes_its_port() {
    let p = start(vec!["a.com".into()]).await.unwrap();
    let port = p.port;
    drop(p);
    tokio::time::sleep(Duration::from_millis(100)).await;
    assert!(TcpStream::connect(("127.0.0.1", port)).await.is_err());
}

#[tokio::test]
async fn oversized_request_head_is_dropped() {
    let p = start(vec!["a.com".into()]).await.unwrap();
    let mut s = TcpStream::connect(("127.0.0.1", p.port)).await.unwrap();
    let junk = format!("CONNECT a.com:443 HTTP/1.1\r\nX: {}\r\n", "y".repeat(9000));
    let _ = s.write_all(junk.as_bytes()).await;
    let mut buf = vec![0u8; 64];
    let n = tokio::time::timeout(Duration::from_secs(2), s.read(&mut buf)).await.unwrap().unwrap_or(0);
    assert!(!String::from_utf8_lossy(&buf[..n]).contains("200"), "no tunnel for an oversized head");
    assert!(p.decisions().is_empty());
}

#[tokio::test]
async fn allowlisted_names_resolving_to_internal_addresses_are_refused() {
    let proxy = start(vec!["localhost".into()]).await.unwrap();
    let mut s = connect_through(&proxy, "localhost").await;
    assert!(read_some(&mut s).await.starts_with("HTTP/1.1 403"));
    assert_eq!(proxy.decisions(), vec![Decision { target: "localhost:443".into(), refusal: Some(Refusal::NotPublic) }]);
}

#[tokio::test]
async fn the_tls_server_name_must_match_the_connect_host() {
    let upstream = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let port = upstream.local_addr().unwrap().port();
    let got = Arc::new(Mutex::new(Vec::<Vec<u8>>::new()));
    let sink = got.clone();
    tokio::spawn(async move {
        while let Ok((mut c, _)) = upstream.accept().await {
            let sink = sink.clone();
            tokio::spawn(async move {
                let mut buf = vec![0u8; 4096];
                let n = c.read(&mut buf).await.unwrap_or(0);
                sink.lock().unwrap().push(buf[..n].to_vec());
            });
        }
    });
    let opts = Options { allow_internal: true, upstream_port: Some(port) };
    let proxy = start_with(vec!["localhost".into()], opts).await.unwrap();
    for (sni, ok) in [(Some("evil.com"), false), (None, false), (Some("localhost"), true)] {
        let mut s = connect_through(&proxy, "localhost").await;
        assert!(read_some(&mut s).await.starts_with("HTTP/1.1 200"));
        let hello = client_hello(sni);
        s.write_all(&hello[..10]).await.unwrap();
        tokio::time::sleep(Duration::from_millis(50)).await;
        s.write_all(&hello[10..]).await.unwrap();
        tokio::time::sleep(Duration::from_millis(200)).await;
        let reached = got.lock().unwrap().iter().any(|b| b == &hello);
        assert_eq!(reached, ok, "sni {sni:?}");
    }
    let mut s = connect_through(&proxy, "localhost").await;
    read_some(&mut s).await;
    let before = proxy.decisions().len();
    s.shutdown().await.unwrap();
    let t0 = std::time::Instant::now();
    while proxy.decisions().len() == before && t0.elapsed() < Duration::from_secs(5) {
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
    assert!(t0.elapsed() < Duration::from_secs(2), "refused promptly after the client closed");
    let mut s = connect_through(&proxy, "localhost").await;
    read_some(&mut s).await;
    s.write_all(b"GET / HTTP/1.1\r\nHost: localhost\r\n\r\n").await.unwrap();
    tokio::time::sleep(Duration::from_millis(200)).await;
    assert_eq!(got.lock().unwrap().len(), 1, "plain HTTP in the tunnel never reaches upstream");
    let outcomes: Vec<Option<Refusal>> = proxy.decisions().into_iter().map(|d| d.refusal).collect();
    assert_eq!(
        outcomes,
        vec![
            Some(Refusal::NameMismatch(Some("evil.com".into()))),
            Some(Refusal::NameMismatch(None)),
            None,
            Some(Refusal::NameMismatch(None)),
            Some(Refusal::NameMismatch(None)),
        ]
    );
}
