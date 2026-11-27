//! How the desktop window looks: theme, light or dark, accent, motion.
//!
//! Kept in `ui.toml` beside `config.toml`, not inside it. Saving settings
//! rewrites `config.toml` from the server's copy of that file; a separate
//! file cannot be dropped by that rewrite. Unknown values fall back to the
//! defaults from the visual spec: Dusk, follow the system, the system blue,
//! and follow the system's reduce-motion setting.

use crate::config::default_home;
use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::io::Write;
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct UiPrefs {
    /// dusk | shore | graphite
    pub theme: String,
    /// system | light | dark
    pub appearance: String,
    /// system | slate | lagoon | reed | rose
    pub accent: String,
    /// system | on | off. `on` means reduce motion.
    pub reduce_motion: String,
}

impl Default for UiPrefs {
    fn default() -> Self {
        UiPrefs {
            theme: "dusk".into(),
            appearance: "system".into(),
            accent: "system".into(),
            reduce_motion: "system".into(),
        }
    }
}

impl UiPrefs {
    pub fn normalized(mut self) -> Self {
        self.theme = canonical(&self.theme, &["dusk", "shore", "graphite"], "dusk").into();
        self.appearance =
            canonical(&self.appearance, &["system", "light", "dark"], "system").into();
        self.accent = canonical(
            &self.accent,
            &["system", "slate", "lagoon", "reed", "rose"],
            "system",
        )
        .into();
        self.reduce_motion =
            canonical(&self.reduce_motion, &["system", "on", "off"], "system").into();
        self
    }

    pub fn path(home: &Path) -> PathBuf {
        home.join("ui.toml")
    }

    /// Preferences for the default home, or the defaults if the file is missing or unreadable.
    pub fn load() -> Self {
        Self::load_from(&default_home())
    }

    pub fn load_from(home: &Path) -> Self {
        let Ok(text) = std::fs::read_to_string(Self::path(home)) else {
            return Self::default();
        };
        toml::from_str::<UiPrefs>(&text)
            .unwrap_or_default()
            .normalized()
    }

    pub fn save(&self) -> Result<()> {
        self.save_to(&default_home())
    }

    pub fn save_to(&self, home: &Path) -> Result<()> {
        std::fs::create_dir_all(home)
            .with_context(|| format!("cannot create {}", home.display()))?;
        let prefs = self.clone().normalized();
        let text =
            toml::to_string_pretty(&prefs).context("could not write appearance preferences")?;
        let path = Self::path(home);
        let tmp = home.join(format!(
            "ui.toml.tmp-{}-{}",
            std::process::id(),
            uuid::Uuid::new_v4().simple()
        ));
        let written = (|| -> Result<()> {
            let mut opts = std::fs::OpenOptions::new();
            opts.write(true).create_new(true);
            #[cfg(unix)]
            {
                use std::os::unix::fs::OpenOptionsExt;
                opts.mode(0o600);
            }
            let mut file = opts.open(&tmp)?;
            file.write_all(text.as_bytes())?;
            file.sync_all()?;
            drop(file);
            std::fs::rename(&tmp, &path)?;
            Ok(())
        })();
        if written.is_err() {
            let _ = std::fs::remove_file(&tmp);
        }
        written.with_context(|| format!("could not save {}", path.display()))
    }

    pub fn is_dark(&self, system_dark: bool) -> bool {
        match self.appearance.as_str() {
            "light" => false,
            "dark" => true,
            _ => system_dark,
        }
    }

    pub fn reduces_motion(&self, system_reduce: bool) -> bool {
        match self.reduce_motion.as_str() {
            "on" => true,
            "off" => false,
            _ => system_reduce,
        }
    }
}

fn canonical<'a>(value: &str, allowed: &[&'a str], fallback: &'a str) -> &'a str {
    allowed
        .iter()
        .copied()
        .find(|item| *item == value)
        .unwrap_or(fallback)
}

