use ratatui::{
    Frame,
    layout::{Constraint, Direction, Layout, Rect},
    style::Style,
    text::{Line, Span, Text},
    widgets::{Block, Borders, Clear, List, ListItem, ListState, Paragraph, Wrap},
};

use crate::{
    app::{App, PreviewPane},
    core::{
        git::ChangeKind,
        tree::{EntryKind, TreeEntry},
    },
    tui::{menu, style::Role, theme::Theme},
};

#[derive(Clone, Copy, Debug)]
pub struct UiAreas {
    pub candidates: Rect,
    pub content: Rect,
    pub git: Rect,
    pub rsync: Rect,
}

pub fn draw(frame: &mut Frame, app: &App) {
    let theme = app.theme_kind.theme();
    frame.render_widget(Clear, frame.area());
    frame.render_widget(
        Block::default().style(theme.style(Role::AppBackground)),
        frame.area(),
    );

    let root = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(3),
            Constraint::Min(1),
            Constraint::Length(4),
        ])
        .split(frame.area());

    draw_header(frame, app, root[0], &theme);
    draw_body(frame, app, root[1], &theme);
    draw_footer(frame, app, root[2], &theme);

    if app.show_help {
        draw_help(frame, app, centered_rect(frame.area(), 72, 70), &theme);
    }
}

pub fn body_areas(area: Rect) -> UiAreas {
    let root = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(3),
            Constraint::Min(1),
            Constraint::Length(4),
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

fn draw_header(frame: &mut Frame, app: &App, area: Rect, theme: &Theme) {
    let mode = if app.command_mode {
        "COMMAND"
    } else {
        "NORMAL"
    };
    let header = Paragraph::new(Line::from(vec![
        Span::styled(" rdg ", theme.style(Role::HeaderBrand)),
        Span::raw(" "),
        Span::styled(theme.name, theme.style(Role::HeaderMode)),
        Span::raw(" "),
        Span::styled(mode, theme.style(Role::HeaderMode)),
        Span::raw(" | target: "),
        Span::styled(&app.target, theme.style(Role::Text)),
        Span::raw(" | selected: "),
        Span::styled(
            app.selected_count().to_string(),
            theme.selector("rdg.header.selected_count"),
        ),
        Span::raw(format!(" | entries: {}", app.entries.len())),
    ]))
    .block(
        Block::default()
            .borders(Borders::ALL)
            .border_style(theme.style(Role::Header))
            .style(theme.style(Role::Header)),
    );

    frame.render_widget(header, area);
}

fn draw_body(frame: &mut Frame, app: &App, area: Rect, theme: &Theme) {
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

    draw_candidates(frame, app, body[0], theme);
    draw_content_diff_preview(frame, app, previews[0], theme);
    draw_git_preview(frame, app, previews[1], theme);
    draw_rsync_preview(frame, app, previews[2], theme);
}

fn draw_candidates(frame: &mut Frame, app: &App, area: Rect, theme: &Theme) {
    let visible = app.visible_indices();
    let items: Vec<ListItem> = visible
        .iter()
        .filter_map(|index| app.entries.get(*index))
        .map(|entry| tree_item(entry, app.selected_only, theme))
        .collect();

    let title = if app.selected_only {
        format!("Repository: selected files ({})", visible.len())
    } else {
        String::from("Repository")
    };

    let list = List::new(items)
        .block(pane_block(
            title,
            app.active_pane == PreviewPane::Repository,
            "rdg.repository.border",
            theme,
        ))
        .highlight_symbol("> ")
        .highlight_style(theme.style(Role::ListItemSelected))
        .style(theme.style(Role::ListItem));

    let viewport_rows = usize::from(area.height.saturating_sub(2));
    let mut list_state =
        ListState::default().with_offset(app.repository_view_offset(viewport_rows));

    if !visible.is_empty() {
        list_state.select(Some(app.cursor));
    }

    frame.render_stateful_widget(list, area, &mut list_state);
}

fn tree_item(entry: &TreeEntry, selected_only: bool, theme: &Theme) -> ListItem<'static> {
    let checkbox = if entry.selected { "[x]" } else { "[ ]" };
    let indent = if selected_only {
        String::new()
    } else {
        "  ".repeat(entry.depth)
    };
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
        Span::styled(format!("{checkbox} "), theme.selector("rdg.file.checkbox")),
        Span::raw(indent),
        Span::styled(
            format!("{marker} {:<1} {:<8}", git_kind.short(), git_kind.label()),
            change_kind_style(git_kind, theme),
        ),
        Span::raw(" "),
        Span::styled(
            format!(
                "{name_prefix}{}{}",
                if selected_only {
                    entry.path.as_str()
                } else {
                    entry.name.as_str()
                },
                kind_suffix
            ),
            change_kind_style(git_kind, theme),
        ),
    ]);

    ListItem::new(line)
}

