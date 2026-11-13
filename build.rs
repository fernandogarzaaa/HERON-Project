// Embeds the app icon and version info into the Windows executables,
// and compiles the Slint UI (ui/*.slint) when the desktop feature is on.

/// Compile the Slint UI (ui/main.slint) when the desktop feature is on.
///
/// Only the root file is compiled: each `compile_with_config` call sets the
/// SLINT_INCLUDE_GENERATED variable, last call wins, and `include_modules!()`
/// reads exactly that one path. Compiling a second top-level file (a lane's
/// ui/views.slint, say) would silently drop MainWindow from the generated
/// code. Lanes add their UI as modules imported from main.slint, never as
/// separate compile roots. Every .slint file under ui/ is still watched for
/// rebuilds, so editing an imported file recompiles the UI.
///
/// Widget style is pinned per target (Inan's call, 2026-11-13: "just make it
/// good so its in your hands"): cupertino on macOS, fluent on Windows and
/// Linux, with Heron's dark palette carried as custom theme properties in the
/// .slint files. This API shape follows the scoping plan; it is NOT verified
/// in this sandbox (no Slint crate here), so the first Mac or CI build is the
/// check for it.
#[cfg(feature = "desktop")]
fn compile_slint_ui() {
    fn collect_slint(dir: &std::path::Path, out: &mut Vec<std::path::PathBuf>) {
        let Ok(entries) = std::fs::read_dir(dir) else {
            return;
        };
        for entry in entries.flatten() {
            let path = entry.path();
            if path.is_dir() {
                collect_slint(&path, out);
            } else if path.extension().is_some_and(|ext| ext == "slint") {
                out.push(path);
            }
        }
    }
    let mut files = Vec::new();
    collect_slint(std::path::Path::new("ui"), &mut files);
    for file in &files {
        println!("cargo:rerun-if-changed={}", file.display());
    }
    let style = match std::env::var("CARGO_CFG_TARGET_OS").as_deref() {
        Ok("macos") => "cupertino",
        _ => "fluent",
    };
    let config = slint_build::CompilerConfiguration::new().with_style(style.to_string());
    slint_build::compile_with_config("ui/main.slint", config).expect("Slint UI failed to compile");
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
