use std::{
    collections::{HashSet, VecDeque},
    path::PathBuf,
};

use anyhow::Result;

use crate::core::{
    git::{Candidate, ChangeKind, git_root, preview_candidate},
    rsync::{RsyncRunReport, rsync_selected},
    tree::{EntryKind, TreeEntry, repo_tree_entries},
};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PreviewPane {
    Repository,
    Content,
    Git,
    Rsync,
}

const REPOSITORY_SCROLL_STEP: usize = 5;

pub struct App {
    pub repo_root: PathBuf,
    pub target: String,
    pub entries: Vec<TreeEntry>,
    pub cursor: usize,
    pub git_preview: String,
    pub rsync_preview: String,
    pub content_diff_preview: String,
    pub git_scroll: u16,
    pub rsync_scroll: u16,
    pub content_diff_scroll: u16,
    pub rsync_dry_run_ready: bool,
    pub rsync_preview_stale: bool,
    pub rsync_report: Option<RsyncRunReport>,
    pub message: String,
    pub active_pane: PreviewPane,
    pub should_quit: bool,
}

impl App {
    pub fn new(target: String) -> Result<Self> {
        let repo_root = git_root()?;
        let entries = repo_tree_entries(&repo_root)?;

        let mut app = Self {
            repo_root,
            target,
            entries,
            cursor: 0,
            git_preview: String::new(),
            rsync_preview: String::from(
                "No selected files.\n\nSelect files to see the expected rsync changes against the destination.",
            ),
            content_diff_preview: String::from(
                "No selected file content diff.\n\nSelect a file to compare source content against the destination.",
            ),
            git_scroll: 0,
            rsync_scroll: 0,
            content_diff_scroll: 0,
            rsync_dry_run_ready: false,
            rsync_preview_stale: false,
            rsync_report: None,
            message: String::from(
                "Space: select | d: refresh rsync diff | r: run | Tab: focus pane | q: quit",
            ),
            active_pane: PreviewPane::Repository,
            should_quit: false,
        };

        app.refresh_git_preview();
        Ok(app)
    }

    pub fn selected_count(&self) -> usize {
        self.selected_candidates().len()
    }

    pub fn move_down(&mut self) {
        let visible = self.visible_indices();

        if visible.is_empty() {
            return;
        }

        self.cursor = (self.cursor + 1) % visible.len();
        self.refresh_git_preview();
        self.render_rsync_preview_for_cursor();
    }

    pub fn move_up(&mut self) {
        let visible = self.visible_indices();

        if visible.is_empty() {
            return;
        }

        if self.cursor == 0 {
            self.cursor = visible.len() - 1;
        } else {
            self.cursor -= 1;
        }

        self.refresh_git_preview();
        self.render_rsync_preview_for_cursor();
    }

    pub fn set_cursor(&mut self, cursor: usize) {
        if cursor >= self.visible_indices().len() {
            return;
        }

        self.cursor = cursor;
        self.refresh_git_preview();
        self.render_rsync_preview_for_cursor();
    }

    pub fn set_active_pane(&mut self, pane: PreviewPane) {
        self.active_pane = pane;
    }

    pub fn toggle_current(&mut self) {
        let Some(index) = self.current_entry_index() else {
            return;
        };

        self.toggle_entry_selection(index);
        self.refresh_rsync_preview(false);
    }

    pub fn toggle_candidate_at(&mut self, visible_index: usize) {
        let Some(entry_index) = self.visible_indices().get(visible_index).copied() else {
            return;
        };

        self.cursor = visible_index;
        self.refresh_git_preview();
        self.toggle_entry_selection(entry_index);
        self.refresh_rsync_preview(false);
    }

    pub fn toggle_all(&mut self) {
        let all_selected = self.entries.iter().all(|entry| entry.selected);

        for entry in &mut self.entries {
            entry.selected = !all_selected;
        }

        self.refresh_rsync_preview(false);
    }

    pub fn toggle_current_expanded(&mut self) {
        let Some(index) = self.current_entry_index() else {
            return;
        };

        if self.entries[index].kind == EntryKind::Directory {
            self.entries[index].expanded = !self.entries[index].expanded;
            self.clamp_cursor();
        }
    }

