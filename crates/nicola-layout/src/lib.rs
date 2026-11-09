//! Loading the layout definition file (docs/implementation.md, file formats).
//!
//! The layout definition only returns the characters of the faces; choosing the keys to send is left to the
//! encoder. Both the kanata configuration generator and the NICOLA engine (developed separately, not yet public;
//! docs/design.md §1.2) get the layout from here.

pub mod keycodes;

use std::collections::BTreeMap;

use serde::Deserialize;

/// Left or right thumb key.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Thumb {
    Left,
    Right,
}

/// The three faces of one character key. An undefined face is `None`.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Faces {
    pub solo: Option<String>,
    pub left: Option<String>,
    pub right: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct Meta {
    pub name: String,
    pub variant: String,
    #[serde(default)]
    pub source: String,
}

#[derive(Debug, Deserialize)]
struct RawKey {
    pos: String,
    #[allow(dead_code)]
    region: String,
    solo: Option<String>,
    left: Option<String>,
    right: Option<String>,
}

/// One key of an overlay file. Replaces only the faces written. `""` removes a face.
#[derive(Debug, Default, Deserialize)]
#[serde(deny_unknown_fields)]
struct OverlayKey {
    pos: Option<String>,
    solo: Option<String>,
    left: Option<String>,
    right: Option<String>,
}

/// An overlay file. A table other than `meta` and `keys` is an error: a misspelt `[key.KEY_W]` would otherwise
/// change nothing without a word.
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct RawOverlay {
    #[serde(default)]
    meta: toml::Table,
    #[serde(default)]
    keys: BTreeMap<String, OverlayKey>,
}

#[derive(Debug, Deserialize)]
struct RawLayout {
    meta: Meta,
    keys: BTreeMap<String, RawKey>,
}

/// Definition of one key.
#[derive(Clone, Debug)]
pub struct KeyDef {
    /// evdev key name (`KEY_W`, etc.).
    pub name: String,
    pub code: u16,
    /// JIS position code (`D02`, etc.).
    pub pos: String,
    pub faces: Faces,
}

#[derive(Debug)]
pub struct LayoutDef {
    pub meta: Meta,
    /// In row/column order (D01, D02, …, B10).
    pub keys: Vec<KeyDef>,
    /// Names of the overlay files applied so far (their `[meta] name`), in order.
    pub overlays: Vec<String>,
}

#[derive(Debug)]
pub struct Error(pub String);

impl std::fmt::Display for Error {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}

impl std::error::Error for Error {}

impl LayoutDef {
    pub fn parse(src: &str) -> Result<Self, Error> {
        let raw: RawLayout = toml::from_str(src).map_err(|e| Error(e.to_string()))?;
        let mut keys = Vec::new();
        for (name, k) in raw.keys {
            let code = keycodes::code_of(&name).ok_or_else(|| Error(format!("unknown key name: {name}")))?;
            let faces = Faces { solo: k.solo, left: k.left, right: k.right };
            keys.push(KeyDef { name, code, pos: k.pos, faces });
        }
        keys.sort_by_key(|k| pos_order(&k.pos));
        Ok(Self { meta: raw.meta, keys, overlays: Vec::new() })
    }

    /// Applies an overlay file (docs/layout-overlays.md).
    ///
    /// Replaces only the faces written; faces not written stay as in the base layout. An empty string `""` removes that face.
    /// A key left with none of its three faces drops out of the layout on the consumer side (`active_keys`) (the physical key is typed as is).
    /// Its position is kept, so a later overlay can add faces back.
    /// Keys not in the base layout can also be added. `pos` sets the position (also of an existing key).
    pub fn apply_overlay(&mut self, src: &str) -> Result<(), Error> {
        let raw: RawOverlay = toml::from_str(src).map_err(|e| Error(e.to_string()))?;
        for (name, o) in raw.keys {
            let code = keycodes::code_of(&name).ok_or_else(|| Error(format!("overlay: unknown key name: {name}")))?;
            let idx = match self.keys.iter().position(|k| k.name == name) {
                Some(i) => i,
                None => {
                    self.keys.push(KeyDef {
                        name: name.clone(),
                        code,
                        pos: o.pos.clone().unwrap_or_default(),
                        faces: Faces::default(),
                    });
                    self.keys.len() - 1
                }
            };
            let k = &mut self.keys[idx];
            if let Some(pos) = o.pos {
                k.pos = pos;
            }
            let set = |face: &mut Option<String>, v: Option<String>| {
                if let Some(v) = v {
                    *face = (!v.is_empty()).then_some(v);
                }
            };
            set(&mut k.faces.solo, o.solo);
            set(&mut k.faces.left, o.left);
            set(&mut k.faces.right, o.right);
        }
        self.keys.sort_by_key(|k| pos_order(&k.pos));
        let name = raw.meta.get("name").and_then(|v| v.as_str()).unwrap_or("(unnamed)");
        self.overlays.push(name.to_string());
        Ok(())
    }

