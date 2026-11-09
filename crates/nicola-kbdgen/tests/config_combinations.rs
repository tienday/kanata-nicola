//! The settings of the Linux scripts (`config.env`, docs/settings.md) in combination:
//!
//! - platform/linux/config.sh turns them into the arguments of the generator, kanata and nicola-imesync,
//! - the generator command makes a .kbd of those arguments that kanata reads,
//! - and the .kbd behaves as set (kanata simulation).
//!
//! Also: wrong values stop with the name of the setting, the warnings (unknown names, CapsLock as Ctrl on the desktop
//! but not in the settings), the kanata version check, the installation and the systemd units
//! (platform/linux/install-files.sh), what the units run (platform/linux/service.sh), the permissions helper
//! (platform/linux/permissions.sh), that the list of settings is the same everywhere, and the defaults that
//! docs/settings.md gives. Needs bash, so Linux only.
//!
//! The combinations (`combos`) are chosen so that each value of each setting appears in one of them, and so that
//! settings that act on each other are combined: IME sync with the keys treated as Ctrl and the overlays. A new
//! setting is spread over them the same way.
//!
//! Not covered here: whether kanata grabs the keyboards named (the E2E test, and by hand), reading the fcitx5 state
//! (nicola-imesync's own tests, and by hand), and registering or starting the units with systemctl (by hand: that
//! restarting kanata-nicola restarts nicola-imesync, or starts or stops it as the settings say).
#![cfg(target_os = "linux")]

mod common;

use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

use common::{presses, random_input, rng, simulate_raw, stuck_keys};

fn repo() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../..").canonicalize().unwrap()
}

/// A fresh directory for one case. `config/kanata-nicola/` in it is the settings directory; `bin/` in it comes first
/// in PATH (`bash`), with a gsettings that gives the desktop's keyboard options of TEST_XKB_OPTIONS (none if unset;
/// fails if `fail`), and a pgrep that finds a kanata if TEST_KANATA is `other` (process 1, which is root's) or `own`
/// (the caller), the process named by TEST_IME (`fcitx5` or `ibus-daemon`), and nothing else, so that the settings and
/// the processes of the machine running the tests do not count.
fn case_dir(name: &str) -> PathBuf {
    let d = Path::new(env!("CARGO_TARGET_TMPDIR")).join("config-combinations").join(name);
    let _ = std::fs::remove_dir_all(&d);
    std::fs::create_dir_all(settings_dir(&d)).unwrap();
    write_executable(
        &d.join("bin/gsettings"),
        "#!/bin/sh\n[ \"$*\" = 'get org.gnome.desktop.input-sources xkb-options' ] || exit 1\n\
         [ \"$TEST_XKB_OPTIONS\" = fail ] && echo 'No such schema' >&2 && exit 1\n\
         echo \"${TEST_XKB_OPTIONS:-@as []}\"\n",
    );
    write_executable(
        &d.join("bin/pgrep"),
        "#!/bin/sh\ncase \"$*:$TEST_KANATA\" in\n  '^kanata:other') echo 1; exit 0 ;;\n  '^kanata:own') echo $PPID; exit 0 ;;\n\
         esac\ncase \"$*\" in\n  *\"-x $TEST_IME\") [ -n \"$TEST_IME\" ] && echo 4242 && exit 0 ;;\nesac\nexit 1\n",
    );
    d
}

fn settings_dir(dir: &Path) -> PathBuf {
    dir.join("config/kanata-nicola")
}

fn write_executable(path: &Path, script: &str) {
    use std::os::unix::fs::PermissionsExt;
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    std::fs::write(path, script).unwrap();
    std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o755)).unwrap();
}

struct Out {
    ok: bool,
    code: Option<i32>,
    stdout: String,
    stderr: String,
}

/// Runs `script` with bash in `cwd` (`$1`... are `args`), with HOME and XDG_CONFIG_HOME in `dir`, `dir/bin` first
/// in PATH, and no other environment than PATH and `env`: neither the user's settings nor NICOLA_ variables around
/// are read.
fn bash(dir: &Path, cwd: &Path, script: &str, args: &[&str], env: &[(&str, &str)]) -> Out {
    let path = format!("{}:{}", dir.join("bin").display(), std::env::var("PATH").unwrap());
    let out = Command::new("bash")
        .args(["-c", script, "test"])
        .args(args)
        .current_dir(cwd)
        .env_clear()
        .env("PATH", path)
        .env("HOME", dir)
        .env("XDG_CONFIG_HOME", dir.join("config"))
        .envs(env.iter().copied())
        .output()
        .unwrap();
    Out {
        ok: out.status.success(),
        code: out.status.code(),
        stdout: String::from_utf8(out.stdout).unwrap(),
        stderr: String::from_utf8(out.stderr).unwrap(),
    }
}

/// What config.sh made of the settings.
#[derive(Debug, Default)]
struct Args {
    gen: Vec<String>,
    kanata: Vec<String>,
    ime_sync: bool,
    port: u16,
}

/// Runs config.sh with `config_env` as config.env (none if `None`). The arguments, or None if it stopped;
/// and what it printed to stderr.
fn config(dir: &Path, config_env: Option<&str>, env: &[(&str, &str)]) -> (Option<Args>, String) {
    if let Some(c) = config_env {
        std::fs::write(settings_dir(dir).join("config.env"), c).unwrap();
    }
    let script = r#"source platform/linux/config.sh
nicola_config || exit 3
for a in "${gen_args[@]}"; do printf 'gen\t%s\n' "$a"; done
for a in "${kanata_args[@]}"; do printf 'kanata\t%s\n' "$a"; done
printf 'ime_sync\t%s\nport\t%s\n' "$ime_sync" "$port""#;
    let out = bash(dir, &repo(), script, &[], env);
    if !out.ok {
        return (None, out.stderr);
    }
    let mut a = Args::default();
    for line in out.stdout.lines() {
        match line.split_once('\t').unwrap() {
            ("gen", v) => a.gen.push(v.into()),
            ("kanata", v) => a.kanata.push(v.into()),
            ("ime_sync", v) => a.ime_sync = v == "1",
            ("port", v) => a.port = v.parse().unwrap(),
            _ => panic!("unexpected output of config.sh: {line}"),
        }
    }
    (Some(a), out.stderr)
}

/// Runs the generator command with `args`: the .kbd, or the error message.
fn generate(dir: &Path, args: &[String]) -> Result<String, String> {
    let kbd = dir.join("nicola.kbd");
    let out = Command::new(env!("CARGO_BIN_EXE_nicola-kbdgen")).args(args).arg("-o").arg(&kbd).output().unwrap();
    if out.status.success() {
        Ok(std::fs::read_to_string(kbd).unwrap())
    } else {
        Err(String::from_utf8(out.stderr).unwrap())
    }
}

/// The generator arguments config.sh always gives.
fn base_args() -> Vec<String> {
    let args = ["--exclude-device", "nicola-e2e-input", "--exclude-device", "nicola-e2e-output"];
    args.iter().map(|s| s.to_string()).collect()
}

fn bundled(name: &str) -> String {
    format!("{}/layouts/overlays/{name}.toml", repo().display())
}

/// A small overlay of one's own: Q gives ゔ (v u).
const OWN_OVERLAY: &str = "[meta]\nname = \"own\"\n\n[keys.KEY_Q]\nsolo = \"ゔ\"\n";

/// A kanata file of the user's own (NICOLA_EXTRA_KBD): Pause gives the Menu key.
const EXTRA_KBD: &str = ";; mine\n(defoverrides\n  (pause) (menu)\n)\n";

/// kanata's library does not read include from a configuration given as a string: put the file in its place.
fn inline_includes(kbd: &str) -> String {
    kbd.lines()
        .map(|l| match l.strip_prefix("(include \"").and_then(|p| p.strip_suffix("\")")) {
            Some(path) => std::fs::read_to_string(path).unwrap_or_else(|e| panic!("{path}: {e}")),
            None => format!("{l}\n"),
        })
        .collect()
}

/// Checks of the bundled overlays and of the own one, once NICOLA is on.
/// practical: `,` gives っ. lag-tolerant: Henkan+`;` gives ん. The own one: Q gives ゔ.
fn practical() -> (&'static str, Vec<&'static str>) {
    ("d:COMMA@100 u:COMMA@130", vec!["X", "T", "U"])
}
fn lag_tolerant() -> (&'static str, Vec<&'static str>) {
    ("d:HENKAN@100 d:SEMICOLON@120 u:SEMICOLON@150 u:HENKAN@180", vec!["X", "N"])
}
fn own() -> (&'static str, Vec<&'static str>) {
    ("d:Q@100 u:Q@130", vec!["V", "U"])
}

