//! JSON persistence for tasks and per-employee memory.
//!
//! The store is written atomically (serialize → temp file → rename) so a crash
//! mid-write can never leave a half-written file that fails to deserialize on
//! the next start (`perf-io-buffering`: single write at completion/save time).

use std::path::{Path, PathBuf};

use color_eyre::eyre::{Result, WrapErr};
use serde::{Deserialize, Serialize};

use crate::task::Task;

/// The on-disk store: scheduled-task history plus per-employee memory.
#[derive(Debug, Default, Serialize, Deserialize)]
pub struct Store {
    /// Scheduled tasks, newest last, retained across restarts.
    #[serde(default)]
    pub tasks: Vec<Task>,
    /// Per-employee memory, keyed by employee id (arbitrary JSON).
    #[serde(default)]
    pub memory: serde_json::Map<String, serde_json::Value>,
    /// The engine gate: while set, an autonomous fire (cron or watch) is queued
    /// as a `pending` task instead of dispatched. Persisted so a paused engine
    /// is still paused after a restart (AC5). Defaults to `false` for a store
    /// written before the field existed.
    #[serde(default)]
    pub paused: bool,
}

impl Store {
    /// Default store path: `<config_dir>/mywaker/store.json`.
    pub fn default_path() -> Option<PathBuf> {
        dirs::config_dir().map(|dir| dir.join("mywaker").join("store.json"))
    }

    /// Load the store from `path`; a missing file loads as an empty store.
    pub fn load(path: &Path) -> Result<Self> {
        if !path.exists() {
            return Ok(Self::default());
        }
        let text = std::fs::read_to_string(path)
            .wrap_err_with(|| format!("reading {}", path.display()))?;
        serde_json::from_str(&text).wrap_err_with(|| format!("parsing {}", path.display()))
    }

    /// Load from the default path, tolerating any error as an empty store.
    pub fn load_or_default() -> Self {
        Self::default_path()
            .and_then(|path| Self::load(&path).ok())
            .unwrap_or_default()
    }

    /// Atomically persist the store: serialize, write a temp sibling, then
    /// rename over the target so a reader never observes a partial file.
    pub fn save(&self, path: &Path) -> Result<()> {
        let text = serde_json::to_string_pretty(self).wrap_err("serializing store")?;
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)
                .wrap_err_with(|| format!("creating {}", parent.display()))?;
        }
        let tmp = path.with_extension("json.tmp");
        std::fs::write(&tmp, text).wrap_err_with(|| format!("writing {}", tmp.display()))?;
        std::fs::rename(&tmp, path).wrap_err_with(|| format!("renaming to {}", path.display()))
    }

    /// Upsert a task (replacing by id) and persist.
    pub fn upsert_task(&mut self, task: Task) {
        let id = task.id;
        if let Some(existing) = self.tasks.iter_mut().find(|t| t.id == id) {
            *existing = task;
        } else {
            self.tasks.push(task);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::task::TaskStatus;

    #[test]
    fn missing_file_loads_empty() {
        let path =
            std::env::temp_dir().join(format!("mywaker-store-missing-{}.json", std::process::id()));
        let _ = std::fs::remove_file(&path);
        let store = Store::load(&path).unwrap();
        assert!(store.tasks.is_empty());
        assert!(store.memory.is_empty());
        assert!(!store.paused);
    }

    #[test]
    fn round_trips_tasks_and_memory() {
        let dir = std::env::temp_dir().join(format!("mywaker-store-test-{}", std::process::id()));
        let path = dir.join("store.json");
        let _ = std::fs::remove_dir_all(&dir);

        let mut store = Store::default();
        store
            .tasks
            .push(Task::new("summarize", Some("planner".into())));
        store.memory.insert(
            "planner".into(),
            serde_json::json!({"note": "likes briefs"}),
        );

        store.save(&path).unwrap();

        // No temp file left behind.
        let tmp = path.with_extension("json.tmp");
        assert!(!tmp.exists());

        let loaded = Store::load(&path).unwrap();
        assert_eq!(loaded.tasks.len(), 1);
        assert_eq!(loaded.tasks[0].goal, "summarize");
        assert_eq!(
            loaded.memory.get("planner"),
            Some(&serde_json::json!({"note": "likes briefs"}))
        );

        let _ = std::fs::remove_dir_all(&dir);
    }

    /// The engine pause gate round-trips: a store saved paused reloads paused
    /// (AC5), and a store predating the field loads not-paused.
    #[test]
    fn engine_pause_survives_a_round_trip() {
        let dir = std::env::temp_dir().join(format!("mywaker-paused-{}", std::process::id()));
        let path = dir.join("store.json");
        let _ = std::fs::remove_dir_all(&dir);

        let store = Store {
            paused: true,
            ..Store::default()
        };
        store.save(&path).unwrap();

        let loaded = Store::load(&path).unwrap();
        assert!(loaded.paused, "a paused engine must reload paused");

        // A legacy payload without the field defaults to not-paused.
        let legacy = serde_json::json!({ "tasks": [], "memory": {} });
        std::fs::write(&path, legacy.to_string()).unwrap();
        assert!(!Store::load(&path).unwrap().paused);

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn upsert_replaces_by_id() {
        let mut store = Store::default();
        let mut task = Task::new("original", Some("writer".into()));
        task.succeed("v1");

        store.upsert_task(task.clone());
        assert_eq!(store.tasks.len(), 1);
        assert_eq!(store.tasks[0].status, TaskStatus::Succeeded);

        // A later task with the same id replaces the entry (status → Failed).
        let mut updated = task;
        updated.result = None; // supersede the earlier deliverable
        updated.fail("v2 failed");
        store.upsert_task(updated);

        assert_eq!(store.tasks.len(), 1);
        assert_eq!(store.tasks[0].status, TaskStatus::Failed);
        assert_eq!(store.tasks[0].result.as_deref(), Some("v2 failed"));
    }
}
