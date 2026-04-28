//! Compact `KeyEvent` constructors for tests. Aim: keep test bodies
//! reading like the keystrokes a user would type.

use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

pub fn enter() -> KeyEvent {
    KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE)
}

pub fn esc() -> KeyEvent {
    KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE)
}

pub fn up() -> KeyEvent {
    KeyEvent::new(KeyCode::Up, KeyModifiers::NONE)
}

pub fn down() -> KeyEvent {
    KeyEvent::new(KeyCode::Down, KeyModifiers::NONE)
}

pub fn left() -> KeyEvent {
    KeyEvent::new(KeyCode::Left, KeyModifiers::NONE)
}

pub fn right() -> KeyEvent {
    KeyEvent::new(KeyCode::Right, KeyModifiers::NONE)
}

pub fn tab() -> KeyEvent {
    KeyEvent::new(KeyCode::Tab, KeyModifiers::NONE)
}

pub fn back_tab() -> KeyEvent {
    KeyEvent::new(KeyCode::BackTab, KeyModifiers::NONE)
}

pub fn space() -> KeyEvent {
    KeyEvent::new(KeyCode::Char(' '), KeyModifiers::NONE)
}

pub fn backspace() -> KeyEvent {
    KeyEvent::new(KeyCode::Backspace, KeyModifiers::NONE)
}

pub fn char(c: char) -> KeyEvent {
    KeyEvent::new(KeyCode::Char(c), KeyModifiers::NONE)
}

pub fn ctrl(c: char) -> KeyEvent {
    KeyEvent::new(KeyCode::Char(c), KeyModifiers::CONTROL)
}

/// Type a string by emitting one KeyEvent per character. Whitespace and
/// other ordinary characters go through unmodified; non-printables aren't
/// supported.
pub fn type_str(s: &str) -> Vec<KeyEvent> {
    s.chars().map(char).collect()
}
