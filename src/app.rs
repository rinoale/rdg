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
use crate::tui::{
    command::{self, Command, ThemeCommand},
    keymap::Keymap,
    theme::ThemeKind,
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
    pub repository_offset: usize,
    pub selected_only: bool,
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
    pub command_input: String,
    pub command_mode: bool,
    pub command_history: Vec<String>,
    pub command_history_cursor: Option<usize>,
    pub show_help: bool,
    pub theme_kind: ThemeKind,
    pub active_pane: PreviewPane,
    pub keymap: Keymap,
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
            repository_offset: 0,
            selected_only: false,
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
            message: String::from("Press : for commands, ? for help, or Tab to change focus."),
            command_input: String::new(),
            command_mode: false,
            command_history: Vec::new(),
            command_history_cursor: None,
            show_help: false,
            theme_kind: ThemeKind::Neutral,
            active_pane: PreviewPane::Repository,
            keymap: Keymap::default(),
            should_quit: false,
        };

        app.refresh_git_preview();
        Ok(app)
    }

    pub fn selected_count(&self) -> usize {
        self.selected_candidates().len()
    }

    pub fn should_quit(&self) -> bool {
        self.should_quit
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

    pub fn enter_command_mode(&mut self) {
        self.command_mode = true;
        self.command_input.clear();
        self.command_input.push(':');
        self.command_history_cursor = None;
        self.message = String::from("command mode");
    }

    pub fn cancel_transient_state(&mut self) {
        if self.show_help {
            self.show_help = false;
            self.message = String::from("help closed");
        } else if self.command_mode {
            self.command_mode = false;
            self.command_input.clear();
            self.command_history_cursor = None;
            self.message = String::from("command canceled");
        } else {
            self.message = String::from("nothing to cancel");
        }
    }

    pub fn push_command_char(&mut self, ch: char) {
        if self.command_mode {
            self.command_input.push(ch);
            self.command_history_cursor = None;
        }
    }

    pub fn pop_command_char(&mut self) {
        if self.command_mode && self.command_input.len() > 1 {
            self.command_input.pop();
            self.command_history_cursor = None;
        }
    }

    pub fn previous_command(&mut self) {
        if !self.command_mode || self.command_history.is_empty() {
            return;
        }

        let index = self
            .command_history_cursor
            .map_or(self.command_history.len().saturating_sub(1), |cursor| {
                cursor.saturating_sub(1)
            });
        self.command_history_cursor = Some(index);
        self.command_input = self.command_history[index].clone();
    }

    pub fn next_command(&mut self) {
        if !self.command_mode {
            return;
        }

        let Some(cursor) = self.command_history_cursor else {
            return;
        };

        if cursor + 1 < self.command_history.len() {
            self.command_history_cursor = Some(cursor + 1);
            self.command_input = self.command_history[cursor + 1].clone();
        } else {
            self.command_history_cursor = None;
            self.command_input.clear();
            self.command_input.push(':');
        }
    }

    pub fn submit_command(&mut self) {
        let command = self.command_input.trim().to_string();
        self.command_mode = false;
        self.command_input.clear();
        self.command_history_cursor = None;

        if command.is_empty() || command == ":" {
            self.message = String::from("empty command");
            return;
        }

        self.command_history.push(command.clone());
        self.run_command(&command);
    }

    pub fn run_command(&mut self, input: &str) {
        match command::parse(input) {
            Command::Quit { force: _ } => self.should_quit = true,
            Command::Help => self.show_help(),
            Command::Theme(theme) => self.set_theme(theme),
            Command::Refresh => {
                if let Err(err) = self.refresh_candidates() {
                    self.message = format!("Refresh failed: {err:#}");
                }
            }
            Command::Diff => {
                self.refresh_rsync_preview(true);
            }
            Command::Deploy => self.deploy_rsync(),
            Command::Select => self.toggle_current(),
            Command::SelectAll => self.toggle_all(),
            Command::SelectedOnly => {
                if !self.selected_only {
                    self.toggle_selected_only();
                } else {
                    self.message = String::from("Already showing selected files only.");
                }
            }
            Command::Tree => {
                if self.selected_only {
                    self.toggle_selected_only();
                } else {
                    self.message = String::from("Already showing the repository tree.");
                }
            }
            Command::Focus(pane) => {
                self.active_pane = pane;
                self.message = format!("Focused {} pane.", pane.label());
            }
            Command::Expand => self.expand_current(),
            Command::Collapse => self.collapse_current(),
            Command::Unknown(name) => {
                self.message = if name == "focus" {
                    String::from("usage: :focus repository|content|git|rsync")
                } else {
                    format!("unknown rdg command: :{name}")
                };
            }
            Command::Empty => {
                self.message = String::from("empty command");
            }
        }
    }

    pub fn toggle_current(&mut self) {
        let Some(index) = self.current_entry_index() else {
            return;
        };

        self.toggle_entry_selection(index);
        self.clamp_cursor();
        if self.selected_only {
            self.refresh_git_preview();
        }
        self.refresh_rsync_preview(false);
    }

    pub fn toggle_candidate_at(&mut self, visible_index: usize) {
        let Some(entry_index) = self.visible_indices().get(visible_index).copied() else {
            return;
        };

        self.cursor = visible_index;
        self.toggle_entry_selection(entry_index);
        self.clamp_cursor();
        self.refresh_git_preview();
        self.refresh_rsync_preview(false);
    }

    pub fn toggle_all(&mut self) {
        let all_selected = self.entries.iter().all(|entry| entry.selected);

        for entry in &mut self.entries {
            entry.selected = !all_selected;
        }

        self.clamp_cursor();
        if self.selected_only {
            self.refresh_git_preview();
        }
        self.refresh_rsync_preview(false);
    }

    pub fn toggle_selected_only(&mut self) {
        self.selected_only = !self.selected_only;
        self.repository_offset = 0;
        self.clamp_cursor();
        self.refresh_git_preview();
        self.render_rsync_preview_for_cursor();

        self.message = if self.selected_only {
            String::from("Showing selected files only. Press s to return to the repository tree.")
        } else {
            String::from("Showing the repository tree.")
        };
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

    pub fn toggle_active_pane_reverse(&mut self) {
        self.active_pane = match self.active_pane {
            PreviewPane::Repository => PreviewPane::Rsync,
            PreviewPane::Content => PreviewPane::Repository,
            PreviewPane::Git => PreviewPane::Content,
            PreviewPane::Rsync => PreviewPane::Git,
        };
    }

    pub fn show_help(&mut self) {
        self.show_help = true;
        self.message = String::from("help opened");
    }

    fn set_theme(&mut self, theme: ThemeCommand) {
        self.theme_kind = match theme {
            ThemeCommand::Neutral => ThemeKind::Neutral,
            ThemeCommand::Safe => ThemeKind::Safe,
            ThemeCommand::Danger => ThemeKind::Danger,
        };
        self.message = format!("theme: {}", self.theme_kind.theme().name);
    }

    pub fn scroll_preview_down(&mut self) {
        match self.active_pane {
            PreviewPane::Repository => {}
            PreviewPane::Content => {
                self.content_diff_scroll = self.content_diff_scroll.saturating_add(10)
            }
            PreviewPane::Git => self.git_scroll = self.git_scroll.saturating_add(10),
            PreviewPane::Rsync => self.rsync_scroll = self.rsync_scroll.saturating_add(10),
        }
    }

    pub fn scroll_preview_up(&mut self) {
        match self.active_pane {
            PreviewPane::Repository => {}
            PreviewPane::Content => {
                self.content_diff_scroll = self.content_diff_scroll.saturating_sub(10)
            }
            PreviewPane::Git => self.git_scroll = self.git_scroll.saturating_sub(10),
            PreviewPane::Rsync => self.rsync_scroll = self.rsync_scroll.saturating_sub(10),
        }
    }

    pub fn scroll_repository_down(&mut self, viewport_rows: usize) {
        let max_offset = self.max_repository_offset(viewport_rows);

        if viewport_rows == 0 {
            return;
        }

        self.repository_offset = self
            .repository_view_offset(viewport_rows)
            .saturating_add(REPOSITORY_SCROLL_STEP)
            .min(max_offset);
        self.clamp_cursor_to_repository_view(viewport_rows);
    }

    pub fn scroll_repository_up(&mut self, viewport_rows: usize) {
        if viewport_rows == 0 {
            return;
        }

        self.repository_offset = self
            .repository_view_offset(viewport_rows)
            .saturating_sub(REPOSITORY_SCROLL_STEP);
        self.clamp_cursor_to_repository_view(viewport_rows);
    }

    pub fn repository_view_offset(&self, viewport_rows: usize) -> usize {
        self.repository_offset
            .min(self.max_repository_offset(viewport_rows))
    }

    pub fn ensure_repository_cursor_visible(&mut self, viewport_rows: usize) {
        let visible_len = self.visible_indices().len();

        if visible_len == 0 || viewport_rows == 0 {
            self.repository_offset = 0;
            return;
        }

        let max_offset = visible_len.saturating_sub(viewport_rows);
        self.repository_offset = self.repository_offset.min(max_offset);

        if self.cursor < self.repository_offset {
            self.repository_offset = self.cursor.min(max_offset);
            return;
        }

        let last_visible = self
            .repository_offset
            .saturating_add(viewport_rows.saturating_sub(1));

        if self.cursor > last_visible {
            self.repository_offset = self
                .cursor
                .saturating_sub(viewport_rows.saturating_sub(1))
                .min(max_offset);
        }
    }

    fn max_repository_offset(&self, viewport_rows: usize) -> usize {
        if viewport_rows == 0 {
            return 0;
        }

        self.visible_indices().len().saturating_sub(viewport_rows)
    }

    fn clamp_cursor_to_repository_view(&mut self, viewport_rows: usize) {
        let visible_len = self.visible_indices().len();

        if visible_len == 0 || viewport_rows == 0 {
            self.repository_offset = 0;
            self.cursor = 0;
            return;
        }

        let offset = self.repository_view_offset(viewport_rows);
        self.repository_offset = offset;

        let last_visible = offset
            .saturating_add(viewport_rows.saturating_sub(1))
            .min(visible_len - 1);
        let cursor = self.cursor.clamp(offset, last_visible);

        if cursor != self.cursor {
            self.set_cursor(cursor);
        }
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
        self.repository_offset = self.repository_offset.min(self.visible_indices().len());

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
        if self.selected_only {
            return self
                .entries
                .iter()
                .enumerate()
                .filter_map(|(index, entry)| (entry.selected && !entry.is_dir()).then_some(index))
                .collect();
        }

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
            self.repository_offset = 0;
        } else {
            self.cursor = self.cursor.min(visible_len - 1);
            self.repository_offset = self.repository_offset.min(visible_len - 1);
        }
    }
}

