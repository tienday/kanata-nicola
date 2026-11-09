//! The output encoder (docs/design.md, output).
//!
//! Converts a character of the layout into the key sequence that kanata sends. A face written in ASCII is not a
//! character but the romaji to type in the IME (`z*`), sent as written ([`typed_keys`]).
//!
//! How the keys are sent (pressed together in one tick, released in the next) is decided by the generator in `lib.rs`.
//! This module only decides which keys to press.

/// Encoder.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Encoder {
    /// Used with the IME in romaji input (`か` → `k a`).
    Romaji,
    /// Kanata's unicode output, one output per character. It bypasses the IME, so it is not for real use; the tests
    /// use it to check the decision logic apart from the romaji sending.
    Unicode,
}

/// The romaji table: the romaji that the IME turns into the character, as typed on a JIS keyboard (the keys
/// to send are those of [`typed_keys`]).
///
/// Uses spellings that the standard romaji table (Mozc default) accepts.
/// Full-width "，" and "．" cannot be typed in romaji, so "、" and "。" are used instead.
/// Never uses the same key twice within one character (all its keys are pressed in the same tick): "ん" is `xn`.
/// "゛" and "゜" alone have nothing to combine with, so they are not sent (OQ-10).
pub fn romaji(symbol: &str) -> Option<&'static str> {
    Some(match symbol {
        "あ" => "a",
        "い" => "i",
        "う" => "u",
        "え" => "e",
        "お" => "o",
        "か" => "ka",
        "き" => "ki",
        "く" => "ku",
        "け" => "ke",
        "こ" => "ko",
        "さ" => "sa",
        "し" => "si",
        "す" => "su",
        "せ" => "se",
        "そ" => "so",
        "た" => "ta",
        "ち" => "ti",
        "つ" => "tu",
        "て" => "te",
        "と" => "to",
        "な" => "na",
        "に" => "ni",
        "ぬ" => "nu",
        "ね" => "ne",
        "の" => "no",
        "は" => "ha",
        "ひ" => "hi",
        "ふ" => "hu",
        "へ" => "he",
        "ほ" => "ho",
        "ま" => "ma",
        "み" => "mi",
        "む" => "mu",
        "め" => "me",
        "も" => "mo",
        "や" => "ya",
        "ゆ" => "yu",
        "よ" => "yo",
        "ら" => "ra",
        "り" => "ri",
        "る" => "ru",
        "れ" => "re",
        "ろ" => "ro",
        "わ" => "wa",
        "を" => "wo",
        "ん" => "xn",
        "が" => "ga",
        "ぎ" => "gi",
        "ぐ" => "gu",
        "げ" => "ge",
        "ご" => "go",
        "ざ" => "za",
        "じ" => "zi",
        "ず" => "zu",
        "ぜ" => "ze",
        "ぞ" => "zo",
        "だ" => "da",
        "ぢ" => "di",
        "づ" => "du",
        "で" => "de",
        "ど" => "do",
        "ば" => "ba",
        "び" => "bi",
        "ぶ" => "bu",
        "べ" => "be",
        "ぼ" => "bo",
        "ぱ" => "pa",
        "ぴ" => "pi",
        "ぷ" => "pu",
        "ぺ" => "pe",
        "ぽ" => "po",
        "ぁ" => "xa",
        "ぃ" => "xi",
        "ぅ" => "xu",
        "ぇ" => "xe",
        "ぉ" => "xo",
        "ゃ" => "xya",
        "ゅ" => "xyu",
        "ょ" => "xyo",
        "っ" => "xtu",
        "ゔ" => "vu",
        "ー" => "-",
        "。" | "．" => ".",
        "、" | "，" => ",",
        // / is ・ with the IME's default symbol style (「」・, in Mozc and Microsoft IME alike), ／ with 「」／ or
        // ［］／ (as ／ of the number row). `z/` would keep ・ in both, but Microsoft IME has no such spelling.
        "・" => "/",
        // Symbols (z + key in the Mozc default romaji table).
        "→" => "zl",
        "←" => "zh",
        "↑" => "zk",
        "↓" => "zj",
        "…" => "z.",
        "‥" => "z,",
        "『" => "z[",
        "』" => "z]",
        "〜" => "z-",
        // Number row (E). Mozc makes the ASCII symbols full-width; [ ] are 「 」 and / is ・ with its default
        // symbol style (／ with 「」／ or ［］／).
        "？" => "?",
        "／" => "/",
        "「" => "[",
        "」" => "]",
        "｛" => "{",
        "｝" => "}",
        "（" => "(",
        "）" => ")",
        _ => return None,
    })
}