fn draw_git_preview(frame: &mut Frame, app: &App, area: Rect, theme: &Theme) {
    let preview_title = app
        .current_entry()
        .map(|entry| format!("Git diff: {}", entry.path))
        .unwrap_or_else(|| String::from("Git diff"));

    let preview = Paragraph::new(styled_git_text(&app.git_preview, theme))
        .block(preview_block(
            preview_title,
            app.active_pane == PreviewPane::Git,
            "rdg.git.border",
            theme,
        ))
        .wrap(Wrap { trim: false })
        .scroll((app.git_scroll, 0))
        .style(theme.style(Role::Text));

    frame.render_widget(preview, area);
}

fn draw_content_diff_preview(frame: &mut Frame, app: &App, area: Rect, theme: &Theme) {
    let preview_title = app
        .current_entry()
        .map(|entry| format!("Selected destination content diff: {}", entry.path))
        .unwrap_or_else(|| String::from("Selected destination content diff"));

    let preview = Paragraph::new(styled_content_diff_text(&app.content_diff_preview, theme))
        .block(preview_block(
            preview_title,
            app.active_pane == PreviewPane::Content,
            "rdg.content.border",
            theme,
        ))
        .wrap(Wrap { trim: false })
        .scroll((app.content_diff_scroll, 0))
        .style(theme.style(Role::Text));

    frame.render_widget(preview, area);
}

fn draw_rsync_preview(frame: &mut Frame, app: &App, area: Rect, theme: &Theme) {
    let preview = Paragraph::new(styled_rsync_text(&app.rsync_preview, theme))
        .block(preview_block(
            "Rsync destination status".to_string(),
            app.active_pane == PreviewPane::Rsync,
            "rdg.rsync.border",
            theme,
        ))
        .wrap(Wrap { trim: false })
        .scroll((app.rsync_scroll, 0))
        .style(theme.style(Role::Text));

    frame.render_widget(preview, area);
}

fn draw_footer(frame: &mut Frame, app: &App, area: Rect, theme: &Theme) {
    let title = if app.command_mode {
        "Command [focus]"
    } else {
        "Command"
    };
    let status_style = if app.message.contains("failed") || app.message.contains("blocked") {
        theme.style(Role::StatusDanger)
    } else {
        theme.style(Role::Status)
    };
    let prompt = if app.command_mode {
        Line::from(vec![
            Span::styled("cmd ", theme.style(Role::PromptPrefix)),
            Span::styled(app.command_input.clone(), theme.style(Role::PromptInput)),
        ])
    } else {
        Line::from(vec![
            Span::styled("status ", theme.style(Role::TextMuted)),
            Span::styled(app.message.clone(), status_style),
            Span::raw("  "),
            Span::styled(
                format!("focus: {}", app.active_pane.label()),
                theme.style(Role::Badge),
            ),
        ])
    };
    let footer = Paragraph::new(vec![prompt, menu::footer(&app.keymap, theme)])
        .block(
            Block::default()
                .title(title)
                .borders(Borders::ALL)
                .border_style(if app.command_mode {
                    theme.style(Role::CommandFocused)
                } else {
                    theme.style(Role::Command)
                })
                .style(theme.style(Role::Command)),
        )
        .style(theme.style(Role::Footer));

    frame.render_widget(footer, area);
}

fn draw_help(frame: &mut Frame, app: &App, area: Rect, theme: &Theme) {
    frame.render_widget(Clear, area);
    let block = Block::default()
        .title("Help")
        .borders(Borders::ALL)
        .border_style(theme.style(Role::HelpTitle))
        .style(theme.style(Role::HelpOverlay));
    frame.render_widget(
        Paragraph::new(menu::help_lines(&app.keymap, app.theme_kind))
            .block(block)
            .style(theme.style(Role::HelpOverlay))
            .wrap(Wrap { trim: false }),
        area,
    );
}

fn pane_block(
    title: String,
    active: bool,
    border_selector: &'static str,
    theme: &Theme,
) -> Block<'static> {
    let border_style = if active {
        theme.selector(border_selector)
    } else {
        theme.style(Role::TextMuted)
    };

    Block::default()
        .borders(Borders::ALL)
        .border_style(border_style)
        .style(theme.style(Role::Panel))
        .title(Span::styled(title, theme.style(Role::PanelTitle)))
}