    /// Keys with at least one face (the keys that take part in decisions).
    pub fn active_keys(&self) -> impl Iterator<Item = &KeyDef> {
        self.keys.iter().filter(|k| k.faces.solo.is_some() || k.faces.left.is_some() || k.faces.right.is_some())
    }

    /// Looks up by character key (evdev name) and thumb (no thumb = solo).
    /// If the shifted face is undefined, falls back to the solo face (ADR-15).
    pub fn lookup(&self, name: &str, thumb: Option<Thumb>) -> Option<&str> {
        let k = self.active_keys().find(|k| k.name == name)?;
        let shifted = match thumb {
            None => None,
            Some(Thumb::Left) => k.faces.left.as_deref(),
            Some(Thumb::Right) => k.faces.right.as_deref(),
        };
        shifted.or(k.faces.solo.as_deref())
    }

    /// The three faces of a character key (evdev name). `None` for a key with no faces.
    pub fn faces(&self, name: &str) -> Option<&Faces> {
        self.active_keys().find(|k| k.name == name).map(|k| &k.faces)
    }
}

/// Sorts in the order D01..D12, C01..C13, B01..B11.
fn pos_order(pos: &str) -> (u8, u8) {
    let row = match pos.as_bytes().first() {
        Some(b'E') => 0,
        Some(b'D') => 1,
        Some(b'C') => 2,
        Some(b'B') => 3,
        _ => 9,
    };
    (row, pos.get(1..).and_then(|n| n.parse().ok()).unwrap_or(99))
}

/// The J-type layout definition shipped with the repository.
pub const NICOLA_J: &str = include_str!("../../../layouts/nicola-j.toml");

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn j_layout_has_43_keys() {
        let l = LayoutDef::parse(NICOLA_J).unwrap();
        assert_eq!(l.keys.len(), 43);
        assert_eq!(l.keys[0].pos, "E01");
        assert_eq!(l.keys.last().unwrap().pos, "B10");
        assert_eq!(l.lookup("KEY_W", None).unwrap(), "か");
        assert_eq!(l.lookup("KEY_W", Some(Thumb::Left)).unwrap(), "え");
        assert_eq!(l.lookup("KEY_W", Some(Thumb::Right)).unwrap(), "が");
    }

    #[test]
    fn overlay_replaces_removes_and_adds_faces() {
        let mut l = LayoutDef::parse(NICOLA_J).unwrap();
        l.apply_overlay(
            r#"
            [meta]
            name = "test"
            [keys.KEY_SEMICOLON]
            right = "ん"
            [keys.KEY_RIGHTBRACE]
            solo = ""
            right = ""
            [keys.KEY_APOSTROPHE]
            pos = "C11"
            solo = "→"
            "#,
        )
        .unwrap();
        // Only the faces written are replaced; the others remain.
        assert_eq!(l.lookup("KEY_SEMICOLON", Some(Thumb::Right)).unwrap(), "ん");
        assert_eq!(l.lookup("KEY_SEMICOLON", None).unwrap(), "ん");
        // A key with all three faces removed drops out of the layout.
        assert!(l.faces("KEY_RIGHTBRACE").is_none());
        // A key not in the base layout can be added.
        assert_eq!(l.lookup("KEY_APOSTROPHE", None).unwrap(), "→");
        assert_eq!(l.active_keys().count(), 43);
        // `pos` also moves an existing key.
        l.apply_overlay("[keys.KEY_W]\npos = \"B11\"").unwrap();
        assert_eq!(l.keys.last().unwrap().name, "KEY_W");
        assert_eq!(l.lookup("KEY_W", None).unwrap(), "か");
    }

    #[test]
    fn overlay_rejects_unknown_keys_and_fields() {
        let mut l = LayoutDef::parse(NICOLA_J).unwrap();
        assert!(l.apply_overlay("[keys.KEY_NOPE]\nsolo = \"あ\"").is_err());
        assert!(l.apply_overlay("[keys.KEY_W]\nsoro = \"あ\"").is_err());
        // Misspelt tables.
        assert!(l.apply_overlay("[key.KEY_W]\nsolo = \"あ\"").is_err());
        assert!(l.apply_overlay("[mata]\nname = \"x\"").is_err());
        // [meta] takes anything (description etc.).
        assert!(l.apply_overlay("[meta]\nname = \"x\"\ndescription = \"y\"").is_ok());
    }
}
