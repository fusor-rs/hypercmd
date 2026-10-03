use crate::{Error, Input, Key, KeyKind, Modifiers};
use std::time::{Duration, Instant};

const ESCAPE_WAIT: Duration = Duration::from_millis(40);
const MAX_SEQUENCE: usize = 64;
const WHEEL_STEP: i32 = 3;
const SHIFT: u8 = 1;
const ALT: u8 = 2;
const CONTROL: u8 = 4;
const SUPER: u8 = 8;
const LOCKS: u8 = 64 | 128;
const PASTE_END: &[u8] = b"\x1b[201~";

#[derive(Debug, PartialEq)]
pub(crate) enum Decoded {
    Input(Input),
    Shutdown,
    Suspend,
}

impl Decoded {
    fn from_input(input: Input) -> Self {
        let Input::Key {
            key,
            modifiers,
            kind: KeyKind::Press,
        } = &input
        else {
            return Self::Input(input);
        };
        if !modifiers.control || modifiers.alt || modifiers.super_key {
            return Self::Input(input);
        }
        match key {
            Key::Char('c') => Self::Shutdown,
            Key::Char('z') => Self::Suspend,
            _ => Self::Input(input),
        }
    }
}

/// The only unfinished input storage: a bounded sequence or a bounded paste.
pub(crate) struct Decoder {
    sequence: Vec<u8>,
    deadline: Option<Instant>,
    paste: Option<Vec<u8>>,
    max_paste: usize,
}

impl Decoder {
    fn reset(&mut self) {
        self.sequence.clear();
        self.deadline = None;
    }
    pub fn new(max_paste: usize) -> Self {
        Self {
            sequence: Vec::with_capacity(MAX_SEQUENCE),
            deadline: None,
            paste: None,
            max_paste,
        }
    }

    pub fn timeout(&self, now: Instant) -> Option<Duration> {
        self.deadline
            .map(|deadline| deadline.saturating_duration_since(now))
    }

    pub fn expire(&mut self, now: Instant) -> Option<Input> {
        if self.deadline.is_none_or(|deadline| now < deadline) {
            return None;
        }
        let escaped = self.sequence == b"\x1b";
        self.reset();
        escaped.then(|| press(Key::Escape, false))
    }

    pub fn feed(&mut self, byte: u8, now: Instant) -> Result<Option<Decoded>, Error> {
        if let Some(paste) = &mut self.paste {
            // The extra six bytes permit a terminator split across reads at the limit.
            if paste.len() >= self.max_paste.saturating_add(PASTE_END.len()) {
                return Err(Error::limit(
                    "bracketed paste exceeds the configured byte limit",
                ));
            }
            paste.push(byte);
            if paste.ends_with(PASTE_END) {
                paste.truncate(paste.len() - PASTE_END.len());
                if paste.len() > self.max_paste {
                    return Err(Error::limit(
                        "bracketed paste exceeds the configured byte limit",
                    ));
                }
                let bytes = self.paste.take().expect("paste state exists");
                return Ok(Some(Decoded::Input(Input::Paste(
                    String::from_utf8_lossy(&bytes).into_owned(),
                ))));
            }
            return Ok(None);
        }
        if self.sequence.is_empty() && byte != 0x1b && byte < 0x80 {
            return Ok(plain(byte));
        }
        if self.sequence.len() == MAX_SEQUENCE {
            return Err(Error::limit("terminal input sequence exceeds 64 bytes"));
        }
        self.sequence.push(byte);
        if self.sequence[0] != 0x1b {
            return self.utf8();
        }
        self.deadline = Some(now + ESCAPE_WAIT);
        if self.sequence == b"\x1b[200~" {
            self.reset();
            self.paste = Some(Vec::new());
            return Ok(None);
        }
        let complete = match self.sequence.as_slice() {
            [0x1b] | [0x1b, b'[' | b'O'] => false,
            [0x1b, b'[' | b'O', rest @ ..] => {
                rest.last().is_some_and(|byte| (0x40..=0x7e).contains(byte))
            }
            _ => true,
        };
        if !complete {
            return Ok(None);
        }
        let key = mouse(&self.sequence).or_else(|| escape_key(&self.sequence));
        self.reset();
        Ok(key.map(Decoded::from_input))
    }

    fn utf8(&mut self) -> Result<Option<Decoded>, Error> {
        match std::str::from_utf8(&self.sequence) {
            Ok(text) => {
                let key = text.chars().next().map(Key::Char);
                self.reset();
                Ok(key.map(|key| Decoded::Input(press(key, false))))
            }
            Err(error) if error.error_len().is_none() && self.sequence.len() < 4 => Ok(None),
            Err(_) => Err(Error::terminal("terminal input is not valid UTF-8")),
        }
    }
}

