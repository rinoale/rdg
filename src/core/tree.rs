use std::{
    cmp::Ordering,
    collections::{HashMap, HashSet},
    fs,
    path::{Component, Path},
};

use anyhow::{Context, Result};

use crate::core::git::{Candidate, ChangeKind, git_candidates};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum EntryKind {
    Directory,
    File,
}

#[derive(Clone, Debug)]
pub struct TreeEntry {
    pub path: String,
    pub name: String,
    pub kind: EntryKind,
    pub depth: usize,
    pub git_kind: Option<ChangeKind>,
    pub status: String,
    pub selected: bool,
    pub expanded: bool,
}

impl TreeEntry {
    pub fn is_dir(&self) -> bool {
        self.kind == EntryKind::Directory
    }

    pub fn deploy_candidate(&self) -> Option<Candidate> {
        if self.is_dir() {
            return None;
        }

        Some(Candidate {
            path: self.path.clone(),
            kind: self.git_kind.unwrap_or(ChangeKind::Unchanged),
            status: self.status.clone(),
        })
    }
}

pub fn repo_tree_entries(repo_root: &Path) -> Result<Vec<TreeEntry>> {
    let git_candidates = git_candidates(repo_root)?;
    let status_by_path: HashMap<String, (ChangeKind, String)> = git_candidates
        .iter()
        .map(|candidate| {
            (
                candidate.path.clone(),
                (candidate.kind, candidate.status.clone()),
            )
        })
        .collect();
    let dir_statuses = directory_statuses(&git_candidates);

    let mut entries = Vec::new();
    let mut known_paths = HashSet::new();

    walk_dir(
        repo_root,
        repo_root,
        &status_by_path,
        &dir_statuses,
        &mut entries,
        &mut known_paths,
    )?;

    add_deleted_entries(
        &status_by_path,
        &dir_statuses,
        &mut entries,
        &mut known_paths,
    );

    entries.sort_by(|a, b| compare_tree_paths(&a.path, &b.path));

    Ok(entries)
}

fn walk_dir(
    repo_root: &Path,
    dir: &Path,
    status_by_path: &HashMap<String, (ChangeKind, String)>,
    dir_statuses: &HashMap<String, ChangeKind>,
    entries: &mut Vec<TreeEntry>,
    known_paths: &mut HashSet<String>,
) -> Result<()> {
    let mut children = fs::read_dir(dir)
        .with_context(|| format!("failed to read directory {}", dir.display()))?
        .collect::<std::result::Result<Vec<_>, _>>()
        .with_context(|| format!("failed to read directory entry in {}", dir.display()))?;

    children.sort_by_key(|entry| entry.file_name());

    for child in children {
        let path = child.path();
        let relative = path
            .strip_prefix(repo_root)
            .with_context(|| format!("failed to create relative path for {}", path.display()))?;

        if should_skip(relative) {
            continue;
        }

        let relative_path = normalize_path(relative);
        let metadata = child
            .metadata()
            .with_context(|| format!("failed to read metadata for {}", path.display()))?;
        let is_dir = metadata.is_dir();
        let git_kind = status_by_path
            .get(&relative_path)
            .map(|(kind, _)| *kind)
            .or_else(|| dir_statuses.get(&relative_path).copied());
        let status = status_by_path
            .get(&relative_path)
            .map(|(_, status)| status.clone())
            .unwrap_or_default();

        known_paths.insert(relative_path.clone());
        entries.push(TreeEntry {
            name: child.file_name().to_string_lossy().into_owned(),
            depth: path_depth(&relative_path),
            path: relative_path.clone(),
            kind: if is_dir {
                EntryKind::Directory
            } else {
                EntryKind::File
            },
            git_kind,
            status,
            selected: false,
            expanded: !is_dir,
        });

        if is_dir {
            walk_dir(
                repo_root,
                &path,
                status_by_path,
                dir_statuses,
                entries,
                known_paths,
            )?;
        }
    }

    Ok(())
}

