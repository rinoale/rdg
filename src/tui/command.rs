use crate::app::PreviewPane;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Command {
    Quit { force: bool },
    Help,
    Theme(ThemeCommand),
    Refresh,
    Diff,
    Deploy,
    Select,
    SelectAll,
    SelectedOnly,
    Tree,
    Focus(PreviewPane),
    Expand,
    Collapse,
    Unknown(String),
    Empty,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ThemeCommand {
    Neutral,
    Safe,
    Danger,
}

pub fn parse(input: &str) -> Command {
    let trimmed = input.trim();
    if trimmed.is_empty() || trimmed == ":" {
        return Command::Empty;
    }

    if trimmed == "\\q" {
        return Command::Quit { force: false };
    }

    let Some(command) = trimmed.strip_prefix(':') else {
        return Command::Unknown(trimmed.to_string());
    };

    let mut parts = command.split_whitespace();
    let Some(name) = parts.next() else {
        return Command::Empty;
    };

    match name {
        "q" | "quit" | "exit" => Command::Quit { force: false },
        "q!" | "quit!" | "exit!" => Command::Quit { force: true },
        "help" | "?" => Command::Help,
        "theme" => match parts.next() {
            Some("neutral") | None => Command::Theme(ThemeCommand::Neutral),
            Some("safe") | Some("green") => Command::Theme(ThemeCommand::Safe),
            Some("danger") | Some("red") | Some("unsafe") => Command::Theme(ThemeCommand::Danger),
            Some(value) => Command::Unknown(format!("unknown theme `{value}`")),
        },
        "refresh" | "reload" => Command::Refresh,
        "diff" | "dry-run" => Command::Diff,
        "run" | "deploy" => Command::Deploy,
        "select" | "toggle" => Command::Select,
        "all" | "select-all" => Command::SelectAll,
        "selected" | "selected-only" => Command::SelectedOnly,
        "tree" | "repository" => Command::Tree,
        "expand" => Command::Expand,
        "collapse" => Command::Collapse,
        "focus" => match parts.next() {
            Some("repo" | "repository" | "files") => Command::Focus(PreviewPane::Repository),
            Some("content" | "destination") => Command::Focus(PreviewPane::Content),
            Some("git") => Command::Focus(PreviewPane::Git),
            Some("rsync" | "status") => Command::Focus(PreviewPane::Rsync),
            _ => Command::Unknown("focus".to_string()),
        },
        _ => Command::Unknown(name.to_string()),
    }
}

#[cfg(test)]
mod tests {
    use crate::app::PreviewPane;

    use super::{Command, ThemeCommand, parse};

    #[test]
    fn parses_quit_aliases() {
        assert_eq!(parse(":q"), Command::Quit { force: false });
        assert_eq!(parse(":quit!"), Command::Quit { force: true });
        assert_eq!(parse("\\q"), Command::Quit { force: false });
    }

    #[test]
    fn parses_theme_aliases() {
        assert_eq!(parse(":theme safe"), Command::Theme(ThemeCommand::Safe));
        assert_eq!(parse(":theme unsafe"), Command::Theme(ThemeCommand::Danger));
    }

    #[test]
    fn parses_deploy_commands() {
        assert_eq!(parse(":diff"), Command::Diff);
        assert_eq!(parse(":run"), Command::Deploy);
        assert_eq!(parse(":deploy"), Command::Deploy);
    }

    #[test]
    fn parses_view_commands() {
        assert_eq!(parse(":selected"), Command::SelectedOnly);
        assert_eq!(parse(":tree"), Command::Tree);
        assert_eq!(parse(":focus rsync"), Command::Focus(PreviewPane::Rsync));
    }
}
