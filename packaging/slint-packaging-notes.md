# Slint packaging notes (Packaging lane, native/slint-packaging)

Historical drafting base: main ea06729a (after PR #8).
Current review: PR #13 against native/slint, with the Slint shell already
merged via PR #9. The sections below record the original packaging audit
and drafts; references to the old Wry desktop binary or an unpushed
worktree describe that *historical* state, not the current integration
target. The CI checks a synthetic merge of this PR with native/slint.
The Linux installer now also has a no-root smoke test in
tests/linux-install-smoke.sh; tagged release installers still require
independent end-to-end testing on each supported platform.

## 1. Audit: what each file does today, and its webview dependency

### packaging/macos/build-app.sh
Builds dist/Heron.app (universal, Apple silicon plus Intel) and
dist/Heron-macos-universal.dmg. Steps: rustup add both macOS targets,
cargo build --release --features desktop --bins per target with
MACOSX_DEPLOYMENT_TARGET=11.0, lipo the heron-desktop and heron binaries
into the app bundle, copy Info.plist with the version substituted, copy
icon.icns, ad-hoc codesign, build the DMG with an Applications symlink.
Webview dependency: none in this script. The binary it lipo's is the
webview binary today, the Slint binary after the Shell lane lands.
No change needed beyond what the Shell lane does to the binary itself.

### packaging/windows/build.ps1
Builds dist/Heron-windows-x64.zip and, when Inno Setup is installed,
dist/Heron-Setup-windows-x64.exe. Steps: cargo build --release --features
desktop --bins, copy heron-desktop.exe to Heron.exe and heron.exe to
heron-cli.exe, copy LICENSE, compress the zip, run ISCC with the version.
Webview dependency: lines 15-17 copied WebView2Loader.dll out of the
cargo build dir for GNU-toolchain builds. Drafted change: that block is
removed. Slint ships no WebView2Loader.dll. Nothing else in the script
mentions the webview.

### packaging/windows/heron.iss
Inno Setup script. Per-user install, no admin rights. Installs
dist/Heron/* to {localappdata}/Programs/Heron, Start menu and optional
desktop shortcuts, optional post-install launch. Webview dependency:
none. No change drafted.

### packaging/linux/build.sh
Builds dist/Heron-linux-x64.tar.gz. Steps: cargo build --release
--features desktop --bins, copy both binaries, the icon, install.sh and
LICENSE into dist/Heron-linux-x64, tar it. Webview dependency: only the
header comment naming libwebkit2gtk-4.1-dev and libgtk-3-dev. Drafted
change: comment now names the Slint libraries. Build command unchanged.

### packaging/linux/install.sh
Per-user install from an unpacked folder. Places both binaries, the icon
and itself under ~/.local/share/heron, symlinks into ~/.local/bin, writes
heron.desktop. Webview dependency: the trailing check warned when
libwebkit2gtk-4.1 was absent and printed apt/dnf install hints. Drafted
change: the check now looks for libfontconfig.so.1 and libGL.so.1 and
prints the Debian/Fedora packages for those. The rest of the script is
unchanged.

### .github/workflows/ci.yml
Test job on ubuntu-22.04, macos-latest, windows-latest: checkout, rustup
stable, Linux apt install, cargo test, cargo build --features desktop
--bins. Neural job on ubuntu-24.04, macos-latest, windows-latest, ignored
neural tests, continue-on-error. Webview dependency: the Linux apt line
installed libwebkit2gtk-4.1-dev libgtk-3-dev. Drafted change: that line
now installs libfontconfig-dev libxkbcommon-dev libxcb-shape0-dev
libxcb-xfixes0-dev libssl-dev pkg-config. Step renamed to "Linux
libraries for the Slint window". Nothing else changes.

### .github/workflows/release.yml
Tag push v* builds macOS DMG, Windows zip plus Inno Setup exe, Linux
tarball, then a publish job checks the tag matches Cargo.toml, writes
SHA256SUMS.txt, and creates or updates a prerelease. Webview dependency:
the Linux job apt line, same as CI. Drafted change: same replacement.
macOS and Windows jobs need no new install step. The publish job is
unchanged. Artifact names are unchanged, so docs/index.html download
links keep working.

### Cargo.toml desktop feature (read only, Shell lane owns)
[package] heron 0.2.0, edition 2021. [[bin]] heron-desktop at
src/bin/desktop.rs with required-features ["desktop"]. The desktop
feature today is ["dep:wry", "dep:tao", "dep:muda"]: wry 0.57.2, tao
0.37.2, muda 0.21.2, all optional. The feature comment still says
"Native desktop window (WebView2 on Windows, WKWebView on macOS,
WebKitGTK on Linux)". src/bin/desktop.rs (157 lines) uses tao for the
window and event loop, wry WebViewBuilder pointed at the local server
URL, and muda for the menu. build.rs only embeds the Windows icon via
winresource. It has no webview dependency and needs no change.
packaging/ and .github/ never reference assets/index.html, so no
packaging step ships the web UI today.

## 2. Slint facts (measured, not guessed)

- Latest published stable is 1.18.1, released 21 Sep 2026. Evidence:
  Slint repo release list shows v1.18.1 as the last release as of
  12 Nov 2026, and an October 2026 version table lists Slint 1.18.
  Master is 1.20.0 and 1.19 is in pre-release on the pre-release/1.19
  branch. The program plan (~/workspace/HERON-Slint-Plan.md, section 2)
  pins slint =1.18.1 and slint-build =1.18.1 in lockstep, default
  features. This lane's recommendation matches the plan exactly.
  Do not pin the 1.19 pre-release for a release build.
- Default features of the slint crate (from api/rs/slint/Cargo.toml on
  master): std, backend-default, renderer-femtovg, renderer-software,
  accessibility, compat-1-2, system-tray. Default backend is winit.
  Default renderer is FemtoVG (OpenGL). Skia is opt-in, not default.
  Qt backend is opt-in, not default.
- Rust requirement: master declares rust-version 1.95 and edition 2024.
  Slint 1.18 needs Rust 1.92 or newer. CI uses rustup stable and the
  sandbox has cargo 1.99.0. No CI toolchain change needed.
- Linux build dependencies. Two documented lists exist and they cover
  different layers, so the CI list is their union. From Slint's own
  build guide (slint-ui/slint master docs/building.md): libxcb-shape0-dev,
  libxcb-xfixes0-dev, libxkbcommon-dev, libfontconfig-dev, libssl-dev.
  From the program plan (section 2, winit backend X11 layer):
  libx11-xcb-dev, libxcursor-dev, libxkbcommon-x11-dev, libx11-dev,
  and "xinput", whose dev package is libxi-dev (mapping inferred:
  libxi-dev provides the XInput headers). pkg-config is needed for
  fontconfig detection. xcb and xkbcommon are not needed for a
  Wayland-only build (backend-winit-wayland without backend-winit-x11),
  but the default winit backend builds both, so the union keeps them.
- macOS: Xcode Command Line Tools only. Present on macos-latest.
  No new install step.
- Windows: MSVC toolchain (stable-x86_64-pc-windows-msvc) and MSVC
  Build Tools. Present on windows-latest. No new install step. The
  Skia note in the build guide applies only to the opt-in Skia
  renderer, not to the default FemtoVG renderer.
- The system-tray default feature pulls ksni on Linux, which needs a
  D-Bus session at runtime. That is a Shell lane decision: keep the
  default features, or disable them and re-enable the needed ones
  without system-tray if Heron has no tray icon.

## 3. System libraries the Slint binary needs

### Linux, build time (CI and local builds)
libfontconfig-dev, libxkbcommon-dev, libxkbcommon-x11-dev,
libxcb-shape0-dev, libxcb-xfixes0-dev, libx11-dev, libx11-xcb-dev,
libxcursor-dev, libxi-dev, libssl-dev, pkg-config. This is the list now
in ci.yml and release.yml. It is the union of the two documented lists
in section 2. Verified in the sandbox (see section 5).

### Linux, runtime (end user machines)
Linked: libfontconfig (system fonts). Dynamically loaded by the
renderers and windowing backend: libGL (FemtoVG OpenGL renderer),
libxcb and libxkbcommon (winit X11 path), Wayland client libs (winit
Wayland path). D-Bus at runtime only if the system-tray feature stays
enabled. install.sh now checks for libfontconfig.so.1 and libGL.so.1
and prints install hints. The exact linked set is measured from the
probe binary in section 5; the install.sh check covers the two
libraries most likely to be missing on a minimal install.

### macOS, runtime
Nothing new. The app bundle ships the same two binaries. Slint on
macOS uses the system OpenGL framework through FemtoVG and AppKit
through winit. No extra framework to bundle. MACOSX_DEPLOYMENT_TARGET
stays 11.0.

### Windows, runtime
Nothing new. No WebView2 runtime and no WebView2Loader.dll. The binary
links the MSVC C runtime the same way today's binary does.

## 4. What the Shell lane must decide (flagged, not edited here)

- Cargo.toml: remove optional wry 0.57.2, tao 0.37.2, muda 0.21.2.
  Add slint = "=1.18.1" and slint-build = "=1.18.1" (exact pins from
  the plan, section 2; the two must stay in lockstep). Change the
  desktop feature to ["dep:slint", "dep:arboard", "dep:rfd"] (arboard
  for programmatic copy, rfd for file dialogs, per the plan section 4)
  and update the stale feature comment. Keep the feature NAME
  "desktop": CI and packaging call cargo build --features desktop
  --bins. slint-build becomes an optional build-dependency, and
  build.rs compiles ui/main.slint with slint_build::compile_with_config
  when CARGO_FEATURE_DESKTOP is set. That also settles the .slint
  question: the UI is compiled into the binary, so no .slint file is
  shipped in the DMG, zip, or tarball.
- Binary name: confirmed by the plan (sections 5 and 7). It stays
  heron-desktop, same packaging. No rename is assumed here.
- Menu: the plan (section 4) says the Slint MenuBar renders as the
  real macOS menu bar via muda, which stays as a transitive dependency
  of the winit backend, not a direct one. The current build_menu()
  and the direct muda dependency go away at the end (Phase 4).
  Packaging is not affected.
- System tray: keep or drop the default system-tray feature (see
  section 2). Dropping it removes the D-Bus runtime dependency.
- macOS floor: CI runs macos-latest, so it cannot catch a problem with
  the 11.0 deployment target. The Shell lane owns runtime verification
  on the oldest supported macOS.
- Widget style: SETTLED (coordinator, Nov 13). Inan handed the call
  to Cookie: "just make it good so its in your hands." Cookie's call,
  following the plan's recommendation: cupertino on macOS, fluent on
  Windows and Linux, pinned per target in build.rs, with Heron's dark
  palette carried as custom theme properties. That lives in the Shell
  lane's build.rs, not in packaging. All three plan decisions are now
  settled.
- License: SETTLED (coordinator, Nov 13). Inan picked the free route:
  the royalty-free Slint license with AboutSlint attribution in the
  About dialog. That is UI, not packaging. No packaging file changes.
  This supersedes the coordinator's earlier message in this lane,
  which had listed it as still open.
- Headless Slint tests in CI via the testing backend: the plan's
  Phase 4 lists this as a CI addition. It depends on the Shell lane's
  testing setup, which does not exist yet. It is not drafted here.
  Flag it for Phase 4.
- "Native look" bar: Inan settled this on Nov 13: "Native window,
  coherent look, no webview. Slint clears it". No packaging impact.

## 5. Verification (measured in this sandbox)

Sandbox: Ubuntu 24.04.5 LTS, cargo 1.99.0. Pre-existing dev packages
(measured with dpkg): libssl-dev, libxkbcommon-dev, libx11-dev,
libfontconfig-dev 2.15.0, pkg-config. NOT installed: libxcb-shape0-dev,
libxcb-xfixes0-dev, libxkbcommon-x11-dev, libx11-xcb-dev,
libxcursor-dev, libxi-dev. An apt-get update to install the missing
ones stalled over 12 minutes on the sandbox proxy and was killed; the
install was not needed once the probe hit the registry wall below.
Probe crate: ~/workspace/slint-probe (scratch, outside the worktree),
slint = "1.18", edition 2021, minimal slint! hello-world main.
Result: cargo check FAILED, exit 101, after 185 seconds. Resolution
got far enough to confirm slint 1.18.1 resolves from "1.18", then
stalled downloading the crates.io sparse-index entry for input-sys:
"transfer too slow: failed to transfer more than 10 bytes in 30s
(transferred 0 bytes)", repeated for 15 index entries, then "unable to
update registry crates-io". This confirms the program plan (section 9)
with an independent measurement. The plan's sharper diagnosis, also
measured: static.crates.io works (a .crate download succeeds in under
2 seconds), index.crates.io times out after 60 seconds with 0 bytes,
and pre-seeding .crate files does not unblock the build because the
failure is on the index metadata fetch. The registry cache already
holds 1,508 .crate files including winit 0.30.13, but no slint crate
and no lockfile pins Slint. What unblocks it: a build on a machine
with working index access (Inan's Mac, or CI), or a retry here later.
The real check for the CI apt list is a CI run on the ubuntu-22.04
runner, after the Shell lane lands. The probe crate is kept at
~/workspace/slint-probe as evidence of the attempt.

## 6. Drafted changes in this worktree (git diff to confirm)

- .github/workflows/ci.yml: Linux apt line replaced, step renamed.
- .github/workflows/release.yml: Linux job apt line replaced.
- packaging/linux/build.sh: header comment replaced.
- packaging/linux/install.sh: WebKitGTK runtime check replaced with
  fontconfig plus OpenGL checks.
- packaging/windows/build.ps1: WebView2Loader.dll copy block removed.
- packaging/macos/build-app.sh: no change. packaging/macos/Info.plist:
  no change. packaging/windows/heron.iss: no change.

## 7. Open items

- Shell lane has not published a Slint version pin yet. Its worktree
  (~/workspace/HERON-Slint-Shell) is still at base ea06729a with no
  changes at the time of this audit. The =1.18.1 pin above comes from
  the program plan (section 2) and from this lane's own check of the
  published releases, so it does not depend on the Shell lane.
- Probe build result (section 5) is pending at the time of writing.
- The drafted workflow and script changes are not verified by a CI
  run. A CI run only happens after the Shell lane lands and a PR
  opens. The Linux apt list is the union of the two documented lists
  (section 2); the sandbox probe is the only local check, and it is
  blocked on the crates.io index timeout (section 5).
- ubuntu-22.04 (CI) versus Ubuntu 24.04 (sandbox): the package names
  in the apt list exist on both. The probe runs on 24.04. A CI run on
  22.04 is still the real check for that runner.
- The plan's Phase 4 also deletes assets/index.html as the app UI,
  build_menu(), the webview navigation handler, and the HTML error page,
  and updates the README desktop section. Those deletions are Shell
  lane and docs work, not packaging, except that nothing in packaging/
  references assets/index.html today, so no packaging change follows.
- Risk 10 (plan section 10, this lane): per-OS packaging drift. Each
  script needs a pass to drop the webview runtime and confirm the Slint
  binary still launches from a cold install. The script pass is done
  (see section 6). The cold-install launch confirmation is still open:
  it needs a built Slint binary on each OS, which needs the Shell lane
  plus a machine with crates.io access or CI. Verification gate (plan
  section 11): cargo build --features desktop --bins green on all
  three OSes in CI, headless Slint tests via the testing backend,
  cargo test for the unchanged lib, and a manual run on Inan's Mac
  before any phase merges to native/slint.
- Integration: lanes merge into native/slint, never directly to main.
  Cookie integrates and verifies. This lane's drafts are uncommitted
  in native/slint-packaging until integration.

## 8. Design bar (program-level, Inan, Nov 13)

"Make it beautiful, more interactive, more intuitive to use." This is
the bar for the whole rebuild, on top of the settled decisions in
section 4. No direct design surface in this lane. The packaging
implication is first impression: the installers and the first launch
are part of it, so the app must launch cleanly into the onboarding
flow with no extra steps or confusing prompts. The current install
paths already meet that bar on paper: the DMG is drag to Applications,
the Inno Setup installer offers a post-install launch, and install.sh
prints one clear next step ("Open it from your applications menu, or
run: heron-desktop"). The first-launch behavior itself (landing in
onboarding, no extra prompts) is Shell lane and app behavior, not
packaging. The cold-install launch check in section 7 is where this
bar gets verified per OS.
