use std::{
    ffi::OsStr,
    path::{Component, Path},
    sync::mpsc::{self, Receiver},
};

use anyhow::Result;
use notify::{Config, Event, EventKind, RecommendedWatcher, RecursiveMode, Watcher};

pub type WatchEvent = notify::Result<Event>;

pub fn watch_repo(repo_root: &Path) -> Result<(RecommendedWatcher, Receiver<WatchEvent>)> {
    let (tx, rx) = mpsc::channel();

    let mut watcher = RecommendedWatcher::new(
        move |event| {
            let _ = tx.send(event);
        },
        Config::default(),
    )?;

    watcher.watch(repo_root, RecursiveMode::Recursive)?;

    Ok((watcher, rx))
}

pub fn should_refresh(repo_root: &Path, event: &Event) -> bool {
    if !matches!(
        event.kind,
        EventKind::Create(_) | EventKind::Modify(_) | EventKind::Remove(_)
    ) {
        return false;
    }

    if event.paths.is_empty() {
        return true;
    }

    event
        .paths
        .iter()
        .any(|path| !is_ignored_repo_path(repo_root, path))
}

fn is_ignored_repo_path(repo_root: &Path, path: &Path) -> bool {
    let Ok(relative) = path.strip_prefix(repo_root) else {
        return true;
    };

    matches!(
        relative.components().next(),
        Some(Component::Normal(name))
            if name == OsStr::new(".git") || name == OsStr::new("target")
    )
}
