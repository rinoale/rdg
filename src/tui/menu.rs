use ratatui::{
    style::Modifier,
    text::{Line, Span},
};

use super::{
    keymap::Keymap,
    style::Role,
    theme::{Theme, ThemeKind},
};

pub fn footer(keymap: &Keymap, theme: &Theme) -> Line<'static> {
    let mut spans = Vec::new();
    for binding in keymap.bindings().iter().take(6) {
        spans.push(Span::styled(
            binding.label,
            theme.style(Role::FooterKey).add_modifier(Modifier::BOLD),
        ));
        spans.push(Span::raw(format!(" {}  ", binding.description)));
    }
    spans.push(Span::styled(
        ":q",
        theme.style(Role::FooterKey).add_modifier(Modifier::BOLD),
    ));
    spans.push(Span::raw(" quit"));
    Line::from(spans)
}

pub fn help_lines(keymap: &Keymap, theme_kind: ThemeKind) -> Vec<Line<'static>> {
    let mut lines = vec![
        Line::from("Common Interface"),
        Line::from(""),
        Line::from("Quit is command-mode only: :q, :quit, :exit, :q!"),
        Line::from("Esc cancels transient state; it never quits."),
        Line::from(""),
        Line::from("Keys"),
    ];

    for binding in keymap.bindings() {
        lines.push(Line::from(format!(
            "  {:<10} {}",
            binding.label, binding.description
        )));
    }

    lines.extend([
        Line::from(""),
        Line::from("rdg local keys"),
        Line::from("  j/k        move through files"),
        Line::from("  Space      select or unselect current row"),
        Line::from("  e          expand or collapse current folder"),
        Line::from("  a          select all or clear all"),
        Line::from("  s          show selected files only"),
        Line::from("  d          refresh destination diff"),
        Line::from("  r          run rsync after reviewing diff"),
        Line::from("  F5         refresh repository candidates"),
        Line::from("  PgUp/PgDn  scroll focused pane"),
        Line::from(""),
        Line::from("Commands"),
        Line::from("  :help              open help"),
        Line::from("  :theme neutral     neutral theme"),
        Line::from("  :theme safe        green safe theme"),
        Line::from("  :theme danger      red danger theme"),
        Line::from("  :refresh           reload repository candidates"),
        Line::from("  :diff              refresh destination diff"),
        Line::from("  :run, :deploy      run rsync"),
        Line::from("  :select            select or unselect current row"),
        Line::from("  :all               select all or clear all"),
        Line::from("  :selected          show selected files only"),
        Line::from("  :tree              show repository tree"),
        Line::from("  :focus <pane>      repository, content, git, rsync"),
        Line::from("  :expand            expand current folder"),
        Line::from("  :collapse          collapse current folder"),
        Line::from("  :q, :quit, :exit   quit"),
        Line::from("  :q!                force quit"),
        Line::from(""),
        Line::from(format!("Current theme: {:?}", theme_kind)),
    ]);

    lines
}
