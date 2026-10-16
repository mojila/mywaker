//! Project discovery: list the working directories under one or more
//! configured roots so a task can be pointed at the right repo.
//!
//! Discovery is a shallow scan of each root (non-recursive): each child
//! *directory* becomes a [`Project`]; files and dot-directories are skipped.
//! The set of roots comes from [`scan_roots`] (env override → persisted list →
//! `~/Projects`) and is managed with the `mywaker projects dirs` CLI.

use std::path::{Path, PathBuf};

use color_eyre::eyre::{Result, WrapErr};
use serde::{Deserialize, Serialize};

/// A discoverable project: its directory name plus the full path OpenCode
/// sessions should be rooted at.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Project {
    pub name: String,
    pub path: PathBuf,
}

/// Persisted list of directories to scan (`~/.config/mywaker/projects.toml`),
/// managed with `mywaker projects dirs add|rm`.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct ScanDirs {
    pub dirs: Vec<PathBuf>,
}

impl ScanDirs {
    /// Default state path: `<config_dir>/mywaker/projects.toml`.
    pub fn default_path() -> Option<PathBuf> {
        dirs::config_dir().map(|dir| dir.join("mywaker").join("projects.toml"))
    }

    /// Load the scan list; a missing file is an empty list.
    pub fn load(path: &Path) -> Result<Self> {
        if !path.exists() {
            return Ok(Self::default());
        }
        let text = std::fs::read_to_string(path)
            .wrap_err_with(|| format!("reading {}", path.display()))?;
        toml::from_str(&text).wrap_err_with(|| format!("parsing {}", path.display()))
    }

    /// Load the persisted scan list (or an empty one if none exists).
    pub fn load_or_default() -> Self {
        Self::default_path()
            .and_then(|path| Self::load(&path).ok())
            .unwrap_or_default()
    }

    /// Persist the scan list (creating parent directories).
    pub fn save(&self, path: &Path) -> Result<()> {
        let text = toml::to_string_pretty(self).wrap_err("serializing scan dirs")?;
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)
                .wrap_err_with(|| format!("creating {}", parent.display()))?;
        }
        std::fs::write(path, text).wrap_err_with(|| format!("writing {}", path.display()))
    }

    /// Add a directory (deduplicated); true if it was newly added.
    pub fn add(&mut self, dir: PathBuf) -> bool {
        let dir = expand_tilde(dir);
        if self.dirs.iter().any(|d| d == &dir) {
            return false;
        }
        self.dirs.push(dir);
        true
    }

    /// Remove a directory; true if it was present.
    pub fn remove(&mut self, dir: &Path) -> bool {
        let dir = expand_tilde(dir.to_path_buf());
        let before = self.dirs.len();
        self.dirs.retain(|d| d != &dir);
        self.dirs.len() != before
    }
}

/// Resolve the directories to scan for projects, in priority order:
/// `$MYWAKER_PROJECTS_DIRS` (`:`-separated) if set, else the persisted
/// [`ScanDirs`], else `~/Projects`.
pub fn scan_roots() -> Vec<PathBuf> {
    if let Ok(env) = std::env::var("MYWAKER_PROJECTS_DIRS") {
        let dirs: Vec<PathBuf> = env
            .split(':')
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .map(PathBuf::from)
            .map(expand_tilde)
            .collect();
        if !dirs.is_empty() {
            return dirs;
        }
    }

    let persisted = ScanDirs::load_or_default();
    if !persisted.dirs.is_empty() {
        return persisted.dirs;
    }

    default_roots()
}

/// The fallback scan roots when nothing is configured: `~/Projects`.
pub fn default_roots() -> Vec<PathBuf> {
    dirs::home_dir()
        .map(|home| vec![home.join("Projects")])
        .unwrap_or_default()
}

/// Discover projects under a single root. A missing or non-directory root is an
/// empty list (not an error). Child directories become projects; dot-entries
/// are skipped. Sorted by name.
pub fn discover(root: &Path) -> Result<Vec<Project>> {
    discover_in(&[root.to_path_buf()])
}