fn press(key: Key, shift: bool) -> Input {
    Input::Key {
        key,
        modifiers: Modifiers {
            shift,
            ..Modifiers::default()
        },
        kind: KeyKind::Press,
    }
}

fn plain(byte: u8) -> Option<Decoded> {
    let key = match byte {
        9 => Key::Tab,
        10 | 13 => Key::Enter,
        8 | 127 => Key::Backspace,
        32..=126 => Key::Char(char::from(byte)),
        1..=26 => {
            return Some(Decoded::from_input(Input::Key {
                key: Key::Char(char::from(b'a' + byte - 1)),
                modifiers: Modifiers {
                    control: true,
                    ..Modifiers::default()
                },
                kind: KeyKind::Press,
            }));
        }
        _ => return None,
    };
    Some(Decoded::Input(press(key, false)))
}

fn mouse(sequence: &[u8]) -> Option<Input> {
    let payload = sequence.strip_prefix(b"\x1b[<")?.strip_suffix(b"M")?;
    // Unknown mouse reports are not application input.
    let payload = std::str::from_utf8(payload).ok()?;
    let mut parts = payload.split(';');
    let button = parts.next()?.parse::<u8>().ok()?;
    let column = parts.next()?.parse::<u16>().ok()?.checked_sub(1)?;
    let row = parts.next()?.parse::<u16>().ok()?.checked_sub(1)?;
    if parts.next().is_some() {
        return None;
    }
    if button == 0 {
        return Some(Input::Click { column, row });
    }
    let (columns, rows) = match button {
        64 => (0, -WHEEL_STEP),
        65 => (0, WHEEL_STEP),
        66 | 68 => (-WHEEL_STEP, 0),
        67 | 69 => (WHEEL_STEP, 0),
        _ => return None,
    };
    Some(Input::Scroll {
        column,
        row,
        rows,
        columns,
    })
}