impl PreviewPane {
    pub fn label(self) -> &'static str {
        match self {
            PreviewPane::Repository => "repository",
            PreviewPane::Content => "content",
            PreviewPane::Git => "git",
            PreviewPane::Rsync => "rsync",
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

#[cfg(test)]
mod tests {
    use super::*;

    fn app_with_file_count(file_count: usize) -> App {
        App {
            repo_root: PathBuf::new(),
            target: String::new(),
            entries: (0..file_count)
                .map(|index| TreeEntry {
                    path: format!("file-{index}.txt"),
                    name: format!("file-{index}.txt"),
                    kind: EntryKind::File,
                    depth: 0,
                    git_kind: None,
                    status: String::new(),
                    selected: false,
                    expanded: true,
                })
                .collect(),
            cursor: 0,
            repository_offset: 0,
            selected_only: false,
            git_preview: String::new(),
            rsync_preview: String::new(),
            content_diff_preview: String::new(),
            git_scroll: 0,
            rsync_scroll: 0,
            content_diff_scroll: 0,
            rsync_dry_run_ready: false,
            rsync_preview_stale: false,
            rsync_report: None,
            message: String::new(),
            command_input: String::new(),
            command_mode: false,
            command_history: Vec::new(),
            command_history_cursor: None,
            show_help: false,
            theme_kind: ThemeKind::Neutral,
            active_pane: PreviewPane::Repository,
            keymap: Keymap::default(),
            should_quit: false,
        }
    }

    #[test]
    fn repository_view_offset_uses_scroll_state() {
        let mut app = app_with_file_count(20);

        app.repository_offset = 0;
        assert_eq!(app.repository_view_offset(5), 0);

        app.repository_offset = 7;
        assert_eq!(app.repository_view_offset(5), 7);

        app.repository_offset = 30;
        assert_eq!(app.repository_view_offset(5), 15);
        assert_eq!(app.repository_view_offset(30), 0);
        assert_eq!(app.repository_view_offset(0), 0);
    }

    #[test]
    fn repository_cursor_visibility_updates_scroll_state_only_when_needed() {
        let mut app = app_with_file_count(20);

        app.cursor = 2;
        app.repository_offset = 5;
        app.ensure_repository_cursor_visible(5);
        assert_eq!(app.repository_offset, 2);

        app.cursor = 12;
        app.repository_offset = 5;
        app.ensure_repository_cursor_visible(5);
        assert_eq!(app.repository_offset, 8);

        app.cursor = 7;
        app.repository_offset = 5;
        app.ensure_repository_cursor_visible(5);
        assert_eq!(app.repository_offset, 5);
    }

    #[test]
    fn repository_scroll_keeps_cursor_inside_visible_rows() {
        let mut app = app_with_file_count(20);

        app.scroll_repository_down(5);
        assert_eq!(app.repository_offset, 5);
        assert_eq!(app.cursor, 5);

        app.scroll_repository_down(5);
        assert_eq!(app.repository_offset, 10);
        assert_eq!(app.cursor, 10);

        app.scroll_repository_up(5);
        assert_eq!(app.repository_offset, 5);
        assert_eq!(app.cursor, 9);
    }

    #[test]
    fn selected_only_visible_indices_show_selected_files_only() {
        let mut app = app_with_file_count(4);
        app.entries.insert(
            1,
            TreeEntry {
                path: String::from("folder"),
                name: String::from("folder"),
                kind: EntryKind::Directory,
                depth: 0,
                git_kind: None,
                status: String::new(),
                selected: true,
                expanded: false,
            },
        );
        app.entries[0].selected = true;
        app.entries[2].selected = true;
        app.entries[4].selected = true;
        app.selected_only = true;

        assert_eq!(app.visible_indices(), vec![0, 2, 4]);
    }

    #[test]
    fn toggle_selected_only_clamps_cursor_and_resets_repository_scroll() {
        let mut app = app_with_file_count(8);
        app.entries[1].selected = true;
        app.entries[6].selected = true;
        app.cursor = 7;
        app.repository_offset = 5;

        app.toggle_selected_only();

        assert!(app.selected_only);
        assert_eq!(app.visible_indices(), vec![1, 6]);
        assert_eq!(app.cursor, 1);
        assert_eq!(app.repository_offset, 0);
    }

    #[test]
    fn command_mode_submits_quit_command() {
        let mut app = app_with_file_count(1);

        app.enter_command_mode();
        app.push_command_char('q');
        app.submit_command();

        assert!(app.should_quit);
        assert!(!app.command_mode);
        assert_eq!(app.command_history, vec![":q"]);
    }

    #[test]
    fn command_history_moves_between_prior_commands() {
        let mut app = app_with_file_count(1);
        app.command_history = vec![":help".to_string(), ":tree".to_string()];

        app.enter_command_mode();
        app.previous_command();
        assert_eq!(app.command_input, ":tree");

        app.previous_command();
        assert_eq!(app.command_input, ":help");

        app.next_command();
        assert_eq!(app.command_input, ":tree");

        app.next_command();
        assert_eq!(app.command_input, ":");
    }
}