/// Whether the operating system is set to a dark appearance.
/// A failure, or a platform with no readable setting, is light.
pub fn system_prefers_dark() -> bool {
    #[cfg(target_os = "macos")]
    {
        command_stdout(&["defaults", "read", "-g", "AppleInterfaceStyle"])
            .is_some_and(|text| text.to_lowercase().contains("dark"))
    }
    #[cfg(target_os = "windows")]
    {
        // AppsUseLightTheme: 0x0 is dark, 0x1 is light.
        let text = command_stdout(&[
            "reg",
            "query",
            r"HKCU\Software\Microsoft\Windows\CurrentVersion\Themes\Personalize",
            "/v",
            "AppsUseLightTheme",
        ]);
        text.is_some_and(|text| text.contains("0x0"))
    }
    #[cfg(not(any(target_os = "macos", target_os = "windows")))]
    {
        if std::env::var("GTK_THEME")
            .ok()
            .is_some_and(|theme| theme.to_lowercase().contains("dark"))
        {
            return true;
        }
        if let Some(text) = command_stdout(&[
            "gsettings",
            "get",
            "org.gnome.desktop.interface",
            "color-scheme",
        ]) {
            if text.contains("prefer-dark") {
                return true;
            }
            if text.contains("prefer-light") || text.contains("default") {
                return false;
            }
        }
        command_stdout(&[
            "gsettings",
            "get",
            "org.gnome.desktop.interface",
            "gtk-theme",
        ])
        .is_some_and(|text| text.to_lowercase().contains("dark"))
    }
}

/// Whether the operating system asks for reduced motion.
/// A failure, or a platform with no readable setting, is full motion.
pub fn system_reduce_motion() -> bool {
    #[cfg(target_os = "macos")]
    {
        command_stdout(&["defaults", "read", "-g", "AppleReduceMotion"])
            .is_some_and(|text| text.trim() == "1")
    }
    #[cfg(target_os = "windows")]
    {
        false
    }
    #[cfg(not(any(target_os = "macos", target_os = "windows")))]
    {
        // enable-animations false means the user turned motion off.
        command_stdout(&[
            "gsettings",
            "get",
            "org.gnome.desktop.interface",
            "enable-animations",
        ])
        .is_some_and(|text| text.trim() == "false")
    }
}

fn command_stdout(argv: &[&str]) -> Option<String> {
    let (program, args) = argv.split_first()?;
    let mut child = std::process::Command::new(program)
        .args(args)
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::null())
        .spawn()
        .ok()?;
    let mut stdout = child.stdout.take()?;
    let (tx, rx) = std::sync::mpsc::channel();
    std::thread::spawn(move || {
        let mut buf = Vec::new();
        let _ = std::io::Read::read_to_end(&mut stdout, &mut buf);
        let _ = tx.send(buf);
    });
    // A stuck settings daemon must not hold up opening the window.
    let buf = match rx.recv_timeout(std::time::Duration::from_millis(400)) {
        Ok(buf) => buf,
        Err(_) => {
            let _ = child.kill();
            return None;
        }
    };
    let _ = child.wait();
    if buf.is_empty() {
        return None;
    }
    String::from_utf8(buf).ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unknown_values_fall_back_to_the_spec_defaults() {
        let prefs = UiPrefs {
            theme: "neon".into(),
            appearance: "sepia".into(),
            accent: "gold".into(),
            reduce_motion: "sometimes".into(),
        }
        .normalized();
        assert_eq!(prefs, UiPrefs::default());
        assert!(!prefs.is_dark(false));
        assert!(prefs.is_dark(true));
        assert!(!prefs.reduces_motion(false));
        assert!(prefs.reduces_motion(true));
    }

    #[test]
    fn explicit_appearance_ignores_the_system() {
        let light = UiPrefs {
            appearance: "light".into(),
            ..UiPrefs::default()
        };
        let dark = UiPrefs {
            appearance: "dark".into(),
            ..UiPrefs::default()
        };
        assert!(!light.is_dark(true));
        assert!(dark.is_dark(false));
        let still = UiPrefs {
            reduce_motion: "off".into(),
            ..UiPrefs::default()
        };
        let quiet = UiPrefs {
            reduce_motion: "on".into(),
            ..UiPrefs::default()
        };
        assert!(!still.reduces_motion(true));
        assert!(quiet.reduces_motion(false));
    }

    #[test]
    fn preferences_round_trip_and_a_broken_file_is_the_default() {
        let dir = tempfile::tempdir().unwrap();
        let prefs = UiPrefs {
            theme: "shore".into(),
            appearance: "light".into(),
            accent: "lagoon".into(),
            reduce_motion: "on".into(),
        };
        prefs.save_to(dir.path()).unwrap();
        assert_eq!(UiPrefs::load_from(dir.path()), prefs);
        std::fs::write(UiPrefs::path(dir.path()), "theme = [").unwrap();
        assert_eq!(UiPrefs::load_from(dir.path()), UiPrefs::default());
        assert!(
            !dir.path().join("ui.toml").exists()
                || UiPrefs::load_from(dir.path()) == UiPrefs::default()
        );
    }

    #[test]
    fn a_missing_file_is_the_default_and_does_not_create_one() {
        let dir = tempfile::tempdir().unwrap();
        assert_eq!(UiPrefs::load_from(dir.path()), UiPrefs::default());
        assert!(!UiPrefs::path(dir.path()).exists());
    }
}
