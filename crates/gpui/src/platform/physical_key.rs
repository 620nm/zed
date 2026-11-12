use std::{fmt, str::FromStr};

macro_rules! physical_keys {
    ($($variant:ident,)*) => {
        /// The position of a key on the keyboard, independent of the active layout.
        ///
        /// Variants are named after, and convert to and from, the W3C UI Events
        /// `KeyboardEvent.code` values: [`PhysicalKey::KeyW`] is the key that types
        /// "w" on a US QWERTY layout and "z" on a French AZERTY one.
        ///
        /// Key events carry it on Linux (Wayland and X11) and on the web. macOS and
        /// Windows do not report it yet, so there the `physical_key` of key events
        /// is always `None`.
        ///
        /// Keys that act as modifiers or locks in the active layout (Shift, Control,
        /// Alt, AltGr, Meta, Caps Lock, and Num Lock on Linux) never produce key down
        /// or key up events, so their variants are not reported in key events.
        #[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
        #[non_exhaustive]
        #[expect(missing_docs)]
        pub enum PhysicalKey {
            $($variant,)*
        }

        impl PhysicalKey {
            /// The W3C `KeyboardEvent.code` name of this key, e.g. `"KeyW"`.
            pub fn code(self) -> &'static str {
                match self {
                    $(Self::$variant => stringify!($variant),)*
                }
            }

            /// The key with the given W3C `KeyboardEvent.code` name, if it is known.
            pub fn from_code(code: &str) -> Option<Self> {
                match code {
                    $(stringify!($variant) => Some(Self::$variant),)*
                    _ => None,
                }
            }

            #[cfg(test)]
            const ALL: &[Self] = &[$(Self::$variant,)*];
        }
    };
}

physical_keys! {
    Backquote, Backslash, BracketLeft, BracketRight, Comma,
    Digit0, Digit1, Digit2, Digit3, Digit4, Digit5, Digit6, Digit7, Digit8, Digit9,
    Equal, IntlBackslash, IntlRo, IntlYen,
    KeyA, KeyB, KeyC, KeyD, KeyE, KeyF, KeyG, KeyH, KeyI, KeyJ, KeyK, KeyL, KeyM,
    KeyN, KeyO, KeyP, KeyQ, KeyR, KeyS, KeyT, KeyU, KeyV, KeyW, KeyX, KeyY, KeyZ,
    Minus, Period, Quote, Semicolon, Slash,
    AltLeft, AltRight, Backspace, CapsLock, ContextMenu, ControlLeft, ControlRight,
    Enter, MetaLeft, MetaRight, ShiftLeft, ShiftRight, Space, Tab,
    Convert, KanaMode, Lang1, Lang2, Lang3, Lang4, Lang5, NonConvert,
    Delete, End, Help, Home, Insert, PageDown, PageUp,
    ArrowDown, ArrowLeft, ArrowRight, ArrowUp,
    NumLock, Numpad0, Numpad1, Numpad2, Numpad3, Numpad4, Numpad5, Numpad6, Numpad7,
    Numpad8, Numpad9, NumpadAdd, NumpadComma, NumpadDecimal, NumpadDivide, NumpadEnter,
    NumpadEqual, NumpadMultiply, NumpadParenLeft, NumpadParenRight, NumpadSubtract,
    Escape, F1, F2, F3, F4, F5, F6, F7, F8, F9, F10, F11, F12,
    F13, F14, F15, F16, F17, F18, F19, F20, F21, F22, F23, F24,
    Fn, PrintScreen, ScrollLock, Pause,
    BrowserBack, BrowserFavorites, BrowserForward, BrowserHome, BrowserRefresh,
    BrowserSearch, BrowserStop, Eject, LaunchApp1, LaunchApp2, LaunchMail,
    MediaPlayPause, MediaStop, MediaTrackNext, MediaTrackPrevious, Power, Sleep, WakeUp,
    AudioVolumeDown, AudioVolumeMute, AudioVolumeUp,
}

impl fmt::Display for PhysicalKey {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.code())
    }
}

/// The error returned when a string is not a known `KeyboardEvent.code` name.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct UnknownPhysicalKey(pub String);

impl fmt::Display for UnknownPhysicalKey {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "unknown physical key code {:?}", self.0)
    }
}

impl std::error::Error for UnknownPhysicalKey {}

impl FromStr for PhysicalKey {
    type Err = UnknownPhysicalKey;

    fn from_str(code: &str) -> Result<Self, Self::Err> {
        Self::from_code(code).ok_or_else(|| UnknownPhysicalKey(code.to_string()))
    }
}

#[cfg(test)]
mod tests {
    use super::PhysicalKey;

    #[test]
    fn test_code_names_round_trip() {
        for &key in PhysicalKey::ALL {
            assert_eq!(PhysicalKey::from_code(key.code()), Some(key));
            assert_eq!(key.code().parse::<PhysicalKey>(), Ok(key));
        }
        assert_eq!(PhysicalKey::KeyW.code(), "KeyW");
        assert_eq!(PhysicalKey::NumpadEnter.to_string(), "NumpadEnter");
        assert_eq!(PhysicalKey::from_code("Unidentified"), None);
        assert_eq!(PhysicalKey::from_code("keyw"), None);
        assert!("Unidentified".parse::<PhysicalKey>().is_err());
    }
}
