use ratatui::{
    Frame,
    layout::{Constraint, Direction, Layout, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span, Text},
    widgets::{Block, Borders, List, ListItem, ListState, Paragraph, Wrap},
};

use crate::{
    app::{App, PreviewPane},
    core::{
        git::ChangeKind,
        tree::{EntryKind, TreeEntry},
    },
};

#[derive(Clone, Copy, Debug)]
pub struct UiAreas {
    pub candidates: Rect,
    pub content: Rect,
    pub git: Rect,
    pub rsync: Rect,
}

pub fn draw(frame: &mut Frame, app: &App) {
    let root = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(3),
            Constraint::Min(1),
            Constraint::Length(3),
        ])
        .split(frame.area());

    draw_header(frame, app, root[0]);
    draw_body(frame, app, root[1]);
    draw_footer(frame, app, root[2]);
}

pub fn body_areas(area: Rect) -> UiAreas {
    let root = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(3),
            Constraint::Min(1),
            Constraint::Length(3),
        ])
        .split(area);

    let body = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([Constraint::Percentage(38), Constraint::Percentage(62)])
        .split(root[1]);

    let previews = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Percentage(48),
            Constraint::Percentage(28),
            Constraint::Percentage(24),
        ])
        .split(body[1]);

    UiAreas {
        candidates: body[0],
        content: previews[0],
        git: previews[1],
        rsync: previews[2],
    }
}

fn draw_header(frame: &mut Frame, app: &App, area: Rect) {
    let header = Paragraph::new(Line::from(vec![
        Span::styled(
            "rdg",
            Style::default()
                .fg(Color::Cyan)
                .add_modifier(Modifier::BOLD),
        ),
        Span::raw(" | target: "),
        Span::styled(&app.target, Style::default().fg(Color::White)),
        Span::raw(format!(" | selected: {}", app.selected_count())),
        Span::raw(format!(" | entries: {}", app.entries.len())),
        Span::raw(" | root: "),
        Span::styled(
            app.repo_root.display().to_string(),
            Style::default().fg(Color::Gray),
        ),
    ]))
    .block(
        Block::default()
            .borders(Borders::ALL)
            .title("Remote deploy based on git"),
    );

    frame.render_widget(header, area);
}

fn draw_body(frame: &mut Frame, app: &App, area: Rect) {
    let body = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([Constraint::Percentage(38), Constraint::Percentage(62)])
        .split(area);

    let previews = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Percentage(48),
            Constraint::Percentage(28),
            Constraint::Percentage(24),
        ])
        .split(body[1]);

    draw_candidates(frame, app, body[0]);
    draw_content_diff_preview(frame, app, previews[0]);
    draw_git_preview(frame, app, previews[1]);
    draw_rsync_preview(frame, app, previews[2]);
}

fn draw_candidates(frame: &mut Frame, app: &App, area: Rect) {
    let visible = app.visible_indices();
    let items: Vec<ListItem> = visible
        .iter()
        .filter_map(|index| app.entries.get(*index))
        .map(tree_item)
        .collect();

    let list = List::new(items)
        .block(Block::default().borders(Borders::ALL).title("Repository"))
        .highlight_symbol("> ")
        .highlight_style(
            Style::default()
                .bg(Color::DarkGray)
                .add_modifier(Modifier::BOLD),
        );

    let mut list_state = ListState::default();

    if !visible.is_empty() {
        list_state.select(Some(app.cursor));
    }

    frame.render_stateful_widget(list, area, &mut list_state);
}

fn tree_item(entry: &TreeEntry) -> ListItem<'static> {
    let checkbox = if entry.selected { "[x]" } else { "[ ]" };
    let indent = "  ".repeat(entry.depth);
    let marker = match entry.kind {
        EntryKind::Directory if entry.expanded => "v",
        EntryKind::Directory => ">",
        EntryKind::File => " ",
    };
    let name_prefix = match entry.kind {
        EntryKind::Directory if entry.expanded => "📂 ",
        EntryKind::Directory => "📁 ",
        EntryKind::File => "",
    };
    let kind_suffix = if entry.is_dir() { "/" } else { "" };
    let git_kind = entry.git_kind.unwrap_or(ChangeKind::Unchanged);

    let line = Line::from(vec![
        Span::raw(format!("{checkbox} ")),
        Span::raw(indent),
        Span::styled(
            format!("{marker} {:<1} {:<8}", git_kind.short(), git_kind.label()),
            change_kind_style(git_kind),
        ),
        Span::raw(" "),
        Span::styled(
            format!("{name_prefix}{}{}", entry.name, kind_suffix),
            change_kind_style(git_kind),
        ),
    ]);

    ListItem::new(line)
}

fn draw_git_preview(frame: &mut Frame, app: &App, area: Rect) {
    let preview_title = app
        .current_entry()
        .map(|entry| format!("Git diff: {}", entry.path))
        .unwrap_or_else(|| String::from("Git diff"));

    let preview = Paragraph::new(styled_git_text(&app.git_preview))
        .block(preview_block(
            preview_title,
            app.active_pane == PreviewPane::Git,
            Color::Cyan,
        ))
        .wrap(Wrap { trim: false })
        .scroll((app.git_scroll, 0));

    frame.render_widget(preview, area);
}

fn draw_content_diff_preview(frame: &mut Frame, app: &App, area: Rect) {
    let preview_title = app
        .current_entry()
        .map(|entry| format!("Selected destination content diff: {}", entry.path))
        .unwrap_or_else(|| String::from("Selected destination content diff"));

    let preview = Paragraph::new(styled_content_diff_text(&app.content_diff_preview))
        .block(preview_block(
            preview_title,
            app.active_pane == PreviewPane::Content,
            Color::Green,
        ))
        .wrap(Wrap { trim: false })
        .scroll((app.content_diff_scroll, 0));

    frame.render_widget(preview, area);
}

