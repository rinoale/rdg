use std::{
    fs,
    io::{self, Stdout, Write},
    path::{Path, PathBuf},
    process::Command,
    time::Duration,
};

use anyhow::{bail, Context, Result};
use clap::Parser;
use crossterm::{
    event::{self, Event, KeyCode, KeyEventKind},
    execute,
    terminal::{disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen},
};
use ratatui::{
    backend::CrosstermBackend,
    layout::{Constraint, Direction, Layout},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, List, ListItem, ListState, Paragraph, Wrap},
    Frame, Terminal,
};
use tempfile::NamedTempFile;

#[derive(Parser, Debug)]
#[command(
    name = "rdg",
    version,
    about = "Remote deploy selected git changes via rsync"
)]
struct Cli {
    /// Rsync destination, for example:
    /// user@example.com:/var/www/my-app/
    ///
    /// You can also set RDG_TARGET.
    #[arg(value_name = "TARGET", env = "RDG_TARGET")]
    target: Option<String>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum ChangeKind {
    Changed,
    Untracked,
    Deleted,
}

impl ChangeKind {
    fn label(self) -> &'static str {
        match self {
            ChangeKind::Changed => "changed",
            ChangeKind::Untracked => "new",
            ChangeKind::Deleted => "deleted",
        }
    }

    fn short(self) -> &'static str {
        match self {
            ChangeKind::Changed => "M",
            ChangeKind::Untracked => "?",
            ChangeKind::Deleted => "D",
        }
    }

    fn style(self) -> Style {
        match self {
            ChangeKind::Changed => Style::default().fg(Color::Yellow),
            ChangeKind::Untracked => Style::default().fg(Color::Green),
            ChangeKind::Deleted => Style::default().fg(Color::Red),
        }
    }
}

#[derive(Clone, Debug)]
struct Candidate {
    path: String,
    kind: ChangeKind,
    selected: bool,
    status: String,
}

struct App {
    repo_root: PathBuf,
    target: String,
    candidates: Vec<Candidate>,
    cursor: usize,
    preview: String,
    preview_scroll: u16,
    message: String,
    should_quit: bool,
}

impl App {
    fn new(target: String) -> Result<Self> {
        let repo_root = git_root()?;
        let candidates = git_candidates(&repo_root)?;

        let mut app = Self {
            repo_root,
            target,
            candidates,
            cursor: 0,
            preview: String::new(),
            preview_scroll: 0,
            message: String::from("Space: select | d: dry-run | r: run | q: quit"),
            should_quit: false,
        };

        app.refresh_preview();
        Ok(app)
    }

    fn current(&self) -> Option<&Candidate> {
        self.candidates.get(self.cursor)
    }

    fn selected_count(&self) -> usize {
        self.candidates.iter().filter(|c| c.selected).count()
    }

    fn move_down(&mut self) {
        if self.candidates.is_empty() {
            return;
        }

        self.cursor = (self.cursor + 1) % self.candidates.len();
        self.refresh_preview();
    }

    fn move_up(&mut self) {
        if self.candidates.is_empty() {
            return;
        }

        if self.cursor == 0 {
            self.cursor = self.candidates.len() - 1;
        } else {
            self.cursor -= 1;
        }

        self.refresh_preview();
    }

    fn toggle_current(&mut self) {
        if let Some(candidate) = self.candidates.get_mut(self.cursor) {
            candidate.selected = !candidate.selected;
        }
    }

    fn toggle_all(&mut self) {
        let all_selected = self.candidates.iter().all(|c| c.selected);

        for candidate in &mut self.candidates {
            candidate.selected = !all_selected;
        }
    }

    fn scroll_preview_down(&mut self) {
        self.preview_scroll = self.preview_scroll.saturating_add(10);
    }

    fn scroll_preview_up(&mut self) {
        self.preview_scroll = self.preview_scroll.saturating_sub(10);
    }

    fn refresh_preview(&mut self) {
        self.preview_scroll = 0;

        let Some(candidate) = self.current().cloned() else {
            self.preview = String::from("No git changes found.");
            return;
        };

        self.preview = match preview_candidate(&self.repo_root, &candidate) {
            Ok(text) => text,
            Err(err) => format!("Failed to create preview:\n{err:#}"),
        };
    }