    pub fn expand_current(&mut self) {
        let Some(index) = self.current_entry_index() else {
            return;
        };

        if self.entries[index].kind == EntryKind::Directory {
            self.entries[index].expanded = true;
        }
    }

    pub fn collapse_current(&mut self) {
        let Some(index) = self.current_entry_index() else {
            return;
        };

        if self.entries[index].kind == EntryKind::Directory {
            self.entries[index].expanded = false;
            self.clamp_cursor();
        }
    }

    pub fn toggle_active_pane(&mut self) {
        self.active_pane = match self.active_pane {
            PreviewPane::Repository => PreviewPane::Content,
            PreviewPane::Content => PreviewPane::Git,
            PreviewPane::Git => PreviewPane::Rsync,
            PreviewPane::Rsync => PreviewPane::Repository,
        };
    }

    pub fn scroll_preview_down(&mut self) {
        match self.active_pane {
            PreviewPane::Repository => self.scroll_repository_down(),
            PreviewPane::Content => {
                self.content_diff_scroll = self.content_diff_scroll.saturating_add(10)
            }
            PreviewPane::Git => self.git_scroll = self.git_scroll.saturating_add(10),
            PreviewPane::Rsync => self.rsync_scroll = self.rsync_scroll.saturating_add(10),
        }
    }

    pub fn scroll_preview_up(&mut self) {
        match self.active_pane {
            PreviewPane::Repository => self.scroll_repository_up(),
            PreviewPane::Content => {
                self.content_diff_scroll = self.content_diff_scroll.saturating_sub(10)
            }
            PreviewPane::Git => self.git_scroll = self.git_scroll.saturating_sub(10),
            PreviewPane::Rsync => self.rsync_scroll = self.rsync_scroll.saturating_sub(10),
        }
    }

    pub fn scroll_repository_down(&mut self) {
        let visible_len = self.visible_indices().len();

        if visible_len == 0 {
            return;
        }

        self.set_cursor(
            self.cursor
                .saturating_add(REPOSITORY_SCROLL_STEP)
                .min(visible_len - 1),
        );
    }

    pub fn scroll_repository_up(&mut self) {
        self.set_cursor(self.cursor.saturating_sub(REPOSITORY_SCROLL_STEP));
    }

    pub fn refresh_candidates(&mut self) -> Result<()> {
        let selected_paths: HashSet<String> = self
            .entries
            .iter()
            .filter(|entry| entry.selected)
            .map(|entry| entry.path.clone())
            .collect();
        let selected_folder_paths: Vec<String> = self
            .entries
            .iter()
            .filter(|entry| entry.selected && entry.is_dir())
            .map(|entry| entry.path.clone())
            .collect();
        let expanded_paths: HashSet<String> = self
            .entries
            .iter()
            .filter(|entry| entry.expanded)
            .map(|entry| entry.path.clone())
            .collect();
        let current_path = self.current_entry().map(|entry| entry.path.clone());

        let mut entries = repo_tree_entries(&self.repo_root)?;

        for entry in &mut entries {
            entry.selected = selected_paths.contains(&entry.path)
                || selected_folder_paths
                    .iter()
                    .any(|folder| entry.path.starts_with(&format!("{folder}/")));
            entry.expanded = expanded_paths.contains(&entry.path) || !entry.is_dir();
        }

        self.entries = entries;
        self.cursor = current_path
            .and_then(|path| {
                self.visible_indices()
                    .iter()
                    .position(|index| self.entries[*index].path == path)
            })
            .unwrap_or_else(|| {
                self.cursor
                    .min(self.visible_indices().len().saturating_sub(1))
            });

        self.clamp_cursor();

        self.refresh_git_preview();
        let selected_count = self.selected_count();
        let rsync_preview_ok = self.refresh_rsync_preview(false);

        if selected_count == 0 || rsync_preview_ok {
            self.message = format!(
                "Auto-refreshed: {} change(s), {} selected.",
                self.entries.len(),
                selected_count
            );
        } else {
            self.message = format!(
                "Auto-refreshed: {} change(s), {} selected. Rsync diff failed.",
                self.entries.len(),
                selected_count
            );
        }

        Ok(())
    }

