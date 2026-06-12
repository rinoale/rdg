use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Intent {
    EnterCommandMode,
    Cancel,
    Help,
    NextPane,
    PreviousPane,
    Activate,
    MoveUp,
    MoveDown,
    MoveLeft,
    MoveRight,
    Search,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Key {
    Char(char),
    Esc,
    Enter,
    Tab,
    BackTab,
    Up,
    Down,
    Left,
    Right,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct KeyPattern {
    pub key: Key,
    pub modifiers: KeyModifiers,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct KeyBinding {
    pub pattern: KeyPattern,
    pub intent: Intent,
    pub label: &'static str,
    pub description: &'static str,
}

#[derive(Debug, Clone)]
pub struct Keymap {
    bindings: Vec<KeyBinding>,
}

impl Default for Keymap {
    fn default() -> Self {
        Self::common()
    }
}

impl Keymap {
    pub fn common() -> Self {
        let none = KeyModifiers::NONE;
        Self {
            bindings: vec![
                binding(
                    Key::Char(':'),
                    none,
                    Intent::EnterCommandMode,
                    ":",
                    "command mode",
                ),
                binding(Key::Esc, none, Intent::Cancel, "Esc", "cancel transient UI"),
                binding(Key::Char('?'), none, Intent::Help, "?", "help"),
                binding(Key::Tab, none, Intent::NextPane, "Tab", "next pane"),
                binding(
                    Key::BackTab,
                    none,
                    Intent::PreviousPane,
                    "Shift-Tab",
                    "previous pane",
                ),
                binding(Key::Enter, none, Intent::Activate, "Enter", "activate"),
                binding(Key::Up, none, Intent::MoveUp, "Up", "move up"),
                binding(Key::Down, none, Intent::MoveDown, "Down", "move down"),
                binding(Key::Left, none, Intent::MoveLeft, "Left", "move left"),
                binding(Key::Right, none, Intent::MoveRight, "Right", "move right"),
                binding(Key::Char('/'), none, Intent::Search, "/", "search"),
            ],
        }
    }

    #[allow(dead_code)]
    pub fn with_binding(mut self, binding: KeyBinding) -> Self {
        self.bindings
            .retain(|existing| existing.pattern != binding.pattern);
        self.bindings.push(binding);
        self
    }

    #[allow(dead_code)]
    pub fn without_key(mut self, pattern: KeyPattern) -> Self {
        self.bindings.retain(|existing| existing.pattern != pattern);
        self
    }

    pub fn intent_for(&self, key: KeyEvent) -> Option<Intent> {
        self.bindings
            .iter()
            .find(|binding| binding.pattern.matches(key))
            .map(|binding| binding.intent)
    }

    pub fn bindings(&self) -> &[KeyBinding] {
        &self.bindings
    }
}

pub fn binding(
    key: Key,
    modifiers: KeyModifiers,
    intent: Intent,
    label: &'static str,
    description: &'static str,
) -> KeyBinding {
    KeyBinding {
        pattern: KeyPattern { key, modifiers },
        intent,
        label,
        description,
    }
}

impl KeyPattern {
    pub fn matches(self, event: KeyEvent) -> bool {
        key_matches(self.key, event.code) && normalized_modifiers(event.modifiers) == self.modifiers
    }
}

pub fn text_input_modifiers(mut modifiers: KeyModifiers) -> bool {
    modifiers.remove(KeyModifiers::SHIFT);
    modifiers.is_empty()
}

fn key_matches(expected: Key, actual: KeyCode) -> bool {
    match (expected, actual) {
        (Key::Char(expected), KeyCode::Char(actual)) => expected == actual,
        (Key::Esc, KeyCode::Esc) => true,
        (Key::Enter, KeyCode::Enter) => true,
        (Key::Tab, KeyCode::Tab) => true,
        (Key::BackTab, KeyCode::BackTab) => true,
        (Key::Up, KeyCode::Up) => true,
        (Key::Down, KeyCode::Down) => true,
        (Key::Left, KeyCode::Left) => true,
        (Key::Right, KeyCode::Right) => true,
        _ => false,
    }
}

fn normalized_modifiers(mut modifiers: KeyModifiers) -> KeyModifiers {
    modifiers.remove(KeyModifiers::SHIFT);
    modifiers
}

#[cfg(test)]
mod tests {
    use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

    use super::{Intent, Keymap};

    #[test]
    fn quit_is_not_bound_to_a_key() {
        let keymap = Keymap::common();
        assert_eq!(
            keymap.intent_for(KeyEvent::new(KeyCode::Char('q'), KeyModifiers::NONE)),
            None
        );
        assert_eq!(
            keymap.intent_for(KeyEvent::new(KeyCode::Char('c'), KeyModifiers::CONTROL)),
            None
        );
    }

    #[test]
    fn colon_enters_command_mode() {
        let keymap = Keymap::common();
        assert_eq!(
            keymap.intent_for(KeyEvent::new(KeyCode::Char(':'), KeyModifiers::NONE)),
            Some(Intent::EnterCommandMode)
        );
    }

    #[test]
    fn escape_cancels_without_quitting() {
        let keymap = Keymap::common();
        assert_eq!(
            keymap.intent_for(KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE)),
            Some(Intent::Cancel)
        );
    }

    #[test]
    fn question_mark_opens_help() {
        let keymap = Keymap::common();
        assert_eq!(
            keymap.intent_for(KeyEvent::new(KeyCode::Char('?'), KeyModifiers::NONE)),
            Some(Intent::Help)
        );
    }
}