    fn run_rsync(&mut self, dry_run: bool) {
        let selected: Vec<Candidate> = self
            .candidates
            .iter()
            .filter(|c| c.selected)
            .cloned()
            .collect();

        if selected.is_empty() {
            self.message = String::from("No selected files.");
            return;
        }

        match rsync_selected(&self.repo_root, &self.target, &selected, dry_run) {
            Ok(output) => {
                self.preview = output;
                self.preview_scroll = 0;

                if dry_run {
                    self.message = format!("Dry-run completed for {} file(s).", selected.len());
                } else {
                    self.message = format!("Rsync completed for {} file(s).", selected.len());
                }
            }
            Err(err) => {
                self.preview = format!("Rsync failed:\n\n{err:#}");
                self.preview_scroll = 0;

                if dry_run {
                    self.message = String::from("Dry-run failed.");
                } else {
                    self.message = String::from("Rsync failed.");
                }
            }
        }
    }
}

fn main() -> Result<()> {
    let cli = Cli::parse();

    let target = cli.target.context(
        "Missing target.\n\nUsage:\n  rdg user@example.com:/var/www/my-app/\n\nOr:\n  RDG_TARGET=user@example.com:/var/www/my-app/ rdg",
    )?;

    let app = App::new(target)?;

    let mut terminal = setup_terminal()?;
    let result = run_app(&mut terminal, app);
    restore_terminal(&mut terminal)?;

    result
}

fn setup_terminal() -> Result<Terminal<CrosstermBackend<Stdout>>> {
    enable_raw_mode()?;

    let mut stdout = io::stdout();
    execute!(stdout, EnterAlternateScreen)?;

    let backend = CrosstermBackend::new(stdout);
    let mut terminal = Terminal::new(backend)?;
    terminal.clear()?;

    Ok(terminal)
}

fn restore_terminal(terminal: &mut Terminal<CrosstermBackend<Stdout>>) -> Result<()> {
    disable_raw_mode()?;
    execute!(terminal.backend_mut(), LeaveAlternateScreen)?;
    terminal.show_cursor()?;
    Ok(())
}

fn run_app(terminal: &mut Terminal<CrosstermBackend<Stdout>>, mut app: App) -> Result<()> {
    loop {
        terminal.draw(|frame| draw_ui(frame, &app))?;

        if app.should_quit {
            break;
        }

        if event::poll(Duration::from_millis(200))? {
            let Event::Key(key) = event::read()? else {
                continue;
            };

            if key.kind != KeyEventKind::Press {
                continue;
            }

            match key.code {
                KeyCode::Char('q') | KeyCode::Esc => {
                    app.should_quit = true;
                }
                KeyCode::Down | KeyCode::Char('j') => {
                    app.move_down();
                }
                KeyCode::Up | KeyCode::Char('k') => {
                    app.move_up();
                }
                KeyCode::Char(' ') => {
                    app.toggle_current();
                }
                KeyCode::Char('a') => {
                    app.toggle_all();
                }
                KeyCode::Char('d') => {
                    app.run_rsync(true);
                }
                KeyCode::Char('r') => {
                    app.run_rsync(false);
                }
                KeyCode::PageDown => {
                    app.scroll_preview_down();
                }
                KeyCode::PageUp => {
                    app.scroll_preview_up();
                }
                _ => {}
            }
        }
    }

    Ok(())
}

fn draw_ui(frame: &mut Frame, app: &App) {
    let root = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(3),
            Constraint::Min(1),
            Constraint::Length(2),
        ])
        .split(frame.area());

    let body = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([Constraint::Percentage(42), Constraint::Percentage(58)])
        .split(root[1]);

    let header = Paragraph::new(Line::from(vec![
        Span::styled(
            "rdg",
            Style::default()
                .fg(Color::Cyan)
                .add_modifier(Modifier::BOLD),
        ),
        Span::raw(" | target: "),
        Span::styled(&app.target, Style::default().fg(Color::White)),
        Span::raw(format!(
            " | selected: {}/{}",
            app.selected_count(),
            app.candidates.len()
        )),
    ]))
    .block(Block::default().borders(Borders::ALL).title("Remote deploy based on git"));

    frame.render_widget(header, root[0]);

    let items: Vec<ListItem> = app
        .candidates
        .iter()
        .map(|candidate| {
            let checkbox = if candidate.selected { "[x]" } else { "[ ]" };

            let line = Line::from(vec![
                Span::raw(format!("{checkbox} ")),
                Span::styled(
                    format!("{:<1} {:<8}", candidate.kind.short(), candidate.kind.label()),
                    candidate.kind.style(),
                ),
                Span::raw(" "),
                Span::raw(candidate.path.clone()),
            ]);

            ListItem::new(line)
        })
        .collect();

    let list = List::new(items)
        .block(Block::default().borders(Borders::ALL).title("Candidates"))
        .highlight_symbol("> ")
        .highlight_style(
            Style::default()
                .bg(Color::DarkGray)
                .add_modifier(Modifier::BOLD),
        );

    let mut list_state = ListState::default();

    if !app.candidates.is_empty() {
        list_state.select(Some(app.cursor));
    }

    frame.render_stateful_widget(list, body[0], &mut list_state);

    let preview_title = app
        .current()
        .map(|c| format!("Preview: {}", c.path))
        .unwrap_or_else(|| String::from("Preview"));

    let preview = Paragraph::new(app.preview.as_str())
        .block(Block::default().borders(Borders::ALL).title(preview_title))
        .wrap(Wrap { trim: false })
        .scroll((app.preview_scroll, 0));

    frame.render_widget(preview, body[1]);

    let footer = Paragraph::new(format!(
        "↑/↓ or j/k: move | Space: select | a: all | d: dry-run | r: run | PgUp/PgDn: scroll | q: quit    {}",
        app.message
    ))
    .block(Block::default().borders(Borders::ALL));

    frame.render_widget(footer, root[2]);
}