    pub fn set_message(&mut self, message: impl Into<String>) {
        self.message = message.into();
    }

    pub fn refresh_git_preview(&mut self) {
        self.git_scroll = 0;

        let Some(entry) = self.current_entry().cloned() else {
            self.git_preview = String::from("No files found.");
            return;
        };

        if entry.is_dir() {
            self.git_preview = folder_preview(&entry, self.folder_child_count(&entry.path));
            return;
        }

        let Some(candidate) = entry.deploy_candidate() else {
            self.git_preview = String::from("No file preview available.");
            return;
        };

        self.git_preview = match preview_candidate(&self.repo_root, &candidate) {
            Ok(text) => text,
            Err(err) => format!("Failed to create git preview:\n{err:#}"),
        };
    }

    pub fn refresh_rsync_preview(&mut self, focus: bool) -> bool {
        let selected = self.selected_candidates();

        if selected.is_empty() {
            self.rsync_preview = String::from(
                "No selected files.\n\nSelect files to see the expected rsync changes against the destination.",
            );
            self.rsync_scroll = 0;
            self.content_diff_preview = String::from(
                "No selected file content diff.\n\nSelect a file to compare source content against the destination.",
            );
            self.content_diff_scroll = 0;
            self.rsync_dry_run_ready = false;
            self.rsync_preview_stale = false;
            self.rsync_report = None;

            if focus {
                self.message = String::from("No selected files.");
                self.active_pane = PreviewPane::Content;
            }

            return false;
        }

        self.rsync_preview_stale = true;
        self.rsync_dry_run_ready = false;

        match rsync_selected(&self.repo_root, &self.target, &selected, true) {
            Ok(report) => {
                let focus_path = self.current_file_path();
                self.rsync_preview = report.render_status(focus_path.as_deref());
                self.content_diff_preview = report.render_content_diff(focus_path.as_deref());
                self.rsync_report = Some(report);
                self.rsync_scroll = 0;
                self.content_diff_scroll = 0;
                self.rsync_dry_run_ready = true;
                self.rsync_preview_stale = false;

                if focus {
                    self.active_pane = PreviewPane::Content;
                }

                self.message = format!(
                    "Rsync diff refreshed for {} file(s). Review it before running.",
                    selected.len()
                );

                true
            }
            Err(err) => {
                self.rsync_preview = format!("Rsync diff failed:\n\n{err:#}");
                self.content_diff_preview = String::from("Rsync diff failed.");
                self.rsync_scroll = 0;
                self.content_diff_scroll = 0;
                self.rsync_dry_run_ready = false;
                self.rsync_preview_stale = true;
                self.rsync_report = None;

                if focus {
                    self.active_pane = PreviewPane::Content;
                }

                self.message = String::from("Rsync diff failed. Deploy is blocked.");

                false
            }
        }
    }

    pub fn deploy_rsync(&mut self) {
        let selected = self.selected_candidates();

        if selected.is_empty() {
            self.message = String::from("No selected files.");
            self.active_pane = PreviewPane::Content;
            return;
        }

        if self.rsync_preview_stale || !self.rsync_dry_run_ready {
            if self.refresh_rsync_preview(true) {
                self.message = String::from("Review the rsync diff, then press r again to deploy.");
            }
            return;
        }

        match rsync_selected(&self.repo_root, &self.target, &selected, false) {
            Ok(report) => {
                let focus_path = self.current_file_path();
                self.rsync_preview = report.render_status(focus_path.as_deref());
                self.content_diff_preview = report.render_content_diff(focus_path.as_deref());
                self.rsync_report = Some(report);
                self.rsync_scroll = 0;
                self.content_diff_scroll = 0;
                self.rsync_dry_run_ready = false;
                self.rsync_preview_stale = true;
                self.active_pane = PreviewPane::Content;
                self.message = format!("Rsync completed for {} file(s).", selected.len());
            }
            Err(err) => {
                self.rsync_preview = format!("Rsync failed:\n\n{err:#}");
                self.content_diff_preview = String::from("Rsync failed.");
                self.rsync_scroll = 0;
                self.content_diff_scroll = 0;
                self.rsync_dry_run_ready = false;
                self.rsync_preview_stale = true;
                self.rsync_report = None;
                self.active_pane = PreviewPane::Content;
                self.message = String::from("Rsync failed.");
            }
        }
    }

