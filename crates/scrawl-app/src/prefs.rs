//! Settings kept between runs (recent files), and where the app keeps its own files: preferences,
//! autosaves, versions and plugins.

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

/// How many files File › Open Recent remembers.
const MAX_RECENT: usize = 10;

/// The per-user folder for Scrawl's own files: `~/Library/Application Support/Scrawl` on macOS,
/// `%APPDATA%\Scrawl` on Windows, `$XDG_CONFIG_HOME/scrawl` (or `~/.config/scrawl`) elsewhere.
pub fn support_dir() -> Option<PathBuf> {
    let home = || std::env::var_os("HOME").map(PathBuf::from);
    if cfg!(target_os = "macos") {
        Some(home()?.join("Library/Application Support/Scrawl"))
    } else if cfg!(windows) {
        Some(PathBuf::from(std::env::var_os("APPDATA")?).join("Scrawl"))
    } else {
        let config = std::env::var_os("XDG_CONFIG_HOME").map(PathBuf::from).or_else(|| Some(home()?.join(".config")))?;
        Some(config.join("scrawl"))
    }
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct Prefs {
    /// Most recently opened or saved first.
    #[serde(default)]
    pub recent: Vec<PathBuf>,
}

impl Prefs {
    fn file(dir: &Path) -> PathBuf {
        dir.join("prefs.json")
    }

    /// Reads the preferences in `dir`, forgetting recent files that no longer exist.
    pub fn load(dir: &Path) -> Self {
        let mut prefs: Prefs =
            std::fs::read(Self::file(dir)).ok().and_then(|b| serde_json::from_slice(&b).ok()).unwrap_or_default();
        prefs.recent.retain(|p| p.exists());
        prefs
    }

    pub fn save(&self, dir: &Path) {
        let result = std::fs::create_dir_all(dir).and_then(|()| {
            let json = serde_json::to_vec_pretty(self).map_err(std::io::Error::other)?;
            std::fs::write(Self::file(dir), json)
        });
        if let Err(e) = result {
            log::warn!("couldn't save preferences: {e}");
        }
    }

    /// Moves `path` to the top of the recent files.
    pub fn add_recent(&mut self, path: &Path) {
        let path = std::fs::canonicalize(path).unwrap_or_else(|_| path.to_path_buf());
        self.recent.retain(|p| *p != path);
        self.recent.insert(0, path);
        self.recent.truncate(MAX_RECENT);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn recent_files_are_ordered_unique_and_bounded() {
        let dir = std::env::temp_dir().join(format!("scrawl-prefs-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let files: Vec<PathBuf> = (0..12).map(|i| dir.join(format!("{i}.scrawl"))).collect();
        files.iter().for_each(|f| std::fs::write(f, b"").unwrap());
        let mut prefs = Prefs::default();
        for f in &files {
            prefs.add_recent(f);
        }
        prefs.add_recent(&files[5]);
        assert_eq!(prefs.recent.len(), MAX_RECENT);
        assert!(prefs.recent[0].ends_with("5.scrawl"));
        assert_eq!(prefs.recent.iter().filter(|p| p.ends_with("5.scrawl")).count(), 1);
        prefs.save(&dir);
        std::fs::remove_file(&files[11]).unwrap();
        let back = Prefs::load(&dir);
        assert_eq!(back.recent.len(), MAX_RECENT - 1, "missing files are dropped");
        std::fs::remove_dir_all(&dir).ok();
    }
}
