//! Test case notation shared by the kanata-nicola tests and those of the NICOLA engine (developed separately,
//! not yet public; docs/design.md §1.2).
//!
//! Input (whitespace-separated, times in ms):
//!
//! ```text
//! d:<KEY>@t  u:<KEY>@t  r:<KEY>@t   key down / up / auto-repeat. KEY is the evdev name without
//!                                   `KEY_`; OL / OR are the left / right thumb keys
//! end@t                             advance to time t without input
//! focus@t  modeoff@t  modeon@t  devremoved@t  reload@t  panic@t
//!                                   control events for the engine tests (kanata ignores them)
//! ```
//!
//! Expect (whitespace-separated tokens; `@t` also checks the output time):
//!
//! ```text
//! か  か(shift)  [OL]  [OR]  pass:d:ENTER
//! ```

use serde::Deserialize;

/// The case file of the thumb-prefix mode (ADR-18).
pub const KANATA_PREFIX_CASES: &str = include_str!("../../../testdata/kanata_prefix_cases.toml");

/// Physical keys used for the thumb keys in the test cases (shared type: left = Muhenkan, right = Henkan).
pub const THUMB_LEFT: &str = "KEY_MUHENKAN";
pub const THUMB_RIGHT: &str = "KEY_HENKAN";

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Action {
    Down,
    Up,
    /// Physical auto-repeat.
    Repeat,
}

/// Events that do not come from the keyboard (focus change, mode switch, ...).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Control {
    FocusLost,
    ModeOff,
    ModeOn,
    DeviceRemoved,
    ConfigReload,
    PanicReset,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Step {
    Key { t_ms: u64, action: Action, name: String },
    Control { t_ms: u64, event: Control },
    End { t_ms: u64 },
}

impl Step {
    pub fn t_ms(&self) -> u64 {
        match self {
            Step::Key { t_ms, .. } | Step::Control { t_ms, .. } | Step::End { t_ms } => *t_ms,
        }
    }
}

/// Expands `OL` / `OR` to the thumb keys and adds the `KEY_` prefix.
pub fn key_name(short: &str) -> String {
    match short {
        "OL" => THUMB_LEFT.to_string(),
        "OR" => THUMB_RIGHT.to_string(),
        s if s.starts_with("KEY_") => s.to_string(),
        s => format!("KEY_{s}"),
    }
}

pub fn parse_input(input: &str) -> Vec<Step> {
    input
        .split_whitespace()
        .map(|tok| {
            let (head, t) = tok.rsplit_once('@').unwrap_or_else(|| panic!("no @t: {tok}"));
            let t_ms: u64 = t.parse().unwrap_or_else(|_| panic!("bad time: {tok}"));
            if let Some((a, k)) = head.split_once(':') {
                let action = match a {
                    "d" => Action::Down,
                    "u" => Action::Up,
                    "r" => Action::Repeat,
                    _ => panic!("bad action: {tok}"),
                };
                return Step::Key { t_ms, action, name: key_name(k) };
            }
            let event = match head {
                "end" => return Step::End { t_ms },
                "focus" => Control::FocusLost,
                "modeoff" => Control::ModeOff,
                "modeon" => Control::ModeOn,
                "devremoved" => Control::DeviceRemoved,
                "reload" => Control::ConfigReload,
                "panic" => Control::PanicReset,
                _ => panic!("bad step: {tok}"),
            };
            Step::Control { t_ms, event }
        })
        .collect()
}

/// One expected output token. When `at_ms` is set, the output time is checked too.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Token {
    pub text: String,
    pub at_ms: Option<u64>,
}

pub fn parse_expect(expect: &str) -> Vec<Token> {
    expect
        .split_whitespace()
        .map(|tok| match tok.rsplit_once('@') {
            Some((text, t)) => Token { text: text.to_string(), at_ms: Some(t.parse().expect("bad time in expect")) },
            None => Token { text: tok.to_string(), at_ms: None },
        })
        .collect()
}

/// Compares expected tokens with actual outputs (text, time in ms).
pub fn compare(expected: &[Token], actual: &[(String, u64)]) -> Result<(), String> {
    compare_within(expected, actual, 0)
}

/// Like [`compare`], but an output time may differ from the expected one by up to `tolerance_ms`.
pub fn compare_within(expected: &[Token], actual: &[(String, u64)], tolerance_ms: u64) -> Result<(), String> {
    let ok = expected.len() == actual.len()
        && expected
            .iter()
            .zip(actual)
            .all(|(e, (text, t))| e.text == *text && e.at_ms.is_none_or(|at| at.abs_diff(*t) <= tolerance_ms));
    if ok {
        Ok(())
    } else {
        Err(format!("expected: {}\n  actual: {}", render_expected(expected), render_actual(actual)))
    }
}

fn render_expected(tokens: &[Token]) -> String {
    tokens
        .iter()
        .map(|t| match t.at_ms {
            Some(at) => format!("{}@{}", t.text, at),
            None => t.text.clone(),
        })
        .collect::<Vec<_>>()
        .join(" ")
}

pub fn render_actual(actual: &[(String, u64)]) -> String {
    actual.iter().map(|(s, t)| format!("{s}@{t}")).collect::<Vec<_>>().join(" ")
}

#[derive(Debug, Deserialize)]
struct File {
    case: Vec<Case>,
}

/// A case of the kanata case files.
#[derive(Clone, Debug, Deserialize)]
pub struct Case {
    pub id: String,
    pub desc: String,
    pub input: String,
    pub expect: String,
}

pub fn load(src: &str) -> Vec<Case> {
    toml::from_str::<File>(src).expect("valid case file").case
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn case_files_parse() {
        for case in load(KANATA_PREFIX_CASES) {
            parse_input(&case.input);
            parse_expect(&case.expect);
        }
    }

    #[test]
    fn notation() {
        assert_eq!(
            parse_input("d:OR@0 u:W@5 focus@7 end@9"),
            [
                Step::Key { t_ms: 0, action: Action::Down, name: "KEY_HENKAN".into() },
                Step::Key { t_ms: 5, action: Action::Up, name: "KEY_W".into() },
                Step::Control { t_ms: 7, event: Control::FocusLost },
                Step::End { t_ms: 9 },
            ]
        );
        let e = parse_expect("か@10 [OR]");
        assert!(compare(&e, &[("か".into(), 10), ("[OR]".into(), 99)]).is_ok());
        assert!(compare(&e, &[("か".into(), 11), ("[OR]".into(), 99)]).is_err());
        assert!(compare_within(&e, &[("か".into(), 13), ("[OR]".into(), 99)], 3).is_ok());
        assert!(compare_within(&e, &[("か".into(), 14), ("[OR]".into(), 99)], 3).is_err());
    }
}
