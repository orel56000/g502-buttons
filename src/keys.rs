//! Key names ("ctrl+alt+f13") and the codes each platform needs to inject them.

use crate::platform::Injector;

/// A key as both a Windows virtual-key code and a Linux evdev key code.
#[derive(Clone, Copy)]
#[allow(dead_code)] // each platform reads only its own field
pub struct Key {
    pub win: u16,
    pub linux: u16,
}

// name, Windows virtual-key, Linux evdev code
const NAMED: &[(&str, u16, u16)] = &[
    ("ctrl", 0x11, 29),
    ("control", 0x11, 29),
    ("alt", 0x12, 56),
    ("shift", 0x10, 42),
    ("win", 0x5B, 125),
    ("super", 0x5B, 125),
    ("space", 0x20, 57),
    ("enter", 0x0D, 28),
    ("esc", 0x1B, 1),
    ("escape", 0x1B, 1),
    ("tab", 0x09, 15),
    ("backspace", 0x08, 14),
    ("insert", 0x2D, 110),
    ("delete", 0x2E, 111),
    ("home", 0x24, 102),
    ("end", 0x23, 107),
    ("pageup", 0x21, 104),
    ("pagedown", 0x22, 109),
    ("left", 0x25, 105),
    ("up", 0x26, 103),
    ("right", 0x27, 106),
    ("down", 0x28, 108),
    ("mute", 0xAD, 113),
    ("volume_down", 0xAE, 114),
    ("volume_up", 0xAF, 115),
    ("next_track", 0xB0, 163),
    ("prev_track", 0xB1, 165),
    ("stop", 0xB2, 166),
    ("play_pause", 0xB3, 164),
];

// evdev numbers letters by keyboard row.
const LINUX_LETTER_ROWS: [(&str, u16); 3] = [("qwertyuiop", 16), ("asdfghjkl", 30), ("zxcvbnm", 44)];

pub struct Combo(Vec<Key>);

impl Combo {
    pub fn parse(s: &str) -> Result<Self, String> {
        s.split('+')
            .map(|t| key(&t.trim().to_ascii_lowercase()).ok_or_else(|| format!("unknown key '{t}'")))
            .collect::<Result<_, _>>()
            .map(Combo)
    }

    pub fn press(&self, inj: &mut Injector) {
        self.0.iter().for_each(|&k| inj.send(k, true));
    }

    pub fn release(&self, inj: &mut Injector) {
        self.0.iter().rev().for_each(|&k| inj.send(k, false));
    }
}

fn key(name: &str) -> Option<Key> {
    if let &[c] = name.as_bytes() {
        if c.is_ascii_digit() {
            let linux = if c == b'0' { 11 } else { (c - b'1') as u16 + 2 };
            return Some(Key { win: c as u16, linux });
        }
        if c.is_ascii_lowercase() {
            let linux = LINUX_LETTER_ROWS
                .iter()
                .find_map(|(row, start)| row.find(c as char).map(|i| start + i as u16))?;
            return Some(Key { win: c.to_ascii_uppercase() as u16, linux });
        }
    }
    if let Some(n) = name.strip_prefix('f').and_then(|n| n.parse::<u16>().ok()) {
        let linux = match n {
            1..=10 => 58 + n,
            11 => 87,
            12 => 88,
            13..=24 => 170 + n,
            _ => return None,
        };
        return Some(Key { win: 0x6F + n, linux });
    }
    NAMED.iter().find(|(n, ..)| *n == name).map(|&(_, win, linux)| Key { win, linux })
}