    fn render_rsync_preview_for_cursor(&mut self) {
        let Some(report) = &self.rsync_report else {
            return;
        };
        let focus_path = self.current_file_path();

        self.rsync_preview = report.render_status(focus_path.as_deref());
        self.content_diff_preview = report.render_content_diff(focus_path.as_deref());
        self.rsync_scroll = 0;
        self.content_diff_scroll = 0;
    }

    pub fn visible_indices(&self) -> Vec<usize> {
        let mut visible = Vec::new();
        let mut collapsed_depths = VecDeque::new();

        for (index, entry) in self.entries.iter().enumerate() {
            while collapsed_depths
                .front()
                .is_some_and(|depth| entry.depth <= *depth)
            {
                collapsed_depths.pop_front();
            }

            if collapsed_depths.is_empty() {
                visible.push(index);

                if entry.is_dir() && !entry.expanded {
                    collapsed_depths.push_front(entry.depth);
                }
            }
        }

        visible
    }

    pub fn current_entry(&self) -> Option<&TreeEntry> {
        self.current_entry_index()
            .and_then(|index| self.entries.get(index))
    }

    pub fn current_entry_index(&self) -> Option<usize> {
        self.visible_indices().get(self.cursor).copied()
    }

    fn toggle_entry_selection(&mut self, index: usize) {
        let selected = !self.entries[index].selected;
        let path = self.entries[index].path.clone();
        let depth = self.entries[index].depth;
        let is_dir = self.entries[index].is_dir();

        self.entries[index].selected = selected;

        if is_dir {
            for entry in self
                .entries
                .iter_mut()
                .skip(index + 1)
                .take_while(|entry| entry.depth > depth)
            {
                if entry.path.starts_with(&format!("{path}/")) {
                    entry.selected = selected;
                }
            }
        }
    }

    fn selected_candidates(&self) -> Vec<Candidate> {
        let mut selected = Vec::new();
        let mut seen = HashSet::new();

        for entry in &self.entries {
            if !entry.selected || entry.is_dir() {
                continue;
            }

            if let Some(candidate) = entry.deploy_candidate()
                && seen.insert(candidate.path.clone())
            {
                selected.push(candidate);
            }
        }

        selected
    }

    fn current_file_path(&self) -> Option<String> {
        self.current_entry()
            .filter(|entry| !entry.is_dir())
            .map(|entry| entry.path.clone())
    }

    fn folder_child_count(&self, path: &str) -> usize {
        let prefix = format!("{path}/");

        self.entries
            .iter()
            .filter(|entry| entry.path.starts_with(&prefix) && !entry.is_dir())
            .count()
    }

    fn clamp_cursor(&mut self) {
        let visible_len = self.visible_indices().len();

        if visible_len == 0 {
            self.cursor = 0;
        } else {
            self.cursor = self.cursor.min(visible_len - 1);
        }
    }
}

fn folder_preview(entry: &TreeEntry, file_count: usize) -> String {
    let mut preview = String::new();

    preview.push_str(&format!("path:   {}\n", entry.path));
    preview.push_str("kind:   folder\n");
    preview.push_str(&format!(
        "git:    {}\n",
        entry.git_kind.map(ChangeKind::label).unwrap_or("clean")
    ));
    preview.push_str(&format!("files:  {file_count}\n\n"));
    preview.push_str(
        "Folder selection includes all descendant files in the rsync preview and deploy.\n",
    );
    preview.push_str("Use Enter/e or Left/Right to collapse and expand folders.\n");

    preview
}
