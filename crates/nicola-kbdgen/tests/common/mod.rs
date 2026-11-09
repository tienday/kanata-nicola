//! Running a generated .kbd in the kanata simulation (shared by the test files).
#![allow(dead_code)] // each test file uses part of it

use kanata_state_machine::oskbd::{KeyEvent, KeyValue};
use kanata_state_machine::{str_to_oscode, Kanata};
use nicola_kbdgen::kanata_key;
use nicola_testcases::{parse_input, Action, Step};

/// A layout overlay that puts symbols on character keys (D11 solo → and right 『, D12 right ←, D01 right …), to
/// apply after practical and lag-tolerant: the tests cover symbol faces where the standard layout has none.
pub const SYMBOLS_OVERLAY: &str = r#"
[meta]
name = "symbols"

[keys.KEY_LEFTBRACE]
solo = "→"
right = "『"

[keys.KEY_RIGHTBRACE]
right = "←"

[keys.KEY_Q]
right = "…"
"#;

/// kanata's parser and state are shared within the process, so parallel tests must not run it at the same time
/// (kanata's own simulation tests lock the same way).
static KANATA_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());

/// Loads a configuration in kanata, under the lock; kanata's error if it rejects it.
pub fn load(cfg: &str) -> Result<(), String> {
    let _lock = KANATA_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    Kanata::new_from_str(cfg, Default::default()).map(|_| ()).map_err(|e| format!("{e:?}"))
}

/// Raw simulation output (`out:↓K` etc.) with output times. Besides the tokens of the case files, the input can
/// have `layer:<name>@<t>`: a switch of the base layer, as nicola-imesync sends it to kanata (`ChangeLayer`).
pub fn simulate_raw(cfg: &str, input: &str) -> Vec<(String, u64)> {
    let _lock = KANATA_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    let mut k = Kanata::new_from_str(cfg, Default::default()).expect("cfg parses");
    let mut now = 0u64;
    let tick_to = |k: &mut Kanata, t: u64, now: &mut u64| {
        while *now < t {
            k.tick_ms(1, &None).unwrap();
            k.can_block_update_idle_waiting(1);
            *now += 1;
        }
    };
    for token in input.split_whitespace() {
        if let Some((layer, t)) = token.strip_prefix("layer:").and_then(|s| s.rsplit_once('@')) {
            tick_to(&mut k, t.parse().unwrap(), &mut now);
            // What kanata does for ChangeLayer.
            let i = k.layer_info.iter().position(|l| l.name == layer).unwrap_or_else(|| panic!("no layer {layer}"));
            k.layout.bm().set_default_layer(i);
            continue;
        }
        for step in parse_input(token) {
            tick_to(&mut k, step.t_ms(), &mut now);
            if let Step::Key { action, name, .. } = step {
                let kn = kanata_key(&name).unwrap_or_else(|| panic!("no kanata name {name}"));
                let value = match action {
                    Action::Down => KeyValue::Press,
                    Action::Up => KeyValue::Release,
                    Action::Repeat => KeyValue::Repeat,
                };
                k.handle_input_event(&KeyEvent { code: str_to_oscode(kn).unwrap(), value }).unwrap();
            }
        }
    }
    // Let the last event be processed (it happens within a tick)
    // and let any pending output finish.
    let end = now + 50;
    tick_to(&mut k, end, &mut now);
    let mut t = 0;
    let mut out = Vec::new();
    for e in &k.kbd_out.outputs.events {
        match e.strip_prefix("t:").and_then(|s| s.strip_suffix("ms")) {
            Some(ms) => t += ms.parse::<u64>().unwrap(),
            None => out.push((e.clone(), t)),
        }
    }
    out
}

/// Lists only the press events, as names like `K` or `CapsLock`.
pub fn presses(cfg: &str, input: &str) -> Vec<String> {
    pressed(&simulate_raw(cfg, input))
}

/// The press events of a simulation output, as names like `K` or `CapsLock`.
pub fn pressed(raw: &[(String, u64)]) -> Vec<String> {
    raw.iter().filter_map(|(e, _)| e.strip_prefix("out:↓").map(String::from)).collect()
}

/// The keys of a simulation output pressed while Shift (`LShift`) is held.
pub fn pressed_with_shift(raw: &[(String, u64)]) -> Vec<String> {
    let mut shift = false;
    let mut keys = Vec::new();
    for (e, _) in raw {
        match e.as_str() {
            "out:↓LShift" => shift = true,
            "out:↑LShift" => shift = false,
            e if shift => keys.extend(e.strip_prefix("out:↓").map(String::from)),
            _ => {}
        }
    }
    keys
}

/// Output keys still pressed at the end of the simulation output (none may remain: docs/design.md §2, stability).
pub fn stuck_keys(raw: &[(String, u64)]) -> Vec<String> {
    let mut held: Vec<&str> = Vec::new();
    for (e, _) in raw {
        if let Some(k) = e.strip_prefix("out:↓") {
            held.push(k);
        } else if let Some(k) = e.strip_prefix("out:↑") {
            if let Some(i) = held.iter().position(|h| *h == k) {
                held.remove(i);
            }
        }
    }
    held.into_iter().map(String::from).collect()
}

/// A random fast keystroke sequence over `keys` (names as in the case files), with Enter or Ctrl pressed
/// and released in between; every key is released at the end.
pub fn random_input(rnd: &mut impl FnMut(u64) -> u64, keys: &[&str]) -> String {
    let mut t = 0;
    let mut held: Vec<&str> = Vec::new();
    let mut input = Vec::new();
    for _ in 0..rnd(12) + 2 {
        t += rnd(60);
        let r = rnd(100);
        if !held.is_empty() && r < 45 {
            let k = held.swap_remove(rnd(held.len() as u64) as usize);
            input.push(format!("u:{k}@{t}"));
        } else if r < 50 {
            let k = if rnd(2) == 0 { "ENTER" } else { "LEFTCTRL" };
            input.push(format!("d:{k}@{t}"));
            t += rnd(20) + 1;
            input.push(format!("u:{k}@{t}"));
        } else {
            let k = keys[rnd(keys.len() as u64) as usize];
            if !held.contains(&k) {
                held.push(k);
                input.push(format!("d:{k}@{t}"));
            }
        }
    }
    for k in held {
        t += rnd(30) + 1;
        input.push(format!("u:{k}@{t}"));
    }
    input.join(" ")
}

/// xorshift64 with a fixed seed, so the random tests are deterministic.
pub fn rng(mut seed: u64) -> impl FnMut(u64) -> u64 {
    move |n| {
        seed ^= seed << 13;
        seed ^= seed >> 7;
        seed ^= seed << 17;
        seed % n
    }
}