/// Characters of the NICOLA layout that the romaji encoder cannot send (OQ-10). Their faces give the
/// key itself; any other character missing from the romaji table is an error ([`check`]).
pub const UNSENDABLE: [&str; 2] = ["゛", "゜"];

/// Whether a face is romaji to type in the IME: one written in ASCII. It is sent as written
/// ([`typed_keys`]), whatever the IME makes of it (`z*` gives ※ if the IME's romaji table has it).
pub fn is_romaji(face: &str) -> bool {
    face.is_ascii()
}

/// The key that types a printable ASCII character on a JIS keyboard (xkb `jp`), and whether it needs
/// Shift. Kanata key names are US key positions.
fn jis_key(c: char) -> Option<(&'static str, bool)> {
    const LETTERS: [&str; 26] = [
        "a", "b", "c", "d", "e", "f", "g", "h", "i", "j", "k", "l", "m", "n", "o", "p", "q", "r", "s", "t", "u", "v",
        "w", "x", "y", "z",
    ];
    const DIGITS: [&str; 10] = ["0", "1", "2", "3", "4", "5", "6", "7", "8", "9"];
    Some(match c {
        'a'..='z' => (LETTERS[c as usize - 'a' as usize], false),
        'A'..='Z' => (LETTERS[c as usize - 'A' as usize], true),
        '0'..='9' => (DIGITS[c as usize - '0' as usize], false),
        '!' => ("1", true),
        '"' => ("2", true),
        '#' => ("3", true),
        '$' => ("4", true),
        '%' => ("5", true),
        '&' => ("6", true),
        '\'' => ("7", true),
        '(' => ("8", true),
        ')' => ("9", true),
        '-' => ("min", false),
        '=' => ("min", true),
        '^' => ("=", false),
        '~' => ("=", true),
        '\\' => ("ro", false),
        '_' => ("ro", true),
        '|' => ("¥", true),
        '@' => ("[", false),
        '`' => ("[", true),
        '[' => ("]", false),
        '{' => ("]", true),
        ';' => ("scln", false),
        '+' => ("scln", true),
        ':' => ("'", false),
        '*' => ("'", true),
        ']' => ("\\", false),
        '}' => ("\\", true),
        ',' => ("comm", false),
        '<' => ("comm", true),
        '.' => (".", false),
        '>' => (".", true),
        '/' => ("/", false),
        '?' => ("/", true),
        _ => return None,
    })
}

/// The keys that type a romaji face on a JIS keyboard, in press order: the keys of the characters
/// without Shift, then `lsft` and the keys of the characters with Shift (`z*` → `z lsft '`). All the
/// keys of a face are pressed at once, in this order, so the characters that need Shift must come last,
/// and no key can be used twice. The error says why the face cannot be typed.
pub fn typed_keys(romaji: &str) -> Result<Vec<&'static str>, String> {
    let mut keys = Vec::new();
    let mut shifted = None;
    for c in romaji.chars() {
        let Some((key, shift)) = jis_key(c) else {
            return Err(format!("{c:?} cannot be typed (romaji is printable ASCII, without spaces)"));
        };
        match (shift, shifted) {
            (true, None) => {
                shifted = Some(c);
                keys.push("lsft");
            }
            (false, Some(s)) => {
                return Err(format!(
                    "{c:?} comes after {s:?}, which needs Shift: the keys of a face are pressed at once, so the \
                     characters that need Shift must come last"
                ))
            }
            _ => {}
        }
        if keys.contains(&key) {
            return Err(format!(
                "{c:?} is on a key that the face already uses: the keys of a face are pressed at once, so each \
                 key can be used once"
            ));
        }
        keys.push(key);
    }
    Ok(keys)
}

