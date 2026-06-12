#![allow(dead_code)]

use ratatui::style::{Color, Modifier, Style};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ColorToken {
    Surface0,
    Surface1,
    Surface2,
    Border,
    Text,
    Muted,
    Accent,
    AccentLow,
    Success,
    Warning,
    Danger,
    Selection,
}

#[derive(Debug, Clone, Copy)]
pub struct Palette {
    pub surface0: Color,
    pub surface1: Color,
    pub surface2: Color,
    pub border: Color,
    pub text: Color,
    pub muted: Color,
    pub accent: Color,
    pub accent_low: Color,
    pub success: Color,
    pub warning: Color,
    pub danger: Color,
    pub selection: Color,
}

impl Palette {
    pub fn color(self, token: ColorToken) -> Color {
        match token {
            ColorToken::Surface0 => self.surface0,
            ColorToken::Surface1 => self.surface1,
            ColorToken::Surface2 => self.surface2,
            ColorToken::Border => self.border,
            ColorToken::Text => self.text,
            ColorToken::Muted => self.muted,
            ColorToken::Accent => self.accent,
            ColorToken::AccentLow => self.accent_low,
            ColorToken::Success => self.success,
            ColorToken::Warning => self.warning,
            ColorToken::Danger => self.danger,
            ColorToken::Selection => self.selection,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Role {
    AppBackground,
    Header,
    HeaderBrand,
    HeaderMode,
    Panel,
    PanelFocused,
    PanelTitle,
    Text,
    TextMuted,
    ListItem,
    ListItemSelected,
    Command,
    CommandFocused,
    PromptPrefix,
    PromptInput,
    Footer,
    FooterKey,
    HelpOverlay,
    HelpTitle,
    Status,
    StatusSuccess,
    StatusWarning,
    StatusDanger,
    Button,
    ButtonPrimary,
    ButtonDanger,
    ButtonFocused,
    Input,
    InputFocused,
    Badge,
    BadgeSuccess,
    BadgeWarning,
    BadgeDanger,
    TableHeader,
    TableCell,
    TableCellSelected,
}

impl Role {
    pub fn selector(self) -> &'static str {
        match self {
            Role::AppBackground => "app.background",
            Role::Header => "header",
            Role::HeaderBrand => "header.brand",
            Role::HeaderMode => "header.mode",
            Role::Panel => "panel",
            Role::PanelFocused => "panel.focused",
            Role::PanelTitle => "panel.title",
            Role::Text => "text",
            Role::TextMuted => "text.muted",
            Role::ListItem => "list.item",
            Role::ListItemSelected => "list.item.selected",
            Role::Command => "command",
            Role::CommandFocused => "command.focused",
            Role::PromptPrefix => "prompt.prefix",
            Role::PromptInput => "prompt.input",
            Role::Footer => "footer",
            Role::FooterKey => "footer.key",
            Role::HelpOverlay => "help.overlay",
            Role::HelpTitle => "help.title",
            Role::Status => "status",
            Role::StatusSuccess => "status.success",
            Role::StatusWarning => "status.warning",
            Role::StatusDanger => "status.danger",
            Role::Button => "button",
            Role::ButtonPrimary => "button.primary",
            Role::ButtonDanger => "button.danger",
            Role::ButtonFocused => "button.focused",
            Role::Input => "input",
            Role::InputFocused => "input.focused",
            Role::Badge => "badge",
            Role::BadgeSuccess => "badge.success",
            Role::BadgeWarning => "badge.warning",
            Role::BadgeDanger => "badge.danger",
            Role::TableHeader => "table.header",
            Role::TableCell => "table.cell",
            Role::TableCellSelected => "table.cell.selected",
        }
    }
}

#[derive(Debug, Clone, Copy)]
pub struct Rule {
    pub selector: &'static str,
    pub style: Style,
}

#[derive(Debug, Clone)]
pub struct Design {
    pub palette: Palette,
    rules: Vec<Rule>,
}

impl Design {
    pub fn new(palette: Palette) -> Self {
        Self {
            palette,
            rules: Vec::new(),
        }
    }

    pub fn role(self, role: Role, style: StyleBuilder) -> Self {
        self.selector(role.selector(), style)
    }

    pub fn selector(mut self, selector: &'static str, style: StyleBuilder) -> Self {
        self.rules.retain(|rule| rule.selector != selector);
        self.rules.push(Rule {
            selector,
            style: style.build(),
        });
        self
    }

    pub fn style(&self, selector: &str) -> Style {
        self.rules
            .iter()
            .rev()
            .find(|rule| rule.selector == selector)
            .map(|rule| rule.style)
            .unwrap_or_else(Style::default)
    }

    pub fn role_style(&self, role: Role) -> Style {
        self.style(role.selector())
    }

    pub fn color(&self, token: ColorToken) -> Color {
        self.palette.color(token)
    }
}

#[derive(Debug, Clone, Copy)]
pub struct StyleBuilder {
    style: Style,
}

pub fn style() -> StyleBuilder {
    StyleBuilder {
        style: Style::default(),
    }
}

impl StyleBuilder {
    pub fn fg(mut self, color: Color) -> Self {
        self.style = self.style.fg(color);
        self
    }

    pub fn bg(mut self, color: Color) -> Self {
        self.style = self.style.bg(color);
        self
    }

    pub fn bold(mut self) -> Self {
        self.style = self.style.add_modifier(Modifier::BOLD);
        self
    }

    pub fn dim(mut self) -> Self {
        self.style = self.style.add_modifier(Modifier::DIM);
        self
    }

    pub fn italic(mut self) -> Self {
        self.style = self.style.add_modifier(Modifier::ITALIC);
        self
    }

    pub fn underlined(mut self) -> Self {
        self.style = self.style.add_modifier(Modifier::UNDERLINED);
        self
    }

    pub fn reversed(mut self) -> Self {
        self.style = self.style.add_modifier(Modifier::REVERSED);
        self
    }

    pub fn build(self) -> Style {
        self.style
    }
}