fn git_root() -> Result<PathBuf> {
    let output = Command::new("git")
        .args(["rev-parse", "--show-toplevel"])
        .output()
        .context("failed to run git rev-parse")?;

    if !output.status.success() {
        bail!(
            "not inside a git repository:\n{}",
            String::from_utf8_lossy(&output.stderr)
        );
    }

    let root = String::from_utf8(output.stdout)
        .context("git root path was not valid UTF-8")?
        .trim()
        .to_string();

    Ok(PathBuf::from(root))
}

fn git_candidates(repo_root: &Path) -> Result<Vec<Candidate>> {
    let output = Command::new("git")
        .current_dir(repo_root)
        .args([
            "status",
            "--porcelain=v1",
            "-z",
            "--untracked-files=all",
        ])
        .output()
        .context("failed to run git status")?;

    if !output.status.success() {
        bail!(
            "git status failed:\n{}",
            String::from_utf8_lossy(&output.stderr)
        );
    }

    let chunks: Vec<&[u8]> = output
        .stdout
        .split(|byte| *byte == 0)
        .filter(|chunk| !chunk.is_empty())
        .collect();

    let mut candidates = Vec::new();
    let mut i = 0;

    while i < chunks.len() {
        let entry = String::from_utf8_lossy(chunks[i]).into_owned();

        if entry.len() < 4 {
            i += 1;
            continue;
        }

        let status = entry[0..2].to_string();
        let path = entry[3..].to_string();

        let x = status.as_bytes()[0] as char;
        let y = status.as_bytes()[1] as char;

        let kind = if x == '?' && y == '?' {
            ChangeKind::Untracked
        } else if x == 'D' || y == 'D' {
            ChangeKind::Deleted
        } else {
            ChangeKind::Changed
        };

        candidates.push(Candidate {
            path,
            kind,
            selected: false,
            status: status.clone(),
        });

        // In porcelain v1 + -z, rename/copy entries have an extra NUL field
        // containing the original path. For renames, add the old path as a
        // separate deletion candidate so the remote old file can be removed.
        let is_rename = x == 'R' || y == 'R';
        let is_copy = x == 'C' || y == 'C';

        if (is_rename || is_copy) && i + 1 < chunks.len() {
            let old_path = String::from_utf8_lossy(chunks[i + 1]).into_owned();

            if is_rename {
                candidates.push(Candidate {
                    path: old_path,
                    kind: ChangeKind::Deleted,
                    selected: false,
                    status: String::from("R-old"),
                });
            }

            i += 1;
        }

        i += 1;
    }

    candidates.sort_by(|a, b| a.path.cmp(&b.path));

    Ok(candidates)
}

fn preview_candidate(repo_root: &Path, candidate: &Candidate) -> Result<String> {
    let mut header = String::new();

    header.push_str(&format!("path:   {}\n", candidate.path));
    header.push_str(&format!("kind:   {}\n", candidate.kind.label()));
    header.push_str(&format!("status: {}\n", candidate.status));
    header.push_str(&format!("{}\n\n", "-".repeat(80)));

    let body = match candidate.kind {
        ChangeKind::Untracked => preview_untracked_file(repo_root, &candidate.path)?,
        ChangeKind::Changed | ChangeKind::Deleted => {
            preview_git_diff(repo_root, &candidate.path)?
        }
    };

    Ok(truncate_text(format!("{header}{body}"), 200_000))
}

