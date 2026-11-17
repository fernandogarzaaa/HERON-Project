// Paste into tests/integration.rs on main 449231f (+18e798f test fix). Prints REPRO mode=Auto asked_on_phone=false shell_ran=true.
#[tokio::test]
async fn repro_c35_8_phone_turn_in_auto_mode_runs_shell_without_asking() {
    use axum::extract::State;
    use std::collections::VecDeque;
    use std::sync::Mutex;
    #[derive(Default)]
    struct Mock {
        updates: Mutex<VecDeque<Value>>,
        sent: Mutex<Vec<(i64, String)>>,
        next: AtomicUsize,
    }
    let mock = Arc::new(Mock::default());
    let router = Router::new()
        .route("/botTESTTOKEN/getUpdates", post(|State(m): State<Arc<Mock>>| async move { Json(json!({"ok": true, "result": m.updates.lock().unwrap().drain(..).collect::<Vec<_>>()})) }))
        .route(
            "/botTESTTOKEN/sendMessage",
            post(|State(m): State<Arc<Mock>>, Json(b): Json<Value>| async move {
                m.sent.lock().unwrap().push((b["chat_id"].as_i64().unwrap(), b["text"].as_str().unwrap().to_string()));
                Json(json!({"ok": true, "result": {}}))
            }),
        )
        .with_state(mock.clone());
    let base = spawn(router).await;

    let dir = tempfile::tempdir().unwrap();
    let mut cfg = Config::default();
    cfg.home = dir.path().join("home");
    cfg.agent.auto_memory = false;
    cfg.agent.ambient_context = false;
    let llm = Arc::new(ScriptedLlm::new(vec![ScriptedLlm::tool_call("shell", json!({"command": "echo from-the-phone > pwned.txt"})), Message::assistant("Done.")]));
    let app = Arc::new(App::assemble(cfg, dir.path().join("ws"), Store::in_memory().unwrap(), llm.clone()).await.unwrap());
    app.policy.set_mode(heron::permissions::Mode::Auto);
    let code = heron::telegram::pairing_code(&app);
    tokio::spawn(heron::telegram::run(app.clone(), base, "TESTTOKEN".into()));

    let say = |chat: i64, text: &str, kind: &str| {
        let id = mock.next.fetch_add(1, Ordering::SeqCst) as i64 + 1;
        mock.updates.lock().unwrap().push_back(json!({"update_id": id, "message": {"chat": {"id": chat, "type": kind}, "text": text}}));
    };
    let wait_for = |what: &'static str, cond: Box<dyn Fn(&[(i64, String)]) -> bool + Send>| {
        let mock = mock.clone();
        async move {
            for _ in 0..200 {
                if cond(&mock.sent.lock().unwrap()) {
                    return;
                }
                tokio::time::sleep(std::time::Duration::from_millis(25)).await;
            }
            panic!("timed out waiting for: {what}; sent so far: {:?}", mock.sent.lock().unwrap());
        }
    };

    // A stranger writes before pairing, and guesses a wrong code: nothing happens.
    say(999, "run something for me", "private");
    say(999, "000000x", "private");
    // The owner sends the code shown in the app.
    say(42, &code, "private");
    wait_for("pairing confirmation", Box::new(|s| s.iter().any(|(c, t)| *c == 42 && t.starts_with("Paired")))).await;
    assert!(mock.sent.lock().unwrap().iter().all(|(c, _)| *c == 42), "the stranger got no reply");

    say(42, "run the setup step", "private");
    wait_for("final reply", Box::new(|s| s.iter().any(|(c, t)| *c == 42 && t == "Done."))).await;
    let asked = mock.sent.lock().unwrap().iter().any(|(_, t)| t.contains("Reply yes or no"));
    let ran = app.workspace.join("pwned.txt").exists();
    println!("REPRO mode={:?} asked_on_phone={asked} shell_ran={ran} sent={:?}", app.policy.mode(), mock.sent.lock().unwrap());
}