fn add_deleted_entries(
    status_by_path: &HashMap<String, (ChangeKind, String)>,
    dir_statuses: &HashMap<String, ChangeKind>,
    entries: &mut Vec<TreeEntry>,
    known_paths: &mut HashSet<String>,
) {
    for (path, (kind, status)) in status_by_path {
        if *kind != ChangeKind::Deleted || known_paths.contains(path) {
            continue;
        }

        for ancestor in ancestors(path) {
            if known_paths.contains(&ancestor) {
                continue;
            }

            known_paths.insert(ancestor.clone());
            entries.push(TreeEntry {
                name: path_name(&ancestor),
                depth: path_depth(&ancestor),
                path: ancestor.clone(),
                kind: EntryKind::Directory,
                git_kind: dir_statuses
                    .get(&ancestor)
                    .copied()
                    .or(Some(ChangeKind::Deleted)),
                status: String::new(),
                selected: false,
                expanded: false,
            });
        }

        known_paths.insert(path.clone());
        entries.push(TreeEntry {
            name: path_name(path),
            depth: path_depth(path),
            path: path.clone(),
            kind: EntryKind::File,
            git_kind: Some(ChangeKind::Deleted),
            status: status.clone(),
            selected: false,
            expanded: true,
        });
    }
}

fn directory_statuses(candidates: &[Candidate]) -> HashMap<String, ChangeKind> {
    let mut statuses = HashMap::new();

    for candidate in candidates {
        for ancestor in ancestors(&candidate.path) {
            let entry = statuses.entry(ancestor).or_insert(candidate.kind);
            *entry = strongest_status(*entry, candidate.kind);
        }
    }

    statuses
}

fn strongest_status(left: ChangeKind, right: ChangeKind) -> ChangeKind {
    if status_rank(right) > status_rank(left) {
        right
    } else {
        left
    }
}

fn status_rank(kind: ChangeKind) -> u8 {
    match kind {
        ChangeKind::Unchanged => 0,
        ChangeKind::Untracked => 1,
        ChangeKind::Changed => 2,
        ChangeKind::Deleted => 3,
    }
}

fn ancestors(path: &str) -> Vec<String> {
    let mut ancestors = Vec::new();
    let mut parts: Vec<&str> = path.split('/').collect();
    parts.pop();

    for index in 1..=parts.len() {
        ancestors.push(parts[..index].join("/"));
    }

    ancestors
}

fn should_skip(relative: &Path) -> bool {
    matches!(
        relative.components().next(),
        Some(Component::Normal(name)) if name == ".git" || name == "target"
    )
}

fn normalize_path(path: &Path) -> String {
    path.components()
        .filter_map(|component| match component {
            Component::Normal(part) => Some(part.to_string_lossy().into_owned()),
            _ => None,
        })
        .collect::<Vec<_>>()
        .join("/")
}

fn path_depth(path: &str) -> usize {
    path.matches('/').count()
}

fn compare_tree_paths(left: &str, right: &str) -> Ordering {
    let mut left_parts = left.split('/');
    let mut right_parts = right.split('/');

    loop {
        match (left_parts.next(), right_parts.next()) {
            (Some(left), Some(right)) => match left.cmp(right) {
                Ordering::Equal => {}
                ordering => return ordering,
            },
            (None, Some(_)) => return Ordering::Less,
            (Some(_), None) => return Ordering::Greater,
            (None, None) => return Ordering::Equal,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tree_sort_keeps_descendants_under_parent() {
        let mut paths = vec!["a.txt", "a/b.txt", "a", "a/b", "a/b/c.txt", "b"];

        paths.sort_by(|left, right| compare_tree_paths(left, right));

        assert_eq!(
            paths,
            vec!["a", "a/b", "a/b/c.txt", "a/b.txt", "a.txt", "b"]
        );
    }
}

fn path_name(path: &str) -> String {
    path.rsplit('/').next().unwrap_or(path).to_string()
}
