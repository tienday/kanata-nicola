//! Turns the sent key event sequence back into the same tokens as the test vectors (kana, `[OL]`/`[OR]`, `pass:*`).
//!
//! Used to check the output of the romaji encoder in the simulation and the real-device E2E.

use std::collections::HashMap;

use nicola_layout::{keycodes, LayoutDef};

use crate::encoder::{self, Encoder};
use crate::kanata_key;

pub struct Decoder {
    /// Kanata key name sequence → symbol.
    table: HashMap<Vec<String>, String>,
    /// Keys that appear in the output table (evdev code → Kanata key name).
    letters: HashMap<u16, &'static str>,
}

/// "，", "．" and "／" are sent with the same keys as "、", "。" and "・", so normalize them before comparing.
pub fn canonical(token: &str) -> String {
    token.replace('，', "、").replace('．', "。").replace('／', "・")
}

impl Decoder {
    pub fn new(layout: &LayoutDef) -> Self {
        let mut table = HashMap::new();
        for k in layout.active_keys() {
            // Romaji faces (ASCII) are keys to type, not characters.
            for sym in [&k.faces.solo, &k.faces.lthumb, &k.faces.rthumb].into_iter().flatten() {
                if encoder::is_romaji(sym) {
                    continue;
                }
                // Characters typed with Shift (？ of the number row) are left out: to the decoder Shift is
                // a modifier, and the tests that decode do not type them.
                if let Some(keys) = encoder::keys(Encoder::Romaji, sym).filter(|k| !k.iter().any(|k| k == "lsft")) {
                    table.insert(keys, canonical(sym));
                }
            }
        }
        let mut letters = HashMap::new();
        for code in 1..=127u16 {
            if let Some(n) = keycodes::name_of(code).and_then(kanata_key) {
                if table.keys().any(|seq: &Vec<String>| seq.iter().any(|k| k == n)) {
                    letters.insert(code, n);
                }
            }
        }
        Self { table, letters }
    }

    /// `events` is a sequence of (evdev code, value). Value 1 = down, 0 = up, 2 = repeat.
    pub fn decode(&self, events: &[(u16, i32)]) -> Vec<String> {
        let mut st = State::default();
        for &(code, value) in events {
            let name = keycodes::name_of(code).unwrap_or("?").trim_start_matches("KEY_");
            if value == 2 {
                continue;
            }
            let down = value == 1;
            if self.letters.contains_key(&code) && st.modifiers == 0 {
                if down {
                    self.letter(&mut st, code);
                }
                continue;
            }
            self.resolve(&mut st);
            if keycodes::is_shift(code) || keycodes::is_modifier(code) {
                if down {
                    st.modifiers += 1;
                } else {
                    st.modifiers -= 1;
                }
                st.out.push(format!("pass:{}:{name}", if down { "d" } else { "u" }));
                continue;
            }
            match (name, down) {
                ("MUHENKAN", true) => st.out.push("[OL]".into()),
                ("HENKAN", true) => st.out.push("[OR]".into()),
                ("MUHENKAN" | "HENKAN", false) => {}
                (_, true) if st.modifiers > 0 => {
                    st.under_modifier.push(code);
                    st.out.push(format!("pass:d:{name}"));
                }
                (_, false) if st.under_modifier.contains(&code) => {
                    st.under_modifier.retain(|c| *c != code);
                    st.out.push(format!("pass:u:{name}"));
                }
                // Releasing an output key that was pressed before the modifier types nothing.
                (_, false) if self.letters.contains_key(&code) => {}
                (_, d) => st.out.push(format!("pass:{}:{name}", if d { "d" } else { "u" })),
            }
        }
        self.resolve(&mut st);
        st.out
    }

    fn letter(&self, st: &mut State, code: u16) {
        let k = self.letters[&code].to_string();
        let mut next = st.pending.clone();
        next.push(k.clone());
        if self.is_prefix(&next) {
            st.pending = next;
        } else {
            // It cannot be extended, so commit what we have and start a new sequence.
            self.resolve(st);
            st.pending = vec![k];
            if !self.is_prefix(&st.pending) {
                self.resolve(st);
            }
        }
        // Commit at once a sequence that matches exactly and cannot be extended further.
        if self.table.contains_key(&st.pending) && !self.extends(&st.pending) {
            self.resolve(st);
        }
    }

    fn is_prefix(&self, seq: &[String]) -> bool {
        self.table.keys().any(|s| s.starts_with(seq))
    }

    fn extends(&self, seq: &[String]) -> bool {
        self.table.keys().any(|s| s.len() > seq.len() && s.starts_with(seq))
    }

    /// Commits the pending sequence. If it is not in the table, emits it as passed-through keys.
    fn resolve(&self, st: &mut State) {
        if st.pending.is_empty() {
            return;
        }
        let pending = std::mem::take(&mut st.pending);
        if let Some(sym) = self.table.get(&pending) {
            st.out.push(sym.clone());
            return;
        }
        for p in pending {
            st.out.push(format!("pass:d:{}", p.to_uppercase()));
        }
    }
}

#[derive(Default)]
struct State {
    out: Vec<String>,
    pending: Vec<String>,
    modifiers: i32,
    under_modifier: Vec<u16>,
}