fn escape_key(sequence: &[u8]) -> Option<Input> {
    let (&last, prefix) = sequence.split_last()?;
    let params = std::str::from_utf8(prefix.get(2..)?).ok()?;
    let mut parts = params.split(';');
    let number = parts.next().unwrap_or("");
    let modifier = parts.next().unwrap_or("1");
    let mut modifier_parts = modifier.split(':');
    let modifiers: u8 = modifier_parts.next()?.parse().ok()?;
    let flags = modifiers.checked_sub(1)?;
    if flags & !(SHIFT | ALT | CONTROL | SUPER | LOCKS) != 0 {
        return None;
    }
    let modifiers = Modifiers {
        shift: flags & SHIFT != 0,
        alt: flags & ALT != 0,
        control: flags & CONTROL != 0,
        super_key: flags & SUPER != 0,
    };
    let kind = match modifier_parts.next().unwrap_or("1") {
        "1" => KeyKind::Press,
        "2" => KeyKind::Repeat,
        "3" => KeyKind::Release,
        _ => return None,
    };
    let key = match last {
        b'A' => Key::Up,
        b'B' => Key::Down,
        b'C' => Key::Right,
        b'D' => Key::Left,
        b'H' => Key::Home,
        b'F' => Key::End,
        b'Z' => return Some(press(Key::Tab, true)),
        b'~' => match number {
            "1" | "7" => Key::Home,
            "4" | "8" => Key::End,
            "3" => Key::Delete,
            "5" => Key::PageUp,
            "6" => Key::PageDown,
            _ => return None,
        },
        b'u' => match number.parse::<u32>().ok()? {
            9 => Key::Tab,
            13 => Key::Enter,
            127 => Key::Backspace,
            27 => Key::Escape,
            number => Key::Char(char::from_u32(number)?),
        },
        _ => return None,
    };
    Some(Input::Key {
        key,
        modifiers,
        kind,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ErrorKind;

    // Raw decoding is not exercised by generated consumers; in particular the
    // paste limit must hold before a completed Input::Paste exists.
    #[test]
    fn paste_is_one_edit_and_unfinished_input_is_bounded() {
        let now = Instant::now();
        let mut decoder = Decoder::new(16);
        let mut events = Vec::new();
        for byte in b"\x1b[200~a\x03\r\n\x1b[A\x1b[201~" {
            if let Some(event) = decoder.feed(*byte, now).unwrap() {
                events.push(event);
            }
        }
        assert_eq!(events.len(), 1);
        assert!(
            matches!(&events[0], Decoded::Input(Input::Paste(value)) if value == "a\x03\r\n\x1b[A")
        );
        let mut decoder = Decoder::new(8);
        for byte in b"\x1b[200~" {
            decoder.feed(*byte, now).unwrap();
        }
        let error = (0..20).find_map(|_| decoder.feed(b'x', now).err()).unwrap();
        assert_eq!(error.kind, ErrorKind::Limit);
        assert!(decoder.paste.as_ref().unwrap().len() <= 14);
        let mut decoder = Decoder::new(8);
        for byte in b"\x1b[" {
            decoder.feed(*byte, now).unwrap();
        }
        assert!((0..70).any(|_| decoder.feed(b'1', now).is_err()));
    }

    #[test]
    fn fragmented_unicode_and_keys_preserve_event_kind() {
        let now = Instant::now();
        let mut decoder = Decoder::new(8);
        let events: Vec<_> =
            "界\x1b[1;2D\x1b[32;1:3u\x1b[32;1:2u\x03\x1b[A\x1bOB\x1b[1;1:3A\x1b[1;1:2B"
                .bytes()
                .filter_map(|byte| decoder.feed(byte, now).unwrap())
                .collect();
        let key = |key, shift, kind| {
            Decoded::Input(Input::Key {
                key,
                modifiers: Modifiers {
                    shift,
                    ..Modifiers::default()
                },
                kind,
            })
        };
        assert_eq!(
            events,
            [
                key(Key::Char('界'), false, KeyKind::Press),
                key(Key::Left, true, KeyKind::Press),
                key(Key::Char(' '), false, KeyKind::Release),
                key(Key::Char(' '), false, KeyKind::Repeat),
                Decoded::Shutdown,
                key(Key::Up, false, KeyKind::Press),
                key(Key::Down, false, KeyKind::Press),
                key(Key::Up, false, KeyKind::Release),
                key(Key::Down, false, KeyKind::Repeat),
            ]
        );
        pointer_and_shortcut_reports(&mut decoder, now);
        command_and_control_reports(&mut decoder, now);
        decoder.feed(0x1b, now).unwrap();
        assert_eq!(
            decoder.expire(now + ESCAPE_WAIT),
            Some(press(Key::Escape, false))
        );
        assert!(decoder.timeout(now).is_none());
        assert_eq!(
            decoder.feed(b'x', now).unwrap(),
            Some(key(Key::Char('x'), false, KeyKind::Press))
        );
    }
    fn command_and_control_reports(decoder: &mut Decoder, now: Instant) {
        let events: Vec<_> = b"\x1b[13;9u\x1b[13;73u\x1b[13;9:3u\x1b[99;5u\x1b[122;5u"
            .iter()
            .filter_map(|byte| decoder.feed(*byte, now).unwrap())
            .collect();
        let command_enter = Input::Key {
            key: Key::Enter,
            modifiers: Modifiers {
                super_key: true,
                ..Modifiers::default()
            },
            kind: KeyKind::Press,
        };
        assert_eq!(
            events,
            [
                Decoded::Input(command_enter.clone()),
                Decoded::Input(command_enter),
                Decoded::Input(Input::Key {
                    key: Key::Enter,
                    modifiers: Modifiers {
                        super_key: true,
                        ..Modifiers::default()
                    },
                    kind: KeyKind::Release,
                }),
                Decoded::Shutdown,
                Decoded::Suspend,
            ]
        );
    }

    fn pointer_and_shortcut_reports(decoder: &mut Decoder, now: Instant) {
        let reports: Vec<_> = b"\x12\x1b[<64;12;8M\x1b[<67;12;8M\x1b[<0;2;3M"
            .iter()
            .filter_map(|byte| decoder.feed(*byte, now).unwrap())
            .collect();
        assert_eq!(
            reports,
            [
                Decoded::Input(Input::Key {
                    key: Key::Char('r'),
                    modifiers: Modifiers {
                        control: true,
                        ..Modifiers::default()
                    },
                    kind: KeyKind::Press
                }),
                Decoded::Input(Input::Scroll {
                    column: 11,
                    row: 7,
                    rows: -3,
                    columns: 0
                }),
                Decoded::Input(Input::Scroll {
                    column: 11,
                    row: 7,
                    rows: 0,
                    columns: 3
                }),
                Decoded::Input(Input::Click { column: 1, row: 2 }),
            ]
        );
    }
}
