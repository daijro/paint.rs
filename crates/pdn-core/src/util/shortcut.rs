//! Port of: js/src/engine/util/{ShortcutKey,KeyButton,MouseButton}.js
//! Reference: paintdotjs @ b4e6fc5
//! All five mouse buttons, as Paint.NET has them.

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MouseButton {
    Left = 0,
    Middle = 1,
    Right = 2,
    X1 = 3,
    X2 = 4,
}

impl MouseButton {
    pub const ALL: [Self; 5] = [Self::Left, Self::Middle, Self::Right, Self::X1, Self::X2];
}

/// KeyButton constants.
pub mod key_button {
    pub const LEFT: &str = "ArrowLeft";
    pub const UP: &str = "ArrowUp";
    pub const RIGHT: &str = "ArrowRight";
    pub const DOWN: &str = "ArrowDown";
}

#[derive(Clone, Debug, PartialEq, Eq, Default)]
pub struct ShortcutKey {
    pub key: Option<String>,
    pub shift: bool,
    pub ctrl: bool,
    pub alt: bool,
    pub meta: bool,
}

impl ShortcutKey {
    pub fn new(key: Option<String>, shift: bool, ctrl: bool, alt: bool, meta: bool) -> Self {
        Self {
            key,
            shift,
            ctrl,
            alt,
            meta,
        }
    }

    /// JS `isEvent`, with the event's fields passed in.
    pub fn matches(&self, key: &str, shift: bool, ctrl: bool, alt: bool, meta: bool) -> bool {
        let normalized = Self::normalize_key(key);
        // Ctrl++ is accepted without an explicit Shift (implicit-Shift exception for "+").
        let shift_matches = if self.key.as_deref() == Some("+") && normalized == "+" && !self.shift
        {
            true
        } else {
            self.shift == shift
        };
        self.key.is_some()
            && shift_matches
            && self.ctrl == ctrl
            && self.alt == alt
            && self.meta == meta
            && self.key.as_deref() == Some(normalized.as_str())
    }

    /// JS `toString`: `null` (None) when empty. Order is Ctrl+Alt+Shift+Meta+Key.
    pub fn to_string(&self) -> Option<String> {
        let key = self.key.as_ref()?;
        let mut combo = String::new();
        if self.ctrl {
            combo += "Ctrl+";
        }
        if self.alt {
            combo += "Alt+";
        }
        if self.shift {
            combo += "Shift+";
        }
        if self.meta {
            combo += "Meta+";
        }
        combo += &Self::display_key(key);
        Some(combo)
    }

    pub fn is_empty(&self) -> bool {
        self.key.is_none()
    }

    pub fn from_combo(combo: &str) -> ShortcutKey {
        let mut normalized = combo.trim();
        if normalized.is_empty() {
            return ShortcutKey::default();
        }
        let plus_key = normalized.ends_with('+');
        if plus_key {
            normalized = &normalized[..normalized.len() - 1];
        }
        let stripped: String = normalized.chars().filter(|c| !c.is_whitespace()).collect();
        let mut target_key = None;
        let (mut shift, mut ctrl, mut alt, mut meta) = (false, false, false, false);
        for key in stripped.split('+').filter(|s| !s.is_empty()) {
            match key.to_lowercase().as_str() {
                "shift" => shift = true,
                "ctrl" | "control" => ctrl = true,
                "alt" => alt = true,
                "meta" | "cmd" | "command" => meta = true,
                _ => target_key = Some(Self::normalize_key(key)),
            }
        }
        if plus_key {
            target_key = Some("+".to_string());
        }
        ShortcutKey::new(target_key, shift, ctrl, alt, meta)
    }

    /// JS `fromEvent`: lone modifier presses give the empty shortcut.
    pub fn from_event(key: &str, shift: bool, ctrl: bool, alt: bool, meta: bool) -> ShortcutKey {
        let key = Self::normalize_key(key);
        if ["Control", "Shift", "Alt", "Meta"].contains(&key.as_str()) {
            return ShortcutKey::default();
        }
        ShortcutKey::new(Some(key), shift, ctrl, alt, meta)
    }

    pub fn normalize_key(key: &str) -> String {
        let normalized = match key {
            " " | "Spacebar" => "Space",
            "Esc" => "Escape",
            "Del" => "Delete",
            "Left" => "ArrowLeft",
            "Right" => "ArrowRight",
            "Up" => "ArrowUp",
            "Down" => "ArrowDown",
            k => k,
        };
        // JS `length === 1` counts UTF-16 units.
        if normalized.encode_utf16().count() == 1 {
            normalized.to_uppercase()
        } else {
            normalized.to_string()
        }
    }

    pub fn display_key(key: &str) -> String {
        match key {
            "ArrowLeft" => "Left",
            "ArrowRight" => "Right",
            "ArrowUp" => "Up",
            "ArrowDown" => "Down",
            "Escape" => "Esc",
            "Delete" => "Del",
            " " => "Space",
            k => k,
        }
        .to_string()
    }
}
