// Embeds the app icon and version info into the Windows executables.
fn main() {
    println!("cargo:rerun-if-changed=packaging/icons/icon.ico");
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() == Ok("windows") {
        let mut res = winresource::WindowsResource::new();
        res.set_icon("packaging/icons/icon.ico");
        res.set("ProductName", "Heron");
        res.set("FileDescription", "Heron — personal AI agent");
        if let Err(e) = res.compile() {
            // An icon is cosmetic: never fail the build over it.
            println!("cargo:warning=could not embed Windows resources: {e}");
        }
    }
}