/// Returns the keys to send (Kanata key names) in press order: those of the romaji to type
/// ([`typed_keys`]), written as is or of the character in the romaji table.
/// None for the Unicode encoder (it sends with the unicode action, not keys).
pub fn keys(encoder: Encoder, symbol: &str) -> Option<Vec<String>> {
    match encoder {
        Encoder::Romaji => {
            let romaji = if is_romaji(symbol) { symbol } else { romaji(symbol)? };
            Some(typed_keys(romaji).ok()?.into_iter().map(String::from).collect())
        }
        Encoder::Unicode => None,
    }
}

/// Whether the face can be sent. The Unicode encoder sends characters only, not romaji to type.
pub fn supports(encoder: Encoder, symbol: &str) -> bool {
    match encoder {
        Encoder::Romaji => keys(encoder, symbol).is_some(),
        Encoder::Unicode => !is_romaji(symbol),
    }
}

/// Checks a face of the layout: romaji that cannot be typed, or a character that the encoder cannot
/// send (other than [`UNSENDABLE`]), is an error. The character codes in the message tell
/// look-alike characters apart (`～` U+FF5E is not `〜` U+301C, nor `ｚ` U+FF5A the romaji `z`).
pub fn check(encoder: Encoder, face: &str) -> Result<(), String> {
    if is_romaji(face) {
        return typed_keys(face).map(|_| ());
    }
    if supports(encoder, face) || UNSENDABLE.contains(&face) {
        return Ok(());
    }
    let codes: Vec<String> = face.chars().map(|c| format!("U+{:04X}", c as u32)).collect();
    Err(format!(
        "{face:?} ({}) is not in the romaji table; to send what the IME should type, write the romaji in ASCII \
         (half-width letters and symbols)",
        codes.join(" ")
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The keys of a JIS keyboard other than the letters, as xkb `jp` (106) defines them, by row:
    /// kanata key, the character without Shift, with Shift.
    const JIS: &[(&str, char, char)] = &[
        ("1", '1', '!'),
        ("2", '2', '"'),
        ("3", '3', '#'),
        ("4", '4', '$'),
        ("5", '5', '%'),
        ("6", '6', '&'),
        ("7", '7', '\''),
        ("8", '8', '('),
        ("9", '9', ')'),
        ("0", '0', '~'),
        ("min", '-', '='),
        ("=", '^', '~'),
        ("¥", '\\', '|'),
        ("[", '@', '`'),
        ("]", '[', '{'),
        ("scln", ';', '+'),
        ("'", ':', '*'),
        ("\\", ']', '}'),
        ("comm", ',', '<'),
        (".", '.', '>'),
        ("/", '/', '?'),
        ("ro", '\\', '_'),
    ];

    /// What a key types on a JIS keyboard, without or with Shift.
    fn jis_char(kanata_key: &str, shift: bool) -> Option<char> {
        if let [c] = kanata_key.as_bytes() {
            if c.is_ascii_lowercase() {
                return Some(if shift { c.to_ascii_uppercase() } else { *c } as char);
            }
        }
        let &(_, plain, shifted) = JIS.iter().find(|(k, _, _)| *k == kanata_key)?;
        Some(if shift { shifted } else { plain })
    }

    /// Every printable ASCII character has a key on a JIS keyboard, and the key (with Shift if it says
    /// so) types that character.
    #[test]
    fn every_printable_ascii_character_has_its_jis_key() {
        for c in (0x21u8..0x7f).map(char::from) {
            let (key, shift) = jis_key(c).unwrap_or_else(|| panic!("no JIS key for {c:?}"));
            assert_eq!(jis_char(key, shift), Some(c), "{c:?}: key {key}, shift {shift}");
        }
        assert_eq!(jis_key(' '), None);
    }

    /// Romaji is typed as written: Shift once, before the characters that need it; those must come last,
    /// and a key cannot be used twice (all keys of a face are pressed at once).
    #[test]
    fn romaji_faces_are_typed_as_written() {
        assert_eq!(typed_keys("z*"), Ok(vec!["z", "lsft", "'"]));
        assert_eq!(typed_keys("z0"), Ok(vec!["z", "0"]));
        assert_eq!(typed_keys("x?!"), Ok(vec!["x", "lsft", "/", "1"]));
        assert_eq!(typed_keys("Ka"), Err("'a' comes after 'K', which needs Shift: the keys of a face are pressed at once, so the characters that need Shift must come last".into()));
        assert!(typed_keys("*z").unwrap_err().contains("must come last"));
        assert!(typed_keys("aA").unwrap_err().contains("each key can be used once"));
        assert!(typed_keys(":*").unwrap_err().contains("each key can be used once"));
        assert!(typed_keys("z a").unwrap_err().contains("cannot be typed"));
        assert!(typed_keys("z\t").unwrap_err().contains("cannot be typed"));
    }

    /// A face that would not be sent as written is an error, with its character codes (look-alikes).
    /// ゛ and ゜ of the standard layout give the key itself, as before (OQ-10).
    #[test]
    fn faces_that_cannot_be_sent_are_errors() {
        for face in ["［", "〔", "～", "ｚ＊", "が\u{3099}", "\u{304B}\u{3099}"] {
            let err = check(Encoder::Romaji, face).unwrap_err();
            assert!(err.contains("not in the romaji table") && err.contains("U+"), "{face}: {err}");
            assert!(check(Encoder::Unicode, face).is_ok(), "{face}");
        }
        assert!(check(Encoder::Romaji, "～").unwrap_err().contains("(U+FF5E)"));
        assert!(check(Encoder::Romaji, "\u{304B}\u{3099}").unwrap_err().contains("(U+304B U+3099)"));
        for face in ["か", "〜", "？", "「", "゛", "゜", "z*", "?"] {
            assert_eq!(check(Encoder::Romaji, face), Ok(()), "{face}");
        }
        assert!(check(Encoder::Romaji, "*z").is_err());
        assert!(check(Encoder::Unicode, "*z").is_err());
        // The Unicode encoder (tests only) sends characters, not romaji: such a face gives the key itself.
        assert!(!supports(Encoder::Unicode, "z*"));
    }

    /// The spellings of the romaji table can be typed (each key once, Shift last), and are those of
    /// Mozc's default romaji table (？｛｝（）: ASCII symbols, which Mozc makes full-width; ・ and ／: / is ・
    /// or ／ by its symbol style). Kana spelled with letters only are not listed here.
    #[test]
    fn romaji_table_has_mozc_spellings() {
        let mozc: &[(&str, &str)] = &[
            ("ー", "-"),
            ("。", "."),
            ("．", "."),
            ("、", ","),
            ("，", ","),
            ("・", "/"),
            ("…", "z."),
            ("‥", "z,"),
            ("『", "z["),
            ("』", "z]"),
            ("〜", "z-"),
            ("？", "?"),
            ("／", "/"),
            ("「", "["),
            ("」", "]"),
            ("｛", "{"),
            ("｝", "}"),
            ("（", "("),
            ("）", ")"),
        ];
        // Every hiragana, and the symbols of the Japanese punctuation, general punctuation and full-width blocks.
        let candidates = ('\u{3041}'..='\u{3096}')
            .chain('\u{3000}'..='\u{303F}')
            .chain('\u{2010}'..='\u{2027}')
            .chain('\u{FF01}'..='\u{FF5E}');
        let symbols: Vec<String> = candidates.chain("ー．，・→←↑↓".chars()).map(String::from).collect();
        for sym in &symbols {
            let sym = sym.as_str();
            let Some(spelling) = romaji(sym) else { continue };
            if let Err(e) = typed_keys(spelling) {
                panic!("{sym}: {spelling:?}: {e}");
            }
            if spelling.chars().all(|c| c.is_ascii_lowercase()) {
                continue;
            }
            let want = mozc.iter().find(|(s, _)| *s == sym).map(|(_, w)| *w);
            assert_eq!(Some(spelling), want, "{sym}");
        }
    }
}
