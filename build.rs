// Embeds the app icon and version info into the Windows executables,
// and compiles the Slint UI (ui/*.slint) when the desktop feature is on.

/// Compile every .slint file in ui/ so `slint::include_modules!()` in the desktop
/// binary picks them up. Each lane adds its own ui/*.slint file; no list to keep here.
///
/// Widget style is pinned per target (Inan's call, 2026-11-13: "just make it
/// good so its in your hands"): cupertino on macOS, fluent on Windows and
/// Linux, with Heron's dark palette carried as custom theme properties in the
/// .slint files. This API shape follows the scoping plan; it is NOT verified
/// in this sandbox (no Slint crate here), so the first Mac or CI build is the
/// check for it.
#[cfg(feature = "desktop")]
fn compile_slint_ui() {
    let mut files: Vec<std::path::PathBuf> = std::fs::read_dir("ui")
        .map(|entries| {
            entries
                .filter_map(|e| e.ok().map(|e| e.path()))
                .filter(|p| p.extension().is_some_and(|ext| ext == "slint"))
                .collect()
        })
        .unwrap_or_default();
    files.sort();
    let style = match std::env::var("CARGO_CFG_TARGET_OS").as_deref() {
        Ok("macos") => "cupertino",
        _ => "fluent",
    };
    for file in &files {
        println!("cargo:rerun-if-changed={}", file.display());
        let config = slint_build::CompilerConfiguration::new().with_style(style.to_string());
        slint_build::compile_with_config(file, config).expect("Slint UI failed to compile");
    }
}

fn main() {
    #[cfg(feature = "desktop")]
    compile_slint_ui();
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