fn preview_block(
    title: String,
    active: bool,
    border_selector: &'static str,
    theme: &Theme,
) -> Block<'static> {
    pane_block(title, active, border_selector, theme)
}

fn centered_rect(area: Rect, percent_x: u16, percent_y: u16) -> Rect {
    let vertical = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Percentage((100 - percent_y) / 2),
            Constraint::Percentage(percent_y),
            Constraint::Percentage((100 - percent_y) / 2),
        ])
        .split(area);

    let horizontal = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([
            Constraint::Percentage((100 - percent_x) / 2),
            Constraint::Percentage(percent_x),
            Constraint::Percentage((100 - percent_x) / 2),
        ])
        .split(vertical[1]);

    horizontal[1]
}

fn change_kind_style(kind: ChangeKind, theme: &Theme) -> Style {
    match kind {
        ChangeKind::Unchanged => theme.selector("git.clean"),
        ChangeKind::Changed => theme.selector("git.changed"),
        ChangeKind::Untracked => theme.selector("git.untracked"),
        ChangeKind::Deleted => theme.selector("git.deleted"),
    }
}

fn styled_git_text(input: &str, theme: &Theme) -> Text<'static> {
    Text::from(
        input
            .lines()
            .map(|line| styled_git_line(line, theme))
            .collect::<Vec<_>>(),
    )
}

fn styled_content_diff_text(input: &str, theme: &Theme) -> Text<'static> {
    Text::from(
        input
            .lines()
            .map(|line| styled_content_diff_line(line, theme))
            .collect::<Vec<_>>(),
    )
}

fn styled_content_diff_line(line: &str, theme: &Theme) -> Line<'static> {
    let style = if line.starts_with("@@") {
        theme.selector("diff.hunk")
    } else if line.starts_with("--- ") || line.starts_with("+++ ") {
        theme.selector("diff.header")
    } else if line.starts_with("+") {
        theme.selector("diff.add")
    } else if line.starts_with("-") {
        theme.selector("diff.remove")
    } else {
        theme.style(Role::Text)
    };

    Line::from(Span::styled(line.to_string(), style))
}

fn styled_git_line(line: &str, theme: &Theme) -> Line<'static> {
    let style = if line.starts_with("diff --git") {
        theme.selector("git.header")
    } else if line.starts_with("@@") {
        theme.selector("diff.hunk")
    } else if line.starts_with("+") {
        theme.selector("diff.add")
    } else if line.starts_with("-") {
        theme.selector("diff.remove")
    } else if line.starts_with("NEW FILE") || line.starts_with("NEW DIRECTORY") {
        theme.selector("diff.emphasis")
    } else if line.starts_with("NEW BINARY") {
        theme.selector("diff.binary")
    } else {
        theme.style(Role::Text)
    };

    Line::from(Span::styled(line.to_string(), style))
}

fn styled_rsync_text(input: &str, theme: &Theme) -> Text<'static> {
    Text::from(
        input
            .lines()
            .map(|line| styled_rsync_line(line, theme))
            .collect::<Vec<_>>(),
    )
}

fn styled_rsync_line(line: &str, theme: &Theme) -> Line<'static> {
    let style = if line == "DRY RUN" || line == "RUN" {
        theme.selector("rsync.header")
    } else if line == "CURRENT FILE RSYNC DIFF"
        || line == "TOTAL RSYNC DIFF"
        || line == "RSYNC STDERR"
        || line == "SELECTED FILES"
        || line == "Content diff:"
        || line == "Decoded:"
        || line == "Itemized rsync change:"
    {
        theme.selector("rsync.header")
    } else if line.starts_with("$ rsync") || line.starts_with("@@") {
        theme.selector("rsync.command")
    } else if line.starts_with("+++") || line.starts_with("---") {
        theme.selector("diff.header")
    } else if line.starts_with("+") {
        theme.selector("rsync.add")
    } else if line.starts_with("- ") || line.starts_with("-") {
        theme.selector("rsync.remove")
    } else if line.starts_with("*deleting") || line.contains(" *deleting") {
        theme.selector("rsync.delete")
    } else if line.starts_with(">f") || line.starts_with("cd") || line.starts_with("cL") {
        theme.selector("rsync.add")
    } else if line.starts_with(".d") || line.starts_with(".f") {
        theme.selector("rsync.unchanged")
    } else if line.starts_with("rsync error") || line.starts_with("Rsync failed") {
        theme.selector("rsync.error")
    } else {
        theme.style(Role::Text)
    };

    Line::from(Span::styled(line.to_string(), style))
}