/// Discover projects across `roots`, merged and de-duplicated by path.
pub fn discover_in(roots: &[PathBuf]) -> Result<Vec<Project>> {
    let mut projects: Vec<Project> = Vec::new();
    for root in roots {
        if !root.is_dir() {
            continue;
        }
        for entry in std::fs::read_dir(root).wrap_err_with(|| format!("read {}", root.display()))? {
            let entry = entry?;
            let path = entry.path();
            if !path.is_dir() {
                continue;
            }
            let name = entry.file_name().to_string_lossy().into_owned();
            if name.starts_with('.') {
                continue;
            }
            if projects.iter().any(|p| p.path == path) {
                continue;
            }
            projects.push(Project { name, path });
        }
    }

    projects.sort_by_key(|p| p.name.to_lowercase());
    Ok(projects)
}

/// Discover projects across all configured scan roots.
pub fn discover_all() -> Result<Vec<Project>> {
    discover_in(&scan_roots())
}

/// Expand a leading `~`/`~/` against the home directory.
fn expand_tilde(path: PathBuf) -> PathBuf {
    let Some(home) = dirs::home_dir() else {
        return path;
    };
    let s = path.to_string_lossy();
    if s == "~" {
        home
    } else if let Some(rest) = s.strip_prefix("~/") {
        home.join(rest)
    } else {
        path
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::{SystemTime, UNIX_EPOCH};

    fn temp_root() -> PathBuf {
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let dir = std::env::temp_dir().join(format!("mywaker-projects-{nanos}"));
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn discovers_only_directories_sorted() {
        let root = temp_root();
        std::fs::create_dir(root.join("zeta")).unwrap();
        std::fs::create_dir(root.join("alpha")).unwrap();
        std::fs::create_dir(root.join(".hidden")).unwrap();
        std::fs::write(root.join("file.txt"), "x").unwrap();

        let projects = discover(&root).unwrap();
        let names: Vec<&str> = projects.iter().map(|p| p.name.as_str()).collect();
        assert_eq!(names, vec!["alpha", "zeta"]);

        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn missing_root_yields_empty_list() {
        let root = std::env::temp_dir().join("mywaker-definitely-missing-root");
        assert!(discover(&root).unwrap().is_empty());
    }

    #[test]
    fn discover_in_merges_dedups_and_sorts() {
        let a = temp_root();
        let b = temp_root();
        std::fs::create_dir(a.join("zzz")).unwrap();
        std::fs::create_dir(b.join("aaa")).unwrap();
        // Same path reached from two roots must not duplicate.
        std::fs::create_dir(a.join("shared")).unwrap();

        let mut roots = vec![a.clone(), b.clone()];
        roots.push(a.join("shared")); // third root == an existing project path

        let projects = discover_in(&roots).unwrap();
        let names: Vec<&str> = projects.iter().map(|p| p.name.as_str()).collect();
        assert_eq!(names, vec!["aaa", "shared", "zzz"]);

        let _ = std::fs::remove_dir_all(&a);
        let _ = std::fs::remove_dir_all(&b);
    }

    #[test]
    fn scan_dirs_add_remove_roundtrip() {
        let mut scan = ScanDirs::default();
        assert!(scan.add("/tmp/one".into()));
        assert!(scan.add("/tmp/two".into()));
        assert!(!scan.add("/tmp/one".into())); // duplicate

        let path = temp_root().join("projects.toml");
        scan.save(&path).unwrap();
        let mut loaded = ScanDirs::load(&path).unwrap();
        assert_eq!(loaded.dirs.len(), 2);

        assert!(loaded.remove(&PathBuf::from("/tmp/one")));
        assert_eq!(loaded.dirs, vec![PathBuf::from("/tmp/two")]);
        assert!(!loaded.remove(&PathBuf::from("/tmp/nope")));

        let _ = std::fs::remove_dir_all(path.parent().unwrap());
    }
}