fn preview_git_diff(repo_root: &Path, path: &str) -> Result<String> {
    let output = Command::new("git")
        .current_dir(repo_root)
        .args(["diff", "--no-ext-diff", "--color=never", "HEAD", "--"])
        .arg(path)
        .output()
        .context("failed to run git diff")?;

    let stdout = String::from_utf8_lossy(&output.stdout).to_string();
    let stderr = String::from_utf8_lossy(&output.stderr).to_string();

    if !output.status.success() && stdout.trim().is_empty() {
        return Ok(format!("git diff failed:\n{stderr}"));
    }

    if stdout.trim().is_empty() {
        Ok(String::from(
            "No textual git diff output.\n\nThis may be a mode-only change, binary file, or a repository without HEAD.",
        ))
    } else {
        Ok(stdout)
    }
}

fn preview_untracked_file(repo_root: &Path, path: &str) -> Result<String> {
    let full_path = repo_root.join(path);

    let metadata = fs::metadata(&full_path)
        .with_context(|| format!("failed to read metadata for {}", full_path.display()))?;

    if metadata.is_dir() {
        return Ok(format!(
            "NEW DIRECTORY\n\n{}\n\nGit usually reports individual files when --untracked-files=all is used.",
            path
        ));
    }

    let bytes = fs::read(&full_path)
        .with_context(|| format!("failed to read {}", full_path.display()))?;

    if bytes.iter().take(8192).any(|byte| *byte == 0) {
        return Ok(format!(
            "NEW BINARY FILE\n\n{}\n\n{} bytes",
            path,
            bytes.len()
        ));
    }

    let truncated = bytes.len() > 200_000;
    let visible_bytes = if truncated {
        &bytes[..200_000]
    } else {
        &bytes[..]
    };

    let text = String::from_utf8_lossy(visible_bytes);

    let mut preview = String::new();
    preview.push_str("NEW FILE PREVIEW\n\n");

    for line in text.lines().take(500) {
        preview.push_str("+ ");
        preview.push_str(line);
        preview.push('\n');
    }

    if truncated {
        preview.push_str("\n... file preview truncated ...\n");
    }

    Ok(preview)
}

fn rsync_selected(
    repo_root: &Path,
    target: &str,
    selected: &[Candidate],
    dry_run: bool,
) -> Result<String> {
    let mut files_from = NamedTempFile::new().context("failed to create rsync file list")?;

    for candidate in selected {
        files_from
            .write_all(candidate.path.as_bytes())
            .context("failed to write path to rsync file list")?;
        files_from
            .write_all(&[0])
            .context("failed to write NUL delimiter to rsync file list")?;
    }

    files_from.flush()?;

    let flags = if dry_run { "-ain" } else { "-ai" };

    let output = Command::new("rsync")
        .current_dir(repo_root)
        .arg(flags)
        .arg("--from0")
        .arg(format!("--files-from={}", files_from.path().display()))
        .arg("--delete-missing-args")
        .arg("./")
        .arg(target)
        .output()
        .context("failed to execute rsync. Is rsync installed and available in PATH?")?;

    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);

    let mode = if dry_run { "DRY RUN" } else { "RUN" };

    let mut report = String::new();

    report.push_str(&format!("{mode}\n"));
    report.push_str(&format!("repo:   {}\n", repo_root.display()));
    report.push_str(&format!("target: {target}\n"));
    report.push_str(&format!("files:  {}\n\n", selected.len()));
    report.push_str(&format!(
        "$ rsync {flags} --from0 --files-from=<tempfile> --delete-missing-args ./ {target}\n\n"
    ));

    report.push_str("SELECTED FILES\n");
    report.push_str("--------------\n");

    for candidate in selected {
        report.push_str(&format!(
            "{} {:<8} {}\n",
            candidate.kind.short(),
            candidate.kind.label(),
            candidate.path
        ));
    }

    report.push_str("\nRSYNC STDOUT\n");
    report.push_str("------------\n");

    if stdout.trim().is_empty() {
        report.push_str("(empty)\n");
    } else {
        report.push_str(&stdout);
    }

    report.push_str("\nRSYNC STDERR\n");
    report.push_str("------------\n");

    if stderr.trim().is_empty() {
        report.push_str("(empty)\n");
    } else {
        report.push_str(&stderr);
    }

    if !output.status.success() {
        bail!("rsync exited with status {}\n\n{}", output.status, report);
    }

    Ok(truncate_text(report, 200_000))
}

fn truncate_text(mut text: String, max_bytes: usize) -> String {
    if text.len() <= max_bytes {
        return text;
    }

    let mut end = max_bytes;

    while !text.is_char_boundary(end) {
        end -= 1;
    }

    text.truncate(end);
    text.push_str("\n\n... truncated ...\n");
    text
}