/// A combination of settings, what config.sh should make of it, and how the result should behave.
struct Combo {
    name: &'static str,
    dir: PathBuf,
    config_env: String,
    /// Generator arguments after the ones config.sh always gives.
    gen: Vec<String>,
    kanata: Vec<&'static str>,
    port: u16,
    /// What config.sh prints to stderr (empty: nothing).
    stderr: &'static str,
    /// Lines the .kbd has.
    kbd: Vec<String>,
    ime_sync: bool,
    caps_is_ctrl: bool,
    /// Further checks, once NICOLA is on: input, expected presses.
    checks: Vec<(&'static str, Vec<&'static str>)>,
    /// Further checks from the start (NICOLA off).
    checks_from_start: Vec<(&'static str, Vec<&'static str>)>,
}

impl Combo {
    fn new(name: &'static str, config_env: &str) -> Self {
        let dir = case_dir(name);
        Combo {
            name,
            config_env: config_env.to_string(),
            gen: Vec::new(),
            // NICOLA_WAIT_DEVICE_MS unset: no wait with our patched kanata.
            kanata: vec!["--wait-device-ms", "0"],
            port: 23800,
            stderr: "",
            kbd: Vec::new(),
            ime_sync: false,
            caps_is_ctrl: false,
            checks: Vec::new(),
            checks_from_start: Vec::new(),
            dir,
        }
    }

    fn gen(mut self, args: &[&str]) -> Self {
        self.gen.extend(args.iter().map(|s| s.to_string()));
        self
    }

    fn kbd(mut self, lines: &[&str]) -> Self {
        self.kbd.extend(lines.iter().map(|s| s.to_string()));
        self
    }
}

const PREFIX: &str = ";; 判定方式: 親指先行";
const TOGGLE_IME_SYNC: &str = ";; 切り替え: キーでは切り替えない";