fn draw_rsync_preview(frame: &mut Frame, app: &App, area: Rect) {
    let preview = Paragraph::new(styled_rsync_text(&app.rsync_preview))
        .block(preview_block(
            "Rsync destination status".to_string(),
            app.active_pane == PreviewPane::Rsync,
            Color::Magenta,
        ))
        .wrap(Wrap { trim: false })
        .scroll((app.rsync_scroll, 0));

    frame.render_widget(preview, area);
}

fn draw_footer(frame: &mut Frame, app: &App, area: Rect) {
    let focus = match app.active_pane {
        PreviewPane::Content => "content",
        PreviewPane::Git => "git",
        PreviewPane::Rsync => "rsync",
    };
    let footer = Paragraph::new(format!(
        "Click row: focus | click checkbox: select | click pane/Tab: focus ({focus}) | wheel/PgUp/PgDn: scroll | Space: select | d: diff | r: run | q: quit\n{}",
        app.message
    ))
    .block(Block::default().borders(Borders::ALL));

    frame.render_widget(footer, area);
}

fn preview_block(title: String, active: bool, color: Color) -> Block<'static> {
    let border_style = if active {
        Style::default().fg(color)
    } else {
        Style::default().fg(Color::DarkGray)
    };

    Block::default()
        .borders(Borders::ALL)
        .border_style(border_style)
        .title(Span::styled(
            title,
            Style::default().fg(color).add_modifier(Modifier::BOLD),
        ))
}

fn change_kind_style(kind: ChangeKind) -> Style {
    match kind {
        ChangeKind::Unchanged => Style::default().fg(Color::Gray),
        ChangeKind::Changed => Style::default().fg(Color::Yellow),
        ChangeKind::Untracked => Style::default().fg(Color::Green),
        ChangeKind::Deleted => Style::default().fg(Color::Red),
    }
}

fn styled_git_text(input: &str) -> Text<'static> {
    Text::from(input.lines().map(styled_git_line).collect::<Vec<_>>())
}

fn styled_content_diff_text(input: &str) -> Text<'static> {
    Text::from(
        input
            .lines()
            .map(styled_content_diff_line)
            .collect::<Vec<_>>(),
    )
}

fn styled_content_diff_line(line: &str) -> Line<'static> {
    let style = if line.starts_with("@@") {
        Style::default().fg(Color::Blue)
    } else if line.starts_with("--- ") || line.starts_with("+++ ") {
        Style::default()
            .fg(Color::Yellow)
            .add_modifier(Modifier::BOLD)
    } else if line.starts_with("+") {
        Style::default().fg(Color::Green)
    } else if line.starts_with("-") {
        Style::default().fg(Color::Red)
    } else {
        Style::default()
    };

    Line::from(Span::styled(line.to_string(), style))
}

fn styled_git_line(line: &str) -> Line<'static> {
    let style = if line.starts_with("diff --git") {
        Style::default()
            .fg(Color::Cyan)
            .add_modifier(Modifier::BOLD)
    } else if line.starts_with("@@") {
        Style::default().fg(Color::Blue)
    } else if line.starts_with("+") {
        Style::default().fg(Color::Green)
    } else if line.starts_with("-") {
        Style::default().fg(Color::Red)
    } else if line.starts_with("NEW FILE") || line.starts_with("NEW DIRECTORY") {
        Style::default()
            .fg(Color::Green)
            .add_modifier(Modifier::BOLD)
    } else if line.starts_with("NEW BINARY") {
        Style::default()
            .fg(Color::Yellow)
            .add_modifier(Modifier::BOLD)
    } else {
        Style::default()
    };

    Line::from(Span::styled(line.to_string(), style))
}

fn styled_rsync_text(input: &str) -> Text<'static> {
    Text::from(input.lines().map(styled_rsync_line).collect::<Vec<_>>())
}

fn styled_rsync_line(line: &str) -> Line<'static> {
    let style = if line == "DRY RUN" || line == "RUN" {
        Style::default()
            .fg(Color::Magenta)
            .add_modifier(Modifier::BOLD)
    } else if line == "CURRENT FILE RSYNC DIFF"
        || line == "TOTAL RSYNC DIFF"
        || line == "RSYNC STDERR"
        || line == "SELECTED FILES"
        || line == "Content diff:"
        || line == "Decoded:"
        || line == "Itemized rsync change:"
    {
        Style::default()
            .fg(Color::Cyan)
            .add_modifier(Modifier::BOLD)
    } else if line.starts_with("$ rsync") || line.starts_with("@@") {
        Style::default().fg(Color::Blue)
    } else if line.starts_with("+++") || line.starts_with("---") {
        Style::default()
            .fg(Color::Yellow)
            .add_modifier(Modifier::BOLD)
    } else if line.starts_with("+") {
        Style::default().fg(Color::Green)
    } else if line.starts_with("- ") || line.starts_with("-") {
        Style::default().fg(Color::Red)
    } else if line.starts_with("*deleting") || line.contains(" *deleting") {
        Style::default().fg(Color::Red).add_modifier(Modifier::BOLD)
    } else if line.starts_with(">f") || line.starts_with("cd") || line.starts_with("cL") {
        Style::default().fg(Color::Green)
    } else if line.starts_with(".d") || line.starts_with(".f") {
        Style::default().fg(Color::DarkGray)
    } else if line.starts_with("rsync error") || line.starts_with("Rsync failed") {
        Style::default().fg(Color::Red).add_modifier(Modifier::BOLD)
    } else {
        Style::default()
    };

    Line::from(Span::styled(line.to_string(), style))
}