fn combos() -> Vec<Combo> {
    let mut v = Vec::new();

    v.push(Combo::new("default", "").kbd(&[PREFIX, ";; 切り替え: grv（半角/全角）で NICOLA の ON/OFF"]));

    let mut c = Combo::new(
        "ime-sync-service",
        "NICOLA_IME_SYNC=1\nNICOLA_KEYS_AS_CTRL=KEY_CAPSLOCK\nNICOLA_LAYOUT_OVERLAYS=practical:lag-tolerant:user-layout.toml\n\
         NICOLA_WAIT_DEVICE_MS=0\nNICOLA_EXCLUDE_DEVICES=\"Example Extra Buttons\"\n",
    );
    let local = settings_dir(&c.dir).join("user-layout.toml");
    std::fs::write(&local, OWN_OVERLAY).unwrap();
    c = c
        .gen(&["--exclude-device", "Example Extra Buttons"])
        .gen(&["--overlay", &bundled("practical"), "--overlay", &bundled("lag-tolerant")])
        .gen(&["--overlay", local.to_str().unwrap(), "--key-as-ctrl", "KEY_CAPSLOCK"])
        .gen(&["--toggle", "none"])
        .kbd(&[
            TOGGLE_IME_SYNC,
            ";; 差分:     practical, lag-tolerant, own",
            r#"linux-dev-names-exclude ("nicola-e2e-input" "nicola-e2e-output" "Example Extra Buttons")"#,
        ]);
    c.kanata = vec!["--wait-device-ms", "0", "-p", "127.0.0.1:23800"];
    c.ime_sync = true;
    c.caps_is_ctrl = true;
    c.checks = vec![practical(), lag_tolerant(), own()];
    v.push(c);

    let mut c = Combo::new("caps-as-ctrl", "NICOLA_KEYS_AS_CTRL=KEY_CAPSLOCK\nNICOLA_WAIT_DEVICE_MS=0\n")
        .gen(&["--key-as-ctrl", "KEY_CAPSLOCK"])
        .kbd(&[";; 切り替え: grv（半角/全角）で NICOLA の ON/OFF", "keys-as-ctrl (or (input real caps))"]);
    c.kanata = vec!["--wait-device-ms", "0"];
    c.caps_is_ctrl = true;
    v.push(c);

    let c = Combo::new("one-keyboard", "NICOLA_INCLUDE_DEVICES=\"AT Translated Set 2 keyboard\"\n")
        .gen(&["--include-device", "AT Translated Set 2 keyboard"])
        .kbd(&[r#"linux-dev-names-include ("AT Translated Set 2 keyboard")"#]);
    v.push(c);

    let mut c = Combo::new("ime-sync-port", "NICOLA_IME_SYNC=1\nNICOLA_KANATA_PORT=09999\nNICOLA_WAIT_DEVICE_MS=200\n")
        .gen(&["--toggle", "none"])
        .kbd(&[TOGGLE_IME_SYNC]);
    c.kanata = vec!["--wait-device-ms", "200", "-p", "127.0.0.1:9999"];
    c.port = 9999;
    c.ime_sync = true;
    // Ctrl+Muhenkan goes to the IME and leaves NICOLA as it is (nicola-imesync follows the IME).
    c.checks_from_start =
        vec![("d:LEFTCTRL@0 d:MUHENKAN@20 u:MUHENKAN@40 u:LEFTCTRL@60 d:W@100 u:W@130", vec!["LCtrl", "K0xAF", "W"])];
    v.push(c);

    let mut c = Combo::new(
        "ime-sync-keyboards",
        "NICOLA_IME_SYNC=1\nNICOLA_INCLUDE_DEVICES=\"Board A:Board B\"\nNICOLA_EXCLUDE_DEVICES=\"Ex [1]:My*Board\"\n",
    )
    .gen(&[
        "--include-device",
        "Board A",
        "--include-device",
        "Board B",
        "--exclude-device",
        "Ex [1]",
        "--exclude-device",
        "My*Board",
    ])
    .gen(&["--toggle", "none"])
    .kbd(&[
        TOGGLE_IME_SYNC,
        r#"linux-dev-names-include ("Board A" "Board B")"#,
        r#"linux-dev-names-exclude ("nicola-e2e-input" "nicola-e2e-output" "Ex [1]" "My*Board")"#,
    ]);
    c.kanata = vec!["--wait-device-ms", "0", "-p", "127.0.0.1:23800"];
    c.ime_sync = true;
    v.push(c);

    let mut c = Combo::new("overlay-path", "");
    let own_path = c.dir.join("my dir/o.toml");
    std::fs::create_dir_all(own_path.parent().unwrap()).unwrap();
    std::fs::write(&own_path, OWN_OVERLAY).unwrap();
    c.config_env = format!("NICOLA_LAYOUT_OVERLAYS=\"practical:{}\"\n", own_path.display());
    c = c
        .gen(&["--overlay", &bundled("practical"), "--overlay", own_path.to_str().unwrap()])
        .kbd(&[";; 差分:     practical, own"]);
    c.checks = vec![practical(), own()];
    v.push(c);

    // A kanata file of the user's own, read at the end: Pause gives the Menu key, with NICOLA on or off.
    let mut c = Combo::new("extra-kbd", "NICOLA_EXTRA_KBD=user-extra.kbd\n");
    let extra = settings_dir(&c.dir).join("user-extra.kbd");
    std::fs::write(&extra, EXTRA_KBD).unwrap();
    // The generator writes the path absolute (it is already), without symbolic links.
    let include = format!("(include \"{}\")", extra.canonicalize().unwrap().display());
    c = c.gen(&["--extra-kbd", extra.to_str().unwrap()]).kbd(&[";;   7. 追加の kanata の設定ファイル", &include]);
    c.checks = vec![("d:PAUSE@100 u:PAUSE@130", vec!["Application"])];
    c.checks_from_start = vec![("d:PAUSE@0 u:PAUSE@30", vec!["Application"])];
    v.push(c);

    // Space as the left thumb key, with IME sync (the recommended way): its face and its solo keystroke; Ctrl+Space
    // goes to the desktop and Muhenkan is a plain key. NICOLA off, Space is Space.
    let mut c = Combo::new("space-thumb", "NICOLA_IME_SYNC=1\nNICOLA_THUMB_KEYS=KEY_SPACE:KEY_HENKAN\n")
        .gen(&["--thumbs", "KEY_SPACE", "KEY_HENKAN", "--toggle", "none"])
        .kbd(&[TOGGLE_IME_SYNC, ";; 親指キー: 左 = spc（Space）、右 = henk（変換）"]);
    c.kanata = vec!["--wait-device-ms", "0", "-p", "127.0.0.1:23800"];
    c.ime_sync = true;
    c.checks = vec![
        ("d:SPACE@100 d:W@120 u:W@150 u:SPACE@180", vec!["E"]),
        ("d:SPACE@100 u:SPACE@150", vec!["Space"]),
        ("d:LEFTCTRL@100 d:SPACE@120 u:SPACE@150 u:LEFTCTRL@180", vec!["LCtrl", "Space"]),
        ("d:MUHENKAN@100 u:MUHENKAN@150", vec!["K0xAF"]),
    ];
    c.checks_from_start = vec![("d:SPACE@0 u:SPACE@30", vec!["Space"])];
    v.push(c);

    // Misspelt names, which are not settings, are warned about and change nothing: the thumb keys, the keyboards and the
    // kanata stay the defaults.
    let mut c = Combo::new(
        "unknown-settings",
        "NICOLA_THUMB_KEY=KEY_SPACE:KEY_HENKAN\nNICOLA_INCLUDE_DEVICE=\"AT Translated Set 2 keyboard\"\n\
         NICOLA_KANATA_PATH=/no/kanata\n",
    )
    .kbd(&[PREFIX]);
    c.stderr = "NICOLA_THUMB_KEY is not a setting";
    v.push(c);

    v
}

/// The input that turns NICOLA on, and what it outputs.
fn on(c: &Combo) -> (&'static str, Vec<&'static str>) {
    if c.ime_sync {
        ("layer:nicola@0", vec![])
    } else {
        ("d:GRAVE@0 u:GRAVE@10", vec!["Grave"])
    }
}

/// Checks the presses for `input`, after turning NICOLA on (`from_start`: as it starts, with NICOLA off).
fn check(c: &Combo, kbd: &str, from_start: bool, input: &str, want: &[&str]) {
    let (input, want) = if from_start {
        (input.to_string(), want.to_vec())
    } else {
        let (on_input, on_output) = on(c);
        (format!("{on_input} {input}"), [on_output, want.to_vec()].concat())
    };
    assert_eq!(presses(kbd, &input), want, "{}: {input}", c.name);
}

fn behaves_as_set(c: &Combo, kbd: &str) {
    // config.sh never starts with NICOLA on.
    check(c, kbd, true, "d:W@0 u:W@30", &["W"]);
    check(c, kbd, false, "d:W@100 u:W@130", &["K", "A"]);
    check(c, kbd, false, "d:HENKAN@100 d:W@120 u:W@150 u:HENKAN@180", &["G", "A"]);
    // A character key first: no chord (the solo face, and the thumb's own keystroke).
    check(c, kbd, false, "d:W@100 d:HENKAN@120 u:W@150 u:HENKAN@170", &["K", "A", "K0xB0"]);
    check(c, kbd, false, "d:HENKAN@100 u:HENKAN@150", &["K0xB0"]);
    check(c, kbd, false, "d:LEFTCTRL@100 d:W@120 u:W@150 u:LEFTCTRL@180", &["LCtrl", "W"]);
    let caps: &[&str] = if c.caps_is_ctrl { &["CapsLock", "W"] } else { &["CapsLock", "K", "A"] };
    check(c, kbd, false, "d:CAPSLOCK@100 d:W@120 u:W@150 u:CAPSLOCK@180", caps);
    // Hankaku/Zenkaku switches NICOLA, except with IME sync (it only goes to the IME).
    let grave: &[&str] = if c.ime_sync { &["Grave", "K", "A"] } else { &["Grave", "W"] };
    check(c, kbd, false, "d:GRAVE@100 u:GRAVE@110 d:W@150 u:W@180", grave);
    // A thumb key held long, then characters: all of them on its face (continuous shift).
    check(c, kbd, false, "d:HENKAN@100 d:W@250 u:W@280 d:E@300 u:E@330 u:HENKAN@350", &["G", "A", "D", "A"]);
    if c.ime_sync {
        let input = "layer:nicola@0 d:W@20 u:W@40 layer:base@60 d:W@80 u:W@100";
        check(c, kbd, true, input, &["K", "A", "W"]);
    }
    for (input, want) in &c.checks {
        check(c, kbd, false, input, want);
    }
    for (input, want) in &c.checks_from_start {
        check(c, kbd, true, input, want);
    }
    // Random typing with modifiers, Enter and the toggles leaves no output key pressed.
    let keys = [
        "W",
        "E",
        "R",
        "A",
        "S",
        "K",
        "L",
        "SEMICOLON",
        "COMMA",
        "MUHENKAN",
        "HENKAN",
        "MUHENKAN",
        "HENKAN",
        "SPACE",
        "LEFTSHIFT",
        "CAPSLOCK",
        "GRAVE",
        "KATAKANAHIRAGANA",
    ];
    let mut rnd = rng(0x0C0F_F1C5_2026_1027);
    for i in 0..10 {
        let input = format!("{} {}", on(c).0, random_input(&mut rnd, &keys));
        let stuck = stuck_keys(&simulate_raw(kbd, &input));
        assert!(stuck.is_empty(), "{} random case {i}: {input}\n  output keys left pressed: {stuck:?}", c.name);
    }
}

#[test]
fn combinations() {
    let mut kbds = Vec::new();
    for c in combos() {
        let (args, stderr) = config(&c.dir, Some(&c.config_env), &[]);
        let args = args.unwrap_or_else(|| panic!("{}: config.sh stopped: {stderr}", c.name));
        let want: Vec<String> = base_args().into_iter().chain(c.gen.clone()).collect();
        assert_eq!(args.gen, want, "{}: generator arguments", c.name);
        assert_eq!(args.kanata, c.kanata, "{}: kanata arguments", c.name);
        assert_eq!((args.ime_sync, args.port), (c.ime_sync, c.port), "{}", c.name);
        if c.stderr.is_empty() {
            assert!(stderr.is_empty(), "{}: config.sh printed: {stderr}", c.name);
        } else {
            assert!(stderr.contains(c.stderr), "{}: config.sh printed: {stderr}", c.name);
        }

        let kbd = generate(&c.dir, &args.gen).unwrap_or_else(|e| panic!("{}: generator failed: {e}", c.name));
        for line in &c.kbd {
            assert!(kbd.contains(line.as_str()), "{}: the .kbd has no line {line}", c.name);
        }
        let kbd = inline_includes(&kbd);
        // kanata reads it (simulate_raw stops otherwise).
        assert!(simulate_raw(&kbd, "").is_empty(), "{}: output with no input", c.name);

        behaves_as_set(&c, &kbd);
        kbds.push((c.name, kbd));
    }
    let kbd = |name: &str| &kbds.iter().find(|(n, _)| *n == name).unwrap().1;
    assert_eq!(kbd("unknown-settings"), kbd("default"), "misspelt settings change the .kbd");
}

#[test]
fn environment_and_missing_config_env() {
    // Settings in config.env win; the environment gives the rest.
    let dir = case_dir("environment");
    let env = [("NICOLA_IME_SYNC", "1"), ("NICOLA_KEYS_AS_CTRL", "KEY_CAPSLOCK")];
    let (args, _) = config(&dir, Some("NICOLA_IME_SYNC=0\n"), &env);
    let args = args.unwrap();
    assert!(!args.ime_sync);
    assert_eq!(args.gen, [base_args(), vec!["--key-as-ctrl".into(), "KEY_CAPSLOCK".into()]].concat());

    // No config.env: the defaults (no wait before a rescan, with our patched kanata).
    let dir = case_dir("no-config-env");
    let (args, stderr) = config(&dir, None, &[]);
    let args = args.unwrap();
    assert_eq!(args.gen, base_args());
    assert_eq!(args.kanata, ["--wait-device-ms", "0"]);
    assert!(!args.ime_sync && args.port == 23800 && stderr.is_empty(), "{args:?} {stderr}");

    // Another kanata (NICOLA_KANATA_BIN): kanata's own wait, unless NICOLA_WAIT_DEVICE_MS is set.
    let dir = case_dir("other-kanata");
    let other = dir.join("other-kanata");
    write_executable(&other, "#!/bin/sh\necho 'kanata 1.11.0'\n");
    let config_env = format!("NICOLA_KANATA_BIN={}\n", other.display());
    assert!(config(&dir, Some(&config_env), &[]).0.unwrap().kanata.is_empty());
    let config_env = format!("{config_env}NICOLA_WAIT_DEVICE_MS=50\n");
    assert_eq!(config(&dir, Some(&config_env), &[]).0.unwrap().kanata, ["--wait-device-ms", "50"]);
}

#[test]
fn wrong_values_stop_with_the_name_of_the_setting() {
    // Stopped by config.sh.
    let cases = [
        ("NICOLA_IME_SYNC=2", "NICOLA_IME_SYNC"),
        ("NICOLA_WAIT_DEVICE_MS=abc", "NICOLA_WAIT_DEVICE_MS"),
        ("NICOLA_KANATA_PORT=0", "NICOLA_KANATA_PORT"),
        ("NICOLA_KANATA_PORT=65536", "NICOLA_KANATA_PORT"),
        ("NICOLA_KANATA_PORT=abc", "NICOLA_KANATA_PORT"),
        // config.env that does not run as written does not leave settings at their defaults.
        ("NICOLA_IME_SYNC=1\nNICOLA_KEYS_AS_CTRL = KEY_CAPSLOCK", "NICOLA_KEYS_AS_CTRL: command not found"),
        ("NICOLA_IME_SYNC=1\nif then", "syntax error"),
        // A variable that is not set (XDG_CONFIG_HOME often is not): the scripts run with set -u.
        ("NICOLA_EXTRA_KBD=\"$UNSET_IN_TEST/user-extra.kbd\"", "UNSET_IN_TEST: unbound variable"),
        ("NICOLA_THUMB_KEYS=KEY_SPACE", "NICOLA_THUMB_KEYS"),
        ("NICOLA_THUMB_KEYS=KEY_SPACE:KEY_HENKAN:KEY_MUHENKAN", "NICOLA_THUMB_KEYS"),
        ("NICOLA_THUMB_KEYS=:KEY_HENKAN", "NICOLA_THUMB_KEYS"),
    ];
    for (i, (config_env, message)) in cases.iter().enumerate() {
        let dir = case_dir(&format!("wrong-{i}"));
        let (args, stderr) = config(&dir, Some(config_env), &[]);
        assert!(args.is_none(), "{config_env:?} did not stop");
        assert!(stderr.contains(message), "{config_env:?}: {stderr}");
    }

    // Stopped by the generator.
    let caps_face = "[keys.KEY_CAPSLOCK]\nsolo = \"あ\"\n";
    let cases = [
        ("NICOLA_KEYS_AS_CTRL=KEY_NOPE", "no kanata name for KEY_NOPE"),
        // A character key: the message names the option, not only the layout.
        ("NICOLA_KEYS_AS_CTRL=KEY_A", "--key-as-ctrl"),
        ("NICOLA_INCLUDE_DEVICES='My \"Board\"'", "double quote"),
        ("NICOLA_LAYOUT_OVERLAYS=nope", "nope.toml"),
        ("NICOLA_KEYS_AS_CTRL=KEY_CAPSLOCK\nNICOLA_LAYOUT_OVERLAYS=caps.toml", "--key-as-ctrl"),
        // kanata would ignore an include that does not exist.
        ("NICOLA_EXTRA_KBD=nope.kbd", "nope.kbd: No such file"),
        ("NICOLA_THUMB_KEYS=KEY_SPACE:KEY_NOPE", "no kanata name for KEY_NOPE"),
        // While a modifier is held, the character keys pass through; the toggle key cannot be a thumb key too.
        ("NICOLA_THUMB_KEYS=KEY_LEFTALT:KEY_HENKAN", "cannot be the left thumb key (--thumbs)"),
        ("NICOLA_THUMB_KEYS=KEY_GRAVE:KEY_HENKAN", "--toggle"),
    ];
    for (i, (config_env, message)) in cases.iter().enumerate() {
        let dir = case_dir(&format!("wrong-gen-{i}"));
        std::fs::write(settings_dir(&dir).join("caps.toml"), caps_face).unwrap();
        let (args, stderr) = config(&dir, Some(config_env), &[]);
        let args = args.unwrap_or_else(|| panic!("{config_env:?}: config.sh stopped: {stderr}"));
        let err = generate(&dir, &args.gen).expect_err(config_env);
        assert!(err.contains(message), "{config_env:?}: {err}");
    }
}

#[test]
fn unknown_names_are_warned_about() {
    for (i, config_env) in ["NICOLA_IME_SYNK=1", "export NICOLA_IME_SYNK=1"].iter().enumerate() {
        let dir = case_dir(&format!("unknown-{i}"));
        let (args, stderr) = config(&dir, Some(config_env), &[]);
        assert!(args.is_some(), "{config_env:?} stopped: {stderr}");
        assert!(stderr.contains("NICOLA_IME_SYNK is not a setting"), "{config_env:?}: {stderr}");
    }
}

#[test]
fn capslock_as_ctrl_without_the_setting_is_warned_about() {
    // The desktop's keyboard options (fake gsettings, `case_dir`), config.env, whether it warns.
    let cases = [
        ("['ctrl:nocaps']", "", true),
        ("['compose:ralt', 'ctrl:swapcaps']", "NICOLA_IME_SYNC=1", true),
        ("['ctrl:swapcaps_hyper']", "", true),
        ("['caps:ctrl_modifier']", "", true),
        ("['ctrl:nocaps']", "NICOLA_KEYS_AS_CTRL=KEY_CAPSLOCK", false),
        ("['ctrl:nocaps']", "NICOLA_KEYS_AS_CTRL=KEY_RIGHTALT:KEY_CAPSLOCK", false),
        ("['ctrl:nocaps']", "NICOLA_KEYS_AS_CTRL=KEY_CAPSLOCKX", true),
        ("['caps:escape']", "", false),
        ("@as []", "", false),
        // No GNOME settings to read.
        ("fail", "", false),
    ];
    for (i, (options, config_env, warns)) in cases.iter().enumerate() {
        let dir = case_dir(&format!("caps-ctrl-{i}"));
        let (args, stderr) = config(&dir, Some(config_env), &[("TEST_XKB_OPTIONS", options)]);
        assert!(args.is_some(), "{options} {config_env:?} stopped: {stderr}");
        let warned = stderr.contains("CapsLock is Ctrl in the desktop's keyboard settings")
            && stderr.contains(options)
            && stderr.contains("NICOLA_KEYS_AS_CTRL=KEY_CAPSLOCK");
        assert_eq!(warned, *warns, "{options} {config_env:?}: {stderr}");
        assert!(*warns || stderr.is_empty(), "{options} {config_env:?}: {stderr}");
    }
}

#[test]
fn kanata_version_check() {
    let dir = case_dir("kanata-version");
    let script = r#"source platform/linux/config.sh
bin=$(nicola_kanata_bin) || exit 4
echo "bin=$bin"
nicola_check_kanata_version "$bin" && echo current
exit 0"#;
    let run = |cwd: &Path, kanata: Option<&Path>| {
        let env: Vec<(&str, &str)> = kanata.map(|k| ("NICOLA_KANATA_BIN", k.to_str().unwrap())).into_iter().collect();
        bash(&dir, cwd, script, &[], &env)
    };
    let stub = |name: &str, output: &str| {
        let path = dir.join(name);
        write_executable(&path, &format!("#!/bin/sh\necho '{output}'\n"));
        path
    };

    // Without the upstream fix: both warnings.
    let out = run(&repo(), Some(&stub("kanata-old", "kanata 1.11.0")));
    assert!(out.ok && !out.stdout.contains("current"), "{}", out.stderr);
    assert!(
        out.stderr.contains("lacks the upstream fix") && out.stderr.contains("not our patched build"),
        "{}",
        out.stderr
    );
    // A release with the upstream fix, but not our build.
    let out = run(&repo(), Some(&stub("kanata-new", "kanata 1.13.0")));
    assert!(
        !out.stderr.contains("lacks the upstream fix") && out.stderr.contains("not our patched build"),
        "{}",
        out.stderr
    );
    let out = run(&repo(), Some(&stub("kanata-mute", "")));
    assert!(out.stderr.contains("cannot read the version"), "{}", out.stderr);
    // Not an executable: stops with the name of the setting.
    let out = run(&repo(), Some(&dir.join("nonexistent")));
    assert!(!out.ok && out.stderr.contains("NICOLA_KANATA_BIN is not an executable"), "{}", out.stderr);

    // Our patched build, in a copy of the scripts: used when its stamp matches the current patches.
    let copy = dir.join("repo");
    let kanata_dir = repo().join("platform/linux/kanata");
    for entry in std::fs::read_dir(kanata_dir.join("patches")).unwrap() {
        let entry = entry.unwrap();
        let to = copy.join("platform/linux/kanata/patches").join(entry.file_name());
        std::fs::create_dir_all(to.parent().unwrap()).unwrap();
        std::fs::copy(entry.path(), to).unwrap();
    }
    std::fs::copy(kanata_dir.join("build.sh"), copy.join("platform/linux/kanata/build.sh")).unwrap();
    std::fs::copy(repo().join("platform/linux/config.sh"), copy.join("platform/linux/config.sh")).unwrap();
    let built = copy.join("target/kanata-patched/kanata");
    write_executable(&built, "#!/bin/sh\necho 'kanata 1.11.0'\n");
    let stamp = bash(&dir, &copy, "platform/linux/kanata/build.sh --stamp", &[], &[]);
    assert!(stamp.ok, "{}", stamp.stderr);
    std::fs::write(copy.join("target/kanata-patched/kanata.stamp"), &stamp.stdout).unwrap();
    let out = run(&copy, None);
    assert!(out.stdout.contains(&format!("bin={}", built.display())), "{}", out.stdout);
    assert!(out.stdout.contains("current") && out.stderr.is_empty(), "{}", out.stderr);
    // Built from other patches: a warning to rebuild.
    std::fs::write(copy.join("target/kanata-patched/kanata.stamp"), "kanata 1.11.0 old\n").unwrap();
    let out = run(&copy, None);
    assert!(!out.stdout.contains("current") && out.stderr.contains("built from other patches"), "{}", out.stderr);
}

/// Splits a command line of a unit (`key` is ExecStart or ExecCondition) as systemd does for what install-files.sh
/// writes: words in double quotes with \\ and \" escaped, %% for %, and in the arguments (not the command) $$ for $.
fn exec_line(unit: &str, key: &str) -> Vec<String> {
    let line = unit.lines().find_map(|l| l.strip_prefix(key)?.strip_prefix('=')).unwrap();
    let mut words = Vec::new();
    let mut chars = line.chars();
    loop {
        match chars.next() {
            None => break,
            Some(' ') => continue,
            Some('"') => {}
            Some(c) => panic!("unquoted {c:?} in {line}"),
        }
        let mut w = String::new();
        loop {
            match chars.next().unwrap() {
                '"' => break,
                '\\' => w.push(chars.next().unwrap()),
                c => w.push(c),
            }
        }
        let w = w.replace("%%", "%");
        words.push(if words.is_empty() { w } else { w.replace("$$", "$") });
    }
    words
}

/// The URI install-files.sh writes for a path: file:// and the bytes other than [A-Za-z0-9/._~-] percent-encoded,
/// % as %%.
fn doc_uri(path: &str) -> String {
    let plain = |b: u8| b.is_ascii_alphanumeric() || b"/._~-".contains(&b);
    let encoded: String =
        path.bytes().map(|b| if plain(b) { (b as char).to_string() } else { format!("%%{b:02X}") }).collect();
    format!("file://{encoded}")
}

#[test]
fn units() {
    let dir = case_dir("units");
    // Paths with what needs care: a space, $, %, non-ASCII.
    let odd = dir.join("a b$c%dにほんご");
    let (inst, settings) = (odd.join("share"), odd.join("config"));
    let service = inst.join("service.sh");
    write_executable(&service, "#!/bin/sh\n");
    let (inst, settings, service, repo_path) =
        (inst.to_str().unwrap(), settings.to_str().unwrap(), service.to_str().unwrap(), odd.to_str().unwrap());
    let unit = |function: &str, args: &[&str]| {
        bash(&dir, &repo(), &format!("source platform/linux/install-files.sh\n{function} \"$@\""), args, &[])
    };
    let has = |unit: &str, line: &str| unit.lines().any(|l| l == line);

    let out = unit("nicola_unit_kanata", &[repo_path, inst, settings]);
    assert!(out.ok, "{}", out.stderr);
    let kanata_unit = out.stdout;
    assert_eq!(exec_line(&kanata_unit, "ExecStart"), [service, "kanata", settings]);
    let doc = format!("Documentation={}", doc_uri(&format!("{repo_path}/docs/usage.md")));
    assert!(has(&kanata_unit, &doc), "{kanata_unit}");
    // nicola-imesync is started with it; it starts with the graphical session only (not with default.target, which
    // the user manager starts at an ssh login and with lingering at boot, when kanata would take the keyboards from
    // the login screen and other users) and stops at its end; no restart after a mistake in the settings (service.sh
    // exits with 78); the log lines are named after kanata, not service.sh; enabling or disabling it does the same to
    // nicola-imesync (which would otherwise start it at login).
    assert!(!kanata_unit.lines().any(|l| !l.starts_with('#') && l.contains("default.target")), "{kanata_unit}");
    for line in [
        "Wants=kanata-nicola-imesync.service",
        "PartOf=graphical-session.target",
        "WantedBy=graphical-session.target",
        "Also=kanata-nicola-imesync.service",
        "RuntimeDirectory=kanata-nicola",
        "RestartPreventExitStatus=78",
        "SyslogIdentifier=kanata",
    ] {
        assert!(has(&kanata_unit, line), "no {line}:\n{kanata_unit}");
    }

    let out = unit("nicola_unit_imesync", &[repo_path, inst, settings]);
    assert!(out.ok, "{}", out.stderr);
    let imesync_unit = out.stdout;
    assert_eq!(exec_line(&imesync_unit, "ExecCondition"), [service, "ime-sync-on", settings]);
    assert_eq!(exec_line(&imesync_unit, "ExecStart"), [service, "imesync", settings]);
    // Runs only with its own kanata (stopped and restarted with kanata-nicola, stopped when it stops or fails), and in
    // the graphical session. Restart=always would restart it again and again when ExecCondition= says no (systemd 249).
    for line in [
        "BindsTo=kanata-nicola.service",
        "PartOf=graphical-session.target",
        "Restart=on-failure",
        "SyslogIdentifier=nicola-imesync",
    ] {
        assert!(has(&imesync_unit, line), "no {line}:\n{imesync_unit}");
    }

    // systemd refuses " and \ in a command path, however quoted: stop instead of writing a broken unit.
    for name in ["q\"uote", "back\\slash"] {
        let path = dir.join(name);
        for function in ["nicola_unit_kanata", "nicola_unit_imesync"] {
            let out = unit(function, &[repo_path, path.to_str().unwrap(), settings]);
            assert!(!out.ok && out.stderr.contains("systemd cannot run"), "{name} {function}: {}", out.stderr);
        }
    }

    // systemd itself reads them, where it can be asked to.
    let verify = |name: &str, text: &str| {
        let path = dir.join(name);
        std::fs::write(&path, text).unwrap();
        Command::new("systemd-analyze").args(["--user", "verify"]).arg(&path).output()
    };
    match verify("probe.service", "[Service]\nExecStart=/bin/true\n") {
        Ok(o) if o.status.success() => {
            for (name, text) in
                [("kanata-nicola.service", &kanata_unit), ("kanata-nicola-imesync.service", &imesync_unit)]
            {
                let o = verify(name, text).unwrap();
                assert!(o.status.success(), "{name}: {}", String::from_utf8_lossy(&o.stderr));
            }
        }
        _ => eprintln!("skipped: systemd-analyze --user verify is not available here"),
    }
}

/// install.sh's note on the way of switching that suits the input method running (config.sh nicola_note_ime): IME
/// sync with fcitx5, the key toggle with ibus; nothing when they already match or no input method runs.
#[test]
fn ime_note() {
    let dir = case_dir("ime-note");
    let note = |ime_sync: &str, running: &str| {
        let script = "source platform/linux/config.sh\nime_sync=$1\nnicola_note_ime";
        let out = bash(&dir, &repo(), script, &[ime_sync], &[("TEST_IME", running)]);
        assert!(out.ok && out.stdout.is_empty(), "{ime_sync} {running}: {}", out.stderr);
        out.stderr
    };
    assert!(note("0", "fcitx5").contains("NICOLA_IME_SYNC=1 is recommended"));
    assert!(note("1", "ibus-daemon").contains("NICOLA_IME_SYNC=0"));
    for (ime_sync, running) in [("1", "fcitx5"), ("0", "ibus-daemon"), ("0", ""), ("1", "")] {
        assert_eq!(note(ime_sync, running), "", "{ime_sync} {running}");
    }
}

/// install.sh's check of the build tools (config.sh nicola_check_build_tools), with only stand-ins in PATH: each one
/// missing in turn, and pkg-config without the D-Bus development files (TEST_NO_DBUS).
#[test]
fn build_tools_check() {
    let dir = case_dir("build-tools");
    let check = |have: &[&str], env: &[(&str, &str)]| {
        let bin = dir.join("tools");
        let _ = std::fs::remove_dir_all(&bin);
        std::fs::create_dir_all(&bin).unwrap();
        for tool in have {
            let script = match *tool {
                "pkg-config" => "#!/bin/sh\n[ \"$*\" = '--exists dbus-1' ] && [ -z \"$TEST_NO_DBUS\" ]\n",
                _ => "#!/bin/sh\n",
            };
            write_executable(&bin.join(tool), script);
        }
        let out = Command::new("/bin/bash")
            .args(["-c", "source platform/linux/config.sh\nnicola_check_build_tools"])
            .current_dir(repo())
            .env_clear()
            .env("PATH", &bin)
            .envs(env.iter().copied())
            .output()
            .unwrap();
        (out.status.success(), String::from_utf8(out.stderr).unwrap())
    };
    let all = ["cargo", "cc", "pkg-config", "curl", "patch"];
    assert_eq!(check(&all, &[]), (true, String::new()));
    for tool in all {
        let have: Vec<&str> = all.into_iter().filter(|t| *t != tool).collect();
        let (ok, stderr) = check(&have, &[]);
        assert!(
            !ok && stderr.contains(&format!("  - {tool}")) && stderr.contains("sudo apt install"),
            "{tool}: {stderr}"
        );
    }
    let (ok, stderr) = check(&all, &[("TEST_NO_DBUS", "1")]);
    assert!(!ok && stderr.contains("D-Bus development files"), "{stderr}");
}

/// What the units run: platform/linux/service.sh in an installation made by install-files.sh, with stand-ins for
/// kanata and nicola-imesync that print how they were run.
#[test]
fn service() {
    let dir = case_dir("service");
    let bin = dir.join("bin");
    std::fs::create_dir_all(&bin).unwrap();
    std::fs::copy(env!("CARGO_BIN_EXE_nicola-kbdgen"), bin.join("nicola-kbdgen")).unwrap();
    write_executable(&bin.join("nicola-imesync"), "#!/bin/sh\necho nicola-imesync \"$@\"\n");
    // kanata --check fails while this file exists.
    let check_fails = dir.join("check-fails");
    let kanata = |name: &str| {
        let path = dir.join(name);
        let script = format!(
            "#!/bin/sh\nif [ \"$1\" = --check ]; then\n  [ -e '{}' ] && echo 'error in the configuration' && exit 1\n  \
             exit 0\nfi\necho {name} \"$@\"\n",
            check_fails.display()
        );
        write_executable(&path, &script);
        path
    };
    let inst = dir.join("share/kanata-nicola");
    let install = "source platform/linux/install-files.sh\nnicola_install_files \"$PWD\" \"$@\"";
    let installed_kanata = kanata("kanata");
    let out = bash(&dir, &repo(), install, &[&bin, &inst, &installed_kanata].map(|p| p.to_str().unwrap()), &[]);
    assert!(out.ok, "{}", out.stderr);

    let run = dir.join("run");
    std::fs::create_dir_all(&run).unwrap();
    let kbd = run.join("nicola.kbd");
    let settings = settings_dir(&dir);
    let script = inst.join("service.sh");
    let service = |command: &str, env: &[(&str, &str)]| {
        let mut env = env.to_vec();
        env.push(("RUNTIME_DIRECTORY", run.to_str().unwrap()));
        // From another directory: it finds the installation by its own path.
        let args = [script.to_str().unwrap(), command, settings.to_str().unwrap()];
        bash(&dir, &dir, "exec \"$@\"", &args, &env)
    };
    let write_config = |text: &str| std::fs::write(settings.join("config.env"), text).unwrap();
    // How kanata was run (the last line), and the configuration it was given.
    let ran = |out: &Out| out.stdout.lines().last().unwrap_or("").to_string();
    let generated = |config_env: Option<&str>| {
        let (args, stderr) = config(&dir, config_env, &[]);
        generate(&dir, &args.unwrap_or_else(|| panic!("config.sh stopped: {stderr}")).gen).unwrap()
    };

    // No config.env: the defaults, with the kanata of the installation.
    let out = service("kanata", &[]);
    assert!(out.ok, "{}", out.stderr);
    assert!(out.stdout.starts_with("settings: none"), "{}", out.stdout);
    assert_eq!(ran(&out), format!("kanata -c {} --wait-device-ms 0 --no-wait", kbd.display()));
    assert_eq!(std::fs::read_to_string(&kbd).unwrap(), generated(None));
    assert_eq!(service("ime-sync-on", &[]).code, Some(1));

    // IME sync, and an overlay of each kind. NICOLA_ variables of the environment do not count.
    std::fs::write(settings.join("own.toml"), OWN_OVERLAY).unwrap();
    let config_env = "NICOLA_IME_SYNC=1\nNICOLA_KANATA_PORT=49501\nNICOLA_WAIT_DEVICE_MS=0\n\
                      NICOLA_LAYOUT_OVERLAYS=practical:own.toml\n";
    write_config(config_env);
    let out = service("kanata", &[("NICOLA_KEYS_AS_CTRL", "KEY_CAPSLOCK"), ("NICOLA_IME_SYNC", "0")]);
    assert!(out.ok, "{}", out.stderr);
    assert_eq!(ran(&out), format!("kanata -c {} --wait-device-ms 0 -p 127.0.0.1:49501 --no-wait", kbd.display()));
    assert_eq!(std::fs::read_to_string(&kbd).unwrap(), generated(Some(config_env)));
    assert_eq!(service("ime-sync-on", &[("NICOLA_IME_SYNC", "0")]).code, Some(0));
    assert_eq!(service("imesync", &[("NICOLA_KANATA_PORT", "1")]).stdout, "nicola-imesync --port 49501\n");

    // check: the same, without running kanata.
    let out = service("check", &[]);
    assert!(out.ok && out.stdout.lines().count() == 1, "{}{}", out.stdout, out.stderr);

    // Another kanata running (the unit's own has exited by then): 78 (the unit does not restart), as this one could not
    // take the keyboards. Another user's is named, as after switching users without logging out. check does not mind.
    let uid_of = |p: &str| {
        let out = Command::new("ps").args(["-o", "uid=", "-p", p]).output().unwrap();
        String::from_utf8(out.stdout).unwrap().trim().to_string()
    };
    if uid_of("1") != uid_of(&std::process::id().to_string()) {
        let out = service("kanata", &[("TEST_KANATA", "other")]);
        assert_eq!(out.code, Some(78), "{}", out.stderr);
        assert!(out.stderr.contains("kanata of another user (root)") && !out.stdout.contains(" -c "), "{}", out.stderr);
    }
    let out = service("kanata", &[("TEST_KANATA", "own")]);
    assert_eq!(out.code, Some(78), "{}", out.stderr);
    assert!(out.stderr.contains("kanata is already running") && !out.stdout.contains(" -c "), "{}", out.stderr);
    assert!(service("check", &[("TEST_KANATA", "other")]).ok);

    // A warning (CapsLock is Ctrl on the desktop, not in the settings) is shown, and kanata runs; not by ime-sync-on.
    let caps_ctrl = [("TEST_XKB_OPTIONS", "['ctrl:nocaps']")];
    let out = service("kanata", &caps_ctrl);
    assert!(out.ok && out.stderr.contains("CapsLock is Ctrl") && ran(&out).starts_with("kanata -c "), "{}", out.stderr);
    let out = service("ime-sync-on", &caps_ctrl);
    assert!(out.code == Some(0) && out.stderr.is_empty(), "{}", out.stderr);

    // kanata refuses the configuration: 78 (the unit does not restart), with kanata's message.
    std::fs::write(&check_fails, "").unwrap();
    for command in ["kanata", "check"] {
        let out = service(command, &[]);
        assert_eq!(out.code, Some(78), "{command}: {}", out.stderr);
        assert!(out.stderr.contains("error in the configuration") && !out.stdout.contains(" -c "), "{command}");
    }
    std::fs::remove_file(&check_fails).unwrap();

    // Mistakes in the settings: 78, with the name of the setting or the file; nicola-imesync does not run.
    for (config_env, message) in [
        ("NICOLA_IME_SYNC=1\nNICOLA_KANATA_PORT=0\n", "NICOLA_KANATA_PORT"),
        ("NICOLA_LAYOUT_OVERLAYS=nope\n", "nope.toml"),
        ("NICOLA_EXTRA_KBD=nope.kbd\n", "nope.kbd"),
        // Not 127 from set -u in service.sh, which would make the unit restart.
        ("NICOLA_EXTRA_KBD=\"$UNSET_IN_TEST/user-extra.kbd\"\n", "UNSET_IN_TEST: unbound variable"),
    ] {
        write_config(config_env);
        let out = service("kanata", &[]);
        assert_eq!(out.code, Some(78), "{config_env:?}: {}", out.stderr);
        assert!(out.stderr.contains(message), "{config_env:?}: {}", out.stderr);
        assert_eq!(service("ime-sync-on", &[]).code, Some(1), "{config_env:?}");
    }

    // NICOLA_KANATA_BIN: that kanata, with the warning that it is not our patched build.
    write_config(&format!("NICOLA_KANATA_BIN={}\n", kanata("other-kanata").display()));
    let out = service("kanata", &[]);
    assert!(out.ok, "{}", out.stderr);
    assert_eq!(ran(&out), format!("other-kanata -c {} --no-wait", kbd.display()));
    assert!(out.stderr.contains("not our patched build"), "{}", out.stderr);

    // Neither NICOLA_KANATA_BIN nor the kanata of the installation: 78, pointing to install.sh.
    std::fs::remove_file(inst.join("kanata")).unwrap();
    write_config("");
    let out = service("kanata", &[]);
    assert_eq!(out.code, Some(78), "{}", out.stderr);
    assert!(out.stderr.contains("run platform/linux/install.sh"), "{}", out.stderr);
}

/// platform/linux/uninstall.sh in a home of its own, with a systemctl that only notes how it was run: it removes
/// nothing unless told to (y, or -y), and then stops the services and removes them and the installation, not the
/// settings.
#[test]
fn uninstall() {
    let dir = case_dir("uninstall");
    let log = dir.join("systemctl.log");
    write_executable(&dir.join("bin/systemctl"), "#!/bin/sh\necho \"$*\" >>\"$SYSTEMCTL_LOG\"\n");
    let units = dir.join(".config/systemd/user");
    let unit_files = [units.join("kanata-nicola.service"), units.join("kanata-nicola-imesync.service")];
    let inst = dir.join(".local/share/kanata-nicola");
    let settings = settings_dir(&dir).join("config.env");
    let install = || {
        std::fs::create_dir_all(&units).unwrap();
        for unit in &unit_files {
            std::fs::write(unit, "").unwrap();
        }
        std::fs::create_dir_all(&inst).unwrap();
        std::fs::write(inst.join("kanata"), "").unwrap();
        std::fs::write(&settings, "NICOLA_IME_SYNC=1\n").unwrap();
        let _ = std::fs::remove_file(&log);
    };
    let run = |args: &[&str], answer: &str| {
        let mut child = Command::new("bash")
            .arg(repo().join("platform/linux/uninstall.sh"))
            .args(args)
            .env_clear()
            .env("PATH", format!("{}:{}", dir.join("bin").display(), std::env::var("PATH").unwrap()))
            .env("HOME", &dir)
            .env("SYSTEMCTL_LOG", &log)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .unwrap();
        child.stdin.take().unwrap().write_all(answer.as_bytes()).unwrap();
        let out = child.wait_with_output().unwrap();
        (out.status.code(), String::from_utf8(out.stdout).unwrap())
    };

    // No, or no answer (an empty line, the end of input): nothing stopped or removed.
    for answer in ["n\n", "\n", ""] {
        install();
        let (code, stdout) = run(&[], answer);
        assert_eq!(code, Some(1), "{answer:?}: {stdout}");
        assert!(stdout.contains("Nothing was removed"), "{answer:?}: {stdout}");
        assert!(inst.exists() && unit_files.iter().all(|u| u.exists()) && !log.exists(), "{answer:?}");
    }
    // Yes, or -y: the services stopped and disabled, their unit files and the installation gone, the settings kept.
    for (args, answer) in [(&[][..], "y\n"), (&["-y"][..], "")] {
        install();
        let (code, stdout) = run(args, answer);
        assert_eq!(code, Some(0), "{args:?}: {stdout}");
        assert!(stdout.ends_with("Removed.\n"), "{args:?}: {stdout}");
        assert!(!inst.exists() && unit_files.iter().all(|u| !u.exists()) && settings.exists(), "{args:?}");
        let calls = std::fs::read_to_string(&log).unwrap();
        assert!(calls.contains("--user disable --now kanata-nicola-imesync.service kanata-nicola.service"), "{calls}");
        assert!(calls.contains("--user daemon-reload"), "{calls}");
    }
    assert_eq!(run(&["--all"], "").0, Some(2));
}

/// platform/linux/permissions.sh, with stand-ins for what it looks at in the system, and a sudo first in PATH that
/// only notes how it was run (and what tee was given), so that nothing is set up for real.
#[test]
fn permissions() {
    use std::os::unix::fs::PermissionsExt;
    // Run as a command, by install.sh and by hand; the cases below source it.
    let mode = std::fs::metadata(repo().join("platform/linux/permissions.sh")).unwrap().permissions().mode();
    assert_eq!(mode & 0o111, 0o111, "platform/linux/permissions.sh is not executable");

    let dir = case_dir("permissions");
    let sudo_log = dir.join("sudo.log");
    write_executable(
        &dir.join("bin/sudo"),
        "#!/bin/sh\necho \"$*\" >>\"$SUDO_LOG\"\n[ \"$1\" = tee ] && sed 's/^/stdin: /' >>\"$SUDO_LOG\"\nexit 0\n",
    );
    let user = String::from_utf8(Command::new("id").arg("-un").output().unwrap().stdout).unwrap();
    let user = user.trim();
    // STATE: the words of what the system has. After the commands have run (udevadm trigger), /dev/uinput is set up
    // if STATE has setup-works.
    let script = r#"source platform/linux/permissions.sh
has() { [[ " $STATE " == *" $1 "* ]]; }
for f in can_read_keyboards can_use_uinput in_group has_group group_exists uinput_exists uinput_set_up rule_file_exists; do
  declare -F "$f" >/dev/null || { echo "permissions.sh has no function $f" >&2; exit 9; }
done
can_read_keyboards() { has keyboards; }
can_use_uinput() { has uinput; }
in_group() { has "db:$1"; }
has_group() { has "login:$1"; }
group_exists() { has "group:$1"; }
uinput_exists() { has dev; }
uinput_set_up() { has set-up || { has setup-works && grep -q '^udevadm trigger' "$SUDO_LOG" 2>/dev/null; }; }
rule_file_exists() { has rule; }
if [[ -n "${NO_INPUT:-}" ]]; then permissions_main "$@" </dev/null; else permissions_main "$@" <<<"$ANSWER"; fi"#;
    // What it printed, and the commands sudo was given.
    let run = |state: &str, args: &[&str], answer: &str| {
        let _ = std::fs::remove_file(&sudo_log);
        let env = [("SUDO_LOG", sudo_log.to_str().unwrap()), ("STATE", state), ("ANSWER", answer)];
        let out = bash(&dir, &repo(), script, args, &env);
        assert_ne!(out.code, Some(9), "{}", out.stderr);
        (out, std::fs::read_to_string(&sudo_log).unwrap_or_default())
    };
    let uinput_all = format!(
        "groupadd --system uinput\nmodprobe uinput\ntee /etc/udev/rules.d/99-input.rules\n\
         stdin: KERNEL==\"uinput\", MODE=\"0660\", GROUP=\"uinput\", OPTIONS+=\"static_node=uinput\"\n\
         udevadm control --reload-rules\nudevadm trigger --subsystem-match=misc --sysname-match=uinput\n\
         usermod -aG uinput {user}\n"
    );

    // Nothing missing.
    for args in [&[][..], &["--check"]] {
        let (out, sudo) = run("keyboards uinput", args, "y");
        assert!(out.ok && out.stdout.starts_with("ok: "), "{args:?}: {}", out.stderr);
        assert_eq!(sudo, "", "{args:?}");
    }

    // --check: says what is missing and how to set it up; asks nothing, runs nothing.
    let (out, sudo) = run("", &["--check"], "y");
    assert!(!out.ok && !out.stdout.contains("Run them?"), "{}", out.stdout);
    assert!(out.stderr.contains("needs the input group") && out.stderr.contains("needs the uinput group"));
    assert!(out.stderr.contains("To set it up: platform/linux/permissions.sh"), "{}", out.stderr);
    assert_eq!(sudo, "");

    // Asks first, with the note on what the groups allow: no, and nothing is run.
    let (out, sudo) = run("", &[], "n");
    assert!(!out.ok && out.stdout.contains("can read all key input"), "{}", out.stdout);
    assert!(out.stdout.contains("Nothing was changed."), "{}", out.stdout);
    assert_eq!(sudo, "");
    // No answer (no terminal, end of input) is no, and says so.
    let _ = std::fs::remove_file(&sudo_log);
    let env = [("SUDO_LOG", sudo_log.to_str().unwrap()), ("STATE", ""), ("NO_INPUT", "1")];
    let out = bash(&dir, &repo(), script, &[], &env);
    assert!(!out.ok && out.stdout.contains("Nothing was changed."), "{}{}", out.stdout, out.stderr);
    assert!(!sudo_log.exists());

    // Yes: only what is missing is set up, in order. The input group takes effect at the next login.
    let (out, sudo) = run("uinput", &[], "y");
    assert!(out.ok && out.stdout.contains("Log out and log in again"), "{}{}", out.stdout, out.stderr);
    assert_eq!(sudo, format!("usermod -aG input {user}\n"));
    let (out, sudo) = run("keyboards setup-works", &[], "y");
    assert!(out.ok && out.stdout.contains("Log out and log in again"), "{}{}", out.stdout, out.stderr);
    assert_eq!(sudo, uinput_all);
    let (out, sudo) = run("setup-works", &[], "y");
    assert!(out.ok, "{}", out.stderr);
    assert_eq!(sudo, format!("usermod -aG input {user}\n{uinput_all}"));
    // An existing group, device and rule file are kept (the rules are read again).
    let (out, sudo) = run("keyboards group:uinput dev rule setup-works", &[], "y");
    assert!(out.ok, "{}", out.stderr);
    let want = format!(
        "udevadm control --reload-rules\nudevadm trigger --subsystem-match=misc --sysname-match=uinput\n\
         usermod -aG uinput {user}\n"
    );
    assert_eq!(sudo, want);
    // /dev/uinput already set up: only the group.
    let (_, sudo) = run("keyboards group:uinput dev set-up", &[], "y");
    assert_eq!(sudo, format!("usermod -aG uinput {user}\n"));
    // Set up, and still not for the uinput group (e.g. an existing rule file without the line): says so.
    let (out, _) = run("keyboards group:uinput dev rule", &[], "y");
    assert!(!out.ok && out.stderr.contains("still not readable and writable"), "{}", out.stderr);

    // In the groups, but not in this login: nothing to run, log in again.
    let (out, sudo) = run("uinput db:input", &[], "y");
    assert!(!out.ok && out.stderr.contains("log out and log in again"), "{}", out.stderr);
    assert_eq!(sudo, "");
    // In this login too, and still no access: no advice to log in again.
    let (out, sudo) = run("uinput db:input login:input", &["--check"], "y");
    assert!(!out.ok && out.stderr.contains("yet lacks the access"), "{}", out.stderr);
    assert_eq!(sudo, "");

    let (out, _) = run("", &["--yes"], "y");
    assert!(!out.ok && out.stderr.contains("unknown argument: --yes"), "{}", out.stderr);
}

/// The settings are listed alike in config.sh, docs/settings.md and config.env.example.
#[test]
fn docs_settings_are_listed_alike() {
    let dir = case_dir("settings-list");
    let out =
        bash(&dir, &repo(), r#"source platform/linux/config.sh; printf '%s\n' "${nicola_settings[@]}""#, &[], &[]);
    let mut script: Vec<String> = out.stdout.lines().map(String::from).collect();
    script.sort();
    let listed = |file: &str, prefix: &str| {
        let text = std::fs::read_to_string(repo().join(file)).unwrap();
        let mut names: Vec<String> = text
            .lines()
            .filter_map(|l| l.strip_prefix(prefix))
            .filter(|l| l.starts_with("NICOLA_"))
            .map(|l| l.chars().take_while(|c| c.is_ascii_uppercase() || *c == '_').collect())
            .collect();
        names.sort();
        names.dedup();
        names
    };
    assert_eq!(script.len(), 10);
    assert_eq!(listed("docs/settings.md", "| `"), script, "docs/settings.md");
    assert_eq!(listed("platform/linux/config.env.example", "# "), script, "platform/linux/config.env.example");
}

/// The defaults that the table of docs/settings.md gives as values are those of config.sh (the TCP port, IME sync, the
/// wait of kanata) and of the generator (the thumb keys).
#[test]
fn docs_settings_give_the_defaults() {
    let dir = case_dir("settings-defaults");
    let port = bash(&dir, &repo(), "source platform/linux/config.sh; echo $nicola_default_port", &[], &[]).stdout;
    // Without a config.env.
    let script = r#"source platform/linux/config.sh; nicola_config; echo "$ime_sync"; echo "${kanata_args[*]}""#;
    let out = bash(&dir, &repo(), script, &[], &[]).stdout;
    let (ime_sync, kanata) = out.trim().split_once('\n').unwrap();
    let wait = kanata.strip_prefix("--wait-device-ms ").unwrap_or_else(|| panic!("{kanata}"));
    let text = std::fs::read_to_string(repo().join("docs/settings.md")).unwrap();
    // The third column (the default) of the setting's row.
    let default = |name: &str| {
        let row = text.lines().find(|l| l.starts_with(&format!("| `{name}` |"))).unwrap_or_else(|| panic!("{name}"));
        row.split(" | ").nth(2).unwrap().to_string()
    };
    assert_eq!(default("NICOLA_KANATA_PORT"), format!("`{}`", port.trim()));
    for (name, value) in [("NICOLA_IME_SYNC", ime_sync), ("NICOLA_WAIT_DEVICE_MS", wait)] {
        let cell = default(name);
        assert!(cell.starts_with(&format!("`{value}`")), "{name}: {cell}");
    }
    let opt = nicola_kbdgen::Options::default();
    let thumbs = default("NICOLA_THUMB_KEYS");
    assert!(thumbs.contains(&format!("`{}:{}`", opt.thumb_left, opt.thumb_right)), "{thumbs}");
}

/// The documents give the packages for the build as config.sh names them when they are missing (README.md,
/// docs/usage.md §2), and the udev rule as permissions.sh writes it (docs/usage.md §5.2).
#[test]
fn docs_give_the_packages_and_the_udev_rule() {
    let read = |file: &str| std::fs::read_to_string(repo().join(file)).unwrap();
    let config = read("platform/linux/config.sh");
    let apt = config
        .lines()
        .find_map(|l| l.split("On Ubuntu: ").nth(1)?.split('"').next())
        .expect("the packages in config.sh");
    for doc in ["README.md", "docs/usage.md"] {
        let text = read(doc);
        let lines: Vec<&str> =
            text.lines().map(str::trim).filter(|l| l.starts_with("sudo apt install build-essential")).collect();
        assert_eq!(lines, [apt], "{doc}");
    }
    let permissions = read("platform/linux/permissions.sh");
    let rule = permissions
        .lines()
        .find_map(|l| l.strip_prefix("rule='")?.strip_suffix('\''))
        .expect("rule= in permissions.sh");
    assert!(read("docs/usage.md").lines().any(|l| l == rule), "docs/usage.md does not have {rule}");
}
