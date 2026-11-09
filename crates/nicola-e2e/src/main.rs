//! `nicola-e2e` — E2E test over the real device path (docs/implementation.md, tests).
//!
//! Plays the case file of expected behavior (testdata/kanata_prefix_cases.toml) in real time over the path
//! virtual keyboard (uinput) -> real kanata process -> kanata output device, and checks the output. Then checks
//! unplugging and replugging the keyboard, the commands that kanata's TCP port (IME sync) takes, and resource use.
//!
//! Safety measures:
//! - kanata is made to grab only the virtual keyboard via `linux-dev-names-include` (real keyboards are not touched).
//! - This tool takes an exclusive grab (EVIOCGRAB) of the kanata output device, so no output reaches the desktop.
//!   If the grab fails, it aborts before injecting any keys.
//! - Until kanata grabs it, the virtual keyboard is an ordinary keyboard to the desktop, so keys are injected
//!   only while kanata's log says it registered the device (it logs "registering" right before the grab, and
//!   a failure or removal afterwards), kanata holds it open and an own grab attempt fails with EBUSY. Then the
//!   grab is kanata's: the device registers only if kanata's grab succeeded. An open node alone is not enough:
//!   kanata's rescan opens every input device before it grabs the new one (docs/design.md, kanata constraints). This is checked before the cases, before each
//!   case, and after the replug of the hotplug check; if it does not hold, no keys are injected.
//!
//! Usage: `cargo run -p nicola-e2e [-- [--kanata <path>] [--only <case-id>] [--wait-device-ms <ms>]]`
//! (`--wait-device-ms` is passed to kanata; see below)
//!
//! kanata runs with --wait-device-ms 0 by default: it then looks for a replugged keyboard before udev has
//! given the device node its permissions, so the hotplug check fails unless kanata also rescans on that
//! permission change (platform/linux/kanata/patches/0002).
//! Required permissions: read/write on /dev/uinput and /dev/input/event* (uinput / input groups).

use std::io::{BufRead, BufReader, Write};
use std::net::{TcpListener, TcpStream};
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::sync::mpsc;
use std::thread;
use std::time::{Duration, Instant};

use evdev::uinput::VirtualDevice;
use evdev::{AttributeSet, Device, EventType, InputEvent, KeyCode};
use nicola_kbdgen::decode::{canonical, Decoder};
use nicola_kbdgen::{generate, Options};
use nicola_layout::{keycodes, LayoutDef, NICOLA_J};
use nicola_testcases::{load, parse_expect, parse_input, Action, Step, KANATA_PREFIX_CASES};

const INPUT_NAME: &str = "nicola-e2e-input";
/// Output device name of the E2E kanata. Distinguishes it from the user's kanata (default name "kanata").
const KANATA_OUTPUT_NAME: &str = "nicola-e2e-output";

type Res<T> = Result<T, Box<dyn std::error::Error>>;

fn main() -> std::process::ExitCode {
    match run() {
        Ok(()) => std::process::ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("nicola-e2e: {e}");
            std::process::ExitCode::FAILURE
        }
    }
}

struct KanataProc {
    child: Child,
    /// Device events from kanata's log.
    log: mpsc::Receiver<(DeviceLog, String)>,
    /// The last device event per device node.
    devices: std::collections::HashMap<String, DeviceLog>,
}

/// What kanata's log says about a device node.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum DeviceLog {
    /// `registering <node>`: written right before the grab.
    Registering,
    /// `found device <node> but could not register it`: the grab (or the wait before it) failed; closed.
    Failed,
    /// `removing kbd device: <node>`, `removing stale kbd device: <node>`.
    Removed,
}

impl DeviceLog {
    /// Parses a log line (colours removed) into a device event and its node.
    fn parse(line: &str) -> Option<(DeviceLog, String)> {
        let node_after = |marker: &str| {
            let rest = line.split_once(marker)?.1;
            Some(rest.split([':', ' ']).next().unwrap_or_default().to_string())
        };
        if line.contains("but could not register it") {
            return node_after("found device ").map(|n| (DeviceLog::Failed, n));
        }
        let removed = || node_after("removing kbd device: ").or_else(|| node_after("removing stale kbd device: "));
        node_after("] registering ")
            .map(|n| (DeviceLog::Registering, n))
            .or_else(|| removed().map(|n| (DeviceLog::Removed, n)))
    }
}

/// Removes a file when dropped.
struct RemoveOnDrop(PathBuf);

impl Drop for RemoveOnDrop {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(&self.0);
    }
}

impl Drop for KanataProc {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

impl KanataProc {
    /// Starts kanata with its log (stdout) read here: device events are passed on, errors shown.
    fn spawn(mut cmd: Command) -> Res<Self> {
        let mut child = cmd.stdin(Stdio::null()).stdout(Stdio::piped()).stderr(Stdio::inherit()).spawn()?;
        let stdout = child.stdout.take().ok_or("no stdout of kanata")?;
        let (tx, log) = mpsc::channel();
        thread::spawn(move || {
            for line in std::io::BufReader::new(stdout).lines().map_while(Result::ok) {
                let line = strip_ansi(&line);
                if let Some(event) = DeviceLog::parse(&line) {
                    if tx.send(event).is_err() {
                        return;
                    }
                } else if line.contains("[ERROR]") {
                    eprintln!("kanata: {line}");
                }
            }
        });
        Ok(Self { child, log, devices: Default::default() })
    }

    /// Takes in the device events logged so far. Fails if kanata's log ended (kanata exited).
    fn read_log(&mut self) -> Res<()> {
        loop {
            match self.log.try_recv() {
                Ok((event, node)) => {
                    self.devices.insert(node, event);
                }
                Err(mpsc::TryRecvError::Empty) => return Ok(()),
                Err(mpsc::TryRecvError::Disconnected) => return Err("kanata's log ended".into()),
            }
        }
    }

    /// Whether kanata's last word on the node is that it registered it (and so grabbed it).
    fn registered(&mut self, node: &Path) -> Res<bool> {
        self.read_log()?;
        Ok(self.devices.get(&*node.to_string_lossy()) == Some(&DeviceLog::Registering))
    }

    /// Forgets what the log said about the node: called before plugging in a new device at that node.
    fn forget(&mut self, node: &Path) -> Res<()> {
        self.read_log()?;
        self.devices.remove(&*node.to_string_lossy());
        Ok(())
    }

    /// Whether kanata is running and holds the device node open.
    fn holds(&mut self, node: &Path) -> Res<bool> {
        if let Some(status) = self.child.try_wait()? {
            return Err(format!("kanata exited ({status})").into());
        }
        let Ok(fds) = std::fs::read_dir(format!("/proc/{}/fd", self.child.id())) else { return Ok(false) };
        Ok(fds.flatten().any(|fd| std::fs::read_link(fd.path()).is_ok_and(|t| t == node)))
    }

    /// Waits until kanata registers the device node (logged right before the grab), then checks the grab.
    fn wait_for_grab(&mut self, node: &Path, timeout: Duration) -> Res<()> {
        let start = Instant::now();
        while !self.registered(node)? {
            if start.elapsed() > timeout {
                return Err(format!("kanata did not register {} within {} s", node.display(), timeout.as_secs()).into());
            }
            if let Some(status) = self.child.try_wait()? {
                return Err(format!("kanata exited ({status})").into());
            }
            thread::sleep(Duration::from_millis(20));
        }
        // The grab follows the log line at once; give it time to finish (or to log a failure).
        thread::sleep(Duration::from_millis(100));
        self.ensure_grabbed(node)
    }

    /// Checked before injecting keys: kanata registered the virtual keyboard (no failure or removal logged
    /// since), holds it open, and someone has an exclusive grab of it, which is then kanata. Otherwise the
    /// keys could reach the desktop.
    fn ensure_grabbed(&mut self, node: &Path) -> Res<()> {
        if !self.registered(node)? {
            return Err(format!("kanata has not registered {} (aborting before injecting keys)", node.display()).into());
        }
        if !self.holds(node)? {
            return Err(format!("kanata no longer holds {} (aborting before injecting keys)", node.display()).into());
        }
        check_exclusive(node)
    }
}

/// Removes the colour escape sequences of kanata's log.
fn strip_ansi(line: &str) -> String {
    let mut out = String::with_capacity(line.len());
    let mut chars = line.chars();
    while let Some(c) = chars.next() {
        if c == '\u{1b}' {
            for c in chars.by_ref() {
                if c.is_ascii_alphabetic() {
                    break;
                }
            }
        } else {
            out.push(c);
        }
    }
    out
}

/// Checks that the device node has an exclusive grab: an own grab attempt must fail with EBUSY. If it
/// succeeds, nobody had grabbed it; it is released at once (before any key is injected) and this fails.
fn check_exclusive(node: &Path) -> Res<()> {
    let mut dev = evdev::raw_stream::RawDevice::open(node)?;
    match dev.grab() {
        Err(e) if e.kind() == std::io::ErrorKind::ResourceBusy => Ok(()),
        Err(e) => Err(format!("cannot check the grab of {}: {e}", node.display()).into()),
        Ok(()) => {
            let _ = dev.ungrab();
            Err(format!("kanata holds {} open but has not grabbed it (aborting before injecting keys)", node.display())
                .into())
        }
    }
}

/// Device node (/dev/input/eventN) of a virtual keyboard.
fn dev_node(dev: &mut VirtualDevice) -> Res<PathBuf> {
    let node = dev.enumerate_dev_nodes_blocking()?.next().ok_or("virtual keyboard has no device node")??;
    Ok(node)
}

fn run() -> Res<()> {
    // Use our own build with the upstream fix (platform/linux/kanata) if present, otherwise the kanata on PATH.
    // Found in the repository, wherever this runs from.
    let patched = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../target/kanata-patched/kanata").canonicalize().ok();
    let mut kanata_bin = patched.map_or_else(|| "kanata".to_string(), |p| p.to_string_lossy().into_owned());
    let mut only: Option<String> = None;
    let mut wait_device_ms = "0".to_string();
    let mut args = std::env::args().skip(1);
    while let Some(a) = args.next() {
        match a.as_str() {
            "--kanata" => kanata_bin = args.next().ok_or("--kanata needs a path")?,
            "--only" => only = Some(args.next().ok_or("--only needs a case id")?),
            "--wait-device-ms" => wait_device_ms = args.next().ok_or("--wait-device-ms needs a value")?,
            _ => return Err(format!("unknown argument {a}").into()),
        }
    }
    if enumerate_by_name(KANATA_OUTPUT_NAME).is_some() {
        return Err("another nicola-e2e is running (its kanata output device exists)".into());
    }
    check_other_kanata()?;

    let layout = LayoutDef::parse(NICOLA_J)?;
    let opt = Options {
        start_enabled: true,
        include_device_names: vec![INPUT_NAME.into()],
        output_device_name: Some(KANATA_OUTPUT_NAME.into()),
        ..Options::default()
    };
    // A directory of this user only (not the shared /tmp), or target/ of the repository.
    let cfg_dir = std::env::var_os("XDG_RUNTIME_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|| Path::new(env!("CARGO_MANIFEST_DIR")).join("../../target"));
    let cfg_path = cfg_dir.join("nicola-e2e.kbd");
    std::fs::write(&cfg_path, generate(&layout, &opt)?)?;
    // Removed at the end, after kanata (declared later, so dropped first) has stopped.
    let _cfg_file = RemoveOnDrop(cfg_path.clone());

    // 1. Create the virtual keyboard.
    let mut keys = AttributeSet::<KeyCode>::new();
    for code in 1..=127u16 {
        keys.insert(KeyCode::new(code));
    }
    let mut input = VirtualDevice::builder()?.name(INPUT_NAME).with_keys(&keys)?.build()?;
    let mut input_node = dev_node(&mut input)?;
    thread::sleep(Duration::from_millis(300));

    // 2. Start kanata and take an exclusive grab of its output device.
    // Without -q: the E2E reads kanata's log (registering lines).
    // With the TCP port of IME sync, on a free port, for the check of the commands it takes (check_tcp).
    let port = TcpListener::bind("127.0.0.1:0")?.local_addr()?.port();
    let mut cmd = Command::new(&kanata_bin);
    cmd.args(["-c", cfg_path.to_str().unwrap(), "-n", "--no-wait"]).args(["--wait-device-ms", wait_device_ms.as_str()]);
    cmd.args(["-p", &format!("127.0.0.1:{port}")]);
    let mut kanata = KanataProc::spawn(cmd)?;
    let mut out_dev = wait_for_device(KANATA_OUTPUT_NAME, Duration::from_secs(5))?;
    out_dev.grab().map_err(|e| format!("cannot grab kanata output device (aborting before injecting keys): {e}"))?;
    let kanata_ver = Command::new(&kanata_bin).arg("--version").output()?;
    println!("kanata: {} ({kanata_bin})  output device grabbed", String::from_utf8_lossy(&kanata_ver.stdout).trim());
    // kanata opens the input devices after creating its output device.
    kanata
        .wait_for_grab(&input_node, Duration::from_secs(5))
        .map_err(|e| format!("kanata did not grab the virtual keyboard (aborting before injecting keys): {e}"))?;
    println!("kanata grabbed the virtual keyboard ({})", input_node.display());
    thread::sleep(Duration::from_millis(500));

    // 3. Thread that reads the output.
    let (tx, rx) = mpsc::channel::<(Instant, u16, i32)>();
    thread::spawn(move || loop {
        let Ok(events) = out_dev.fetch_events() else { return };
        for ev in events {
            if ev.event_type() == EventType::KEY && tx.send((Instant::now(), ev.code(), ev.value())).is_err() {
                return;
            }
        }
    });

    let kanata_pid = kanata.child.id();
    let typing_start = (Instant::now(), cpu_seconds(kanata_pid)?);

    // 4. Play the cases.
    let decoder = Decoder::new(&layout);
    let (mut pass, mut fail, mut skip) = (0, 0, 0);
    for nicola_testcases::Case { id, input: case_input, expect: want, .. } in load(KANATA_PREFIX_CASES) {
        if only.as_ref().is_some_and(|o| *o != id) {
            continue;
        }
        let want = want.as_str();
        let steps = parse_input(&case_input);
        if steps.iter().any(|s| matches!(s, Step::Control { .. })) {
            skip += 1;
            continue;
        }
        kanata.ensure_grabbed(&input_node)?;
        let got = play(&mut input, &rx, &decoder, &steps)?;
        let want_tokens: Vec<String> = parse_expect(want).into_iter().map(|t| canonical(&t.text)).collect();
        if got == want_tokens {
            pass += 1;
            println!("ok    {:<14} {}", id, got.join(" "));
        } else {
            fail += 1;
            println!(
                "FAIL  {:<14} expected: {}\n      {:<14} actual:   {}",
                id,
                want_tokens.join(" "),
                "",
                got.join(" ")
            );
        }
        thread::sleep(Duration::from_millis(200));
    }
    println!("\n{pass} passed, {fail} failed, {skip} skipped (control events)");

    // 5. Keyboard unplug/replug (stands in for reconnection on resume from sleep).
    let (hotplug_failures, new_input) = check_hotplug(input, &mut input_node, &keys, &rx, &decoder, &mut kanata)?;
    fail += hotplug_failures;
    drop(new_input);

    // 6. The TCP port (IME sync).
    fail += check_tcp(port)?;

    // 7. Resource use (docs/design.md, performance; docs/implementation.md, tests).
    let resource_failures = check_resources(kanata_pid, typing_start)?;
    fail += resource_failures;
    if fail > 0 {
        // Leave via return so Drop stops kanata (process::exit does not run destructors).
        return Err(format!("{fail} cases failed").into());
    }
    Ok(())
}

/// Plays one case and returns the output as tokens. Keys still held at the end are released afterwards.
fn play(
    input: &mut VirtualDevice,
    rx: &mpsc::Receiver<(Instant, u16, i32)>,
    decoder: &Decoder,
    steps: &[Step],
) -> Res<Vec<String>> {
    while rx.try_recv().is_ok() {}
    let mut held: Vec<u16> = Vec::new();
    let start = Instant::now();
    for step in steps {
        let at = start + Duration::from_millis(step.t_ms());
        if let Some(d) = at.checked_duration_since(Instant::now()) {
            thread::sleep(d);
        }
        if let Step::Key { action, name, .. } = step {
            let code = keycodes::code_of(name).ok_or_else(|| format!("unknown key {name}"))?;
            let value = match action {
                Action::Down => 1,
                Action::Up => 0,
                Action::Repeat => 2,
            };
            input.emit(&[InputEvent::new(EventType::KEY.0, code, value)])?;
            match action {
                Action::Down => held.push(code),
                Action::Up => held.retain(|c| *c != code),
                Action::Repeat => {}
            }
        }
    }
    // Wait for the last character to be sent (it goes out within a few ms of its key), then collect the output.
    thread::sleep(Duration::from_millis(300));
    let mut raw = Vec::new();
    while let Ok(ev) = rx.try_recv() {
        raw.push(ev);
    }
    let raw: Vec<(u16, i32)> = raw.iter().map(|&(_, c, v)| (c, v)).collect();
    let got = decoder.decode(&raw);
    // Clean up after cases that end with keys held, so the next case's key downs are not dropped as duplicates.
    for code in held.drain(..) {
        input.emit(&[InputEvent::new(EventType::KEY.0, code, 0)])?;
    }
    thread::sleep(Duration::from_millis(300));
    while rx.try_recv().is_ok() {}
    Ok(got)
}

fn new_input(keys: &AttributeSet<KeyCode>) -> Res<VirtualDevice> {
    Ok(VirtualDevice::builder()?.name(INPUT_NAME).with_keys(keys)?.build()?)
}

/// Keyboard unplug/replug. Instead of a USB or Bluetooth keyboard reconnecting on resume from sleep,
/// removes and recreates the virtual keyboard. Also checks that when unplugged with a key held, neither the
/// shift (the thumb's while-held layer) nor any output-side key stays pressed.
fn check_hotplug(
    mut input: VirtualDevice,
    input_node: &mut PathBuf,
    keys: &AttributeSet<KeyCode>,
    rx: &mpsc::Receiver<(Instant, u16, i32)>,
    decoder: &Decoder,
    kanata: &mut KanataProc,
) -> Res<(usize, VirtualDevice)> {
    println!("\nhotplug:");
    let mut failures = 0;
    let mut expect = |label: &str, got: Vec<String>, want: &str| {
        let want: Vec<String> = want.split_whitespace().map(String::from).collect();
        let ok = got == want;
        println!("{}  {label:<46} {}", if ok { "ok  " } else { "FAIL" }, got.join(" "));
        if !ok {
            println!("      {:<46} expected: {}", "", want.join(" "));
            failures += 1;
        }
    };
    let steps = |s: &str| parse_input(s);
    let shifted = steps("d:HENKAN@0 d:W@30 u:W@60 u:HENKAN@100");
    let plain = steps("d:W@0 u:W@40");
    kanata.ensure_grabbed(input_node)?;
    expect("baseline: Henkan+W", play(&mut input, rx, decoder, &shifted)?, "が");

    // Collect all raw output from here on, to check that output-side downs and ups balance.
    let mut balance: std::collections::HashMap<u16, i32> = Default::default();
    let drain = |balance: &mut std::collections::HashMap<u16, i32>| {
        while let Ok((_, code, value)) = rx.try_recv() {
            match value {
                1 => *balance.entry(code).or_default() += 1,
                0 => *balance.entry(code).or_default() -= 1,
                _ => {}
            }
        }
    };

    // The unplug/replug sequence can be changed with NICOLA_E2E_HOTPLUG (e.g. "idle,thumb,char"), for isolating problems.
    let plan = std::env::var("NICOLA_E2E_HOTPLUG").unwrap_or_else(|_| "idle,thumb,char".into());
    let plan: Vec<(&str, Option<&str>)> = plan
        .split(',')
        .map(|p| match p {
            "thumb" => ("thumb held (unplug with Henkan held)", Some("KEY_HENKAN")),
            "char" => ("char held (unplug with W held)", Some("KEY_W")),
            _ => ("idle", None),
        })
        .collect();
    for (label, hold) in plan {
        if let Some(k) = hold {
            let code = keycodes::code_of(k).unwrap();
            kanata.ensure_grabbed(input_node)?;
            input.emit(&[InputEvent::new(EventType::KEY.0, code, 1)])?;
            thread::sleep(Duration::from_millis(150));
        }
        drop(input); // unplug
        thread::sleep(Duration::from_millis(1000));
        drain(&mut balance);
        // The new device may get the same node: what the log said about the old one no longer counts.
        kanata.forget(input_node)?;
        input = new_input(keys)?; // replug
        *input_node = dev_node(&mut input)?;
        // Until kanata grabs it again, the new keyboard types into the desktop: inject nothing before that.
        let t0 = Instant::now();
        let grabbed = kanata.wait_for_grab(input_node, Duration::from_secs(10));
        let grab_ms = t0.elapsed().as_millis();
        if let Err(e) = grabbed {
            expect(&format!("{label}: re-grabbed ({e})"), vec!["false".into()], "true");
            println!("      stopping the hotplug check (no keys are injected into a keyboard kanata did not grab)");
            break;
        }
        // Once grabbed, the first key must come through.
        let through = probe(&mut input, rx)?;
        expect(&format!("{label}: re-grabbed in {grab_ms} ms, first key through"), vec![through.to_string()], "true");
        expect(&format!("{label}: W after replug (no leftover shift)"), play(&mut input, rx, decoder, &plain)?, "か");
        expect(&format!("{label}: Henkan+W after replug"), play(&mut input, rx, decoder, &shifted)?, "が");
    }
    drain(&mut balance);
    let stuck: Vec<String> = balance
        .iter()
        .filter(|(_, n)| **n > 0)
        .map(|(c, _)| keycodes::name_of(*c).unwrap_or("?").to_string())
        .collect();
    expect("no stuck output keys", stuck, "");
    let alive = kanata.child.try_wait()?.is_none();
    expect("kanata still running", vec![alive.to_string()], "true");
    Ok((failures, input))
}

/// Sends one key outside the layout (1) and waits up to 1 s for kanata to output it (a rescan kanata is
/// still running can delay it). Only call while kanata has grabbed the keyboard.
fn probe(input: &mut VirtualDevice, rx: &mpsc::Receiver<(Instant, u16, i32)>) -> Res<bool> {
    let code = keycodes::code_of("KEY_1").unwrap();
    while rx.try_recv().is_ok() {}
    input.emit(&[InputEvent::new(EventType::KEY.0, code, 1)])?;
    input.emit(&[InputEvent::new(EventType::KEY.0, code, 0)])?;
    let deadline = Instant::now() + Duration::from_secs(1);
    while let Some(left) = deadline.checked_duration_since(Instant::now()) {
        if matches!(rx.recv_timeout(left), Ok((_, c, 1)) if c == code) {
            thread::sleep(Duration::from_millis(50));
            while rx.try_recv().is_ok() {}
            return Ok(true);
        }
    }
    Ok(false)
}

/// The TCP port of kanata, opened for IME sync: it takes ChangeLayer, and answers with the new layer. Our build
/// refuses the commands that would let any local process load another configuration or act on virtual keys, and
/// disconnects (platform/linux/kanata/patches/0004). The commands refused name nothing that exists, so they change
/// nothing in a kanata that takes them.
fn check_tcp(port: u16) -> Res<usize> {
    println!("\ntcp port:");
    let mut failures = 0;
    let mut expect = |label: &str, ok: bool, got: &str| {
        println!("{}  {label:<46} {got}", if ok { "ok  " } else { "FAIL" });
        failures += usize::from(!ok);
    };
    // A connection, and its first line: kanata tells a new client the current layer.
    let connect = || -> Res<(TcpStream, BufReader<TcpStream>, String)> {
        let stream = TcpStream::connect(("127.0.0.1", port))?;
        stream.set_read_timeout(Some(Duration::from_secs(2)))?;
        let mut reader = BufReader::new(stream.try_clone()?);
        let first = read_line(&mut reader);
        Ok((stream, reader, first))
    };
    let (mut stream, mut reader, first) = connect()?;
    stream.write_all(b"{\"ChangeLayer\":{\"new\":\"base\"}}\n")?;
    let to_base = read_line(&mut reader);
    stream.write_all(b"{\"ChangeLayer\":{\"new\":\"nicola\"}}\n")?;
    let back = read_line(&mut reader);
    let layer = |line: &str, name: &str| line.contains(&format!("\"new\":\"{name}\""));
    let ok = layer(&first, "nicola") && layer(&to_base, "base") && layer(&back, "nicola");
    expect("ChangeLayer taken", ok, &format!("{first} / {to_base} / {back}"));
    drop(stream);

    let refused = [
        ("ReloadFile refused", r#"{"ReloadFile":{"path":"/nonexistent/nicola-e2e.kbd"}}"#),
        ("ActOnFakeKey refused", r#"{"ActOnFakeKey":{"name":"nicola-e2e-none","action":"Tap"}}"#),
    ];
    for (label, command) in refused {
        let (mut stream, mut reader, _) = connect()?;
        stream.write_all(format!("{command}\n").as_bytes())?;
        let answer = read_line(&mut reader);
        let then = read_line(&mut reader);
        expect(label, answer.contains("command not taken") && then == "(closed)", &format!("{answer} / {then}"));
    }
    Ok(failures)
}

/// A line from kanata's TCP port, "(closed)" at its end, or what went wrong. A reset is an end too: kanata closes
/// the connection with the line break after a command still unread, and then the peer gets a reset.
fn read_line(reader: &mut BufReader<TcpStream>) -> String {
    let mut line = String::new();
    match reader.read_line(&mut line) {
        Ok(0) => "(closed)".to_string(),
        Err(e) if e.kind() == std::io::ErrorKind::ConnectionReset => "(closed)".to_string(),
        Ok(_) => line.trim_end().to_string(),
        Err(e) => format!("(no answer: {e})"),
    }
}

/// Limits. Exceeding one fails the test (set with several times the margin over the measured values).
const MAX_IDLE_CPU_PERCENT: f64 = 0.5;
const MAX_TYPING_CPU_PERCENT: f64 = 5.0;
const MAX_RSS_KB: u64 = 50_000;

/// utime+stime from `/proc/<pid>/stat` (seconds). USER_HZ is 100 on Linux.
fn cpu_seconds(pid: u32) -> Res<f64> {
    let stat = std::fs::read_to_string(format!("/proc/{pid}/stat"))?;
    // comm may contain spaces, so count fields after the last ')'.
    let rest = &stat[stat.rfind(')').ok_or("bad stat")? + 2..];
    let f: Vec<&str> = rest.split_whitespace().collect();
    let ticks: u64 = f[11].parse::<u64>()? + f[12].parse::<u64>()?;
    Ok(ticks as f64 / 100.0)
}

fn rss_kb(pid: u32) -> Res<u64> {
    let status = std::fs::read_to_string(format!("/proc/{pid}/status"))?;
    let line = status.lines().find(|l| l.starts_with("VmRSS:")).ok_or("no VmRSS")?;
    Ok(line.split_whitespace().nth(1).ok_or("bad VmRSS")?.parse()?)
}

fn check(label: &str, value: f64, max: f64, unit: &str) -> usize {
    let ok = value <= max;
    println!("{}  {label:<34} {value:>8.2} {unit} (max {max} {unit})", if ok { "ok  " } else { "FAIL" });
    usize::from(!ok)
}

fn check_resources(kanata_pid: u32, typing_start: (Instant, f64)) -> Res<usize> {
    println!("\nresources:");
    let mut failures = 0;
    let (t0, c0) = typing_start;
    let typing = (cpu_seconds(kanata_pid)? - c0) / t0.elapsed().as_secs_f64() * 100.0;
    failures += check("kanata CPU while typing", typing, MAX_TYPING_CPU_PERCENT, "%");

    let idle_secs = 5.0;
    let c1 = cpu_seconds(kanata_pid)?;
    thread::sleep(Duration::from_secs_f64(idle_secs));
    let idle = (cpu_seconds(kanata_pid)? - c1) / idle_secs * 100.0;
    failures += check("kanata CPU idle", idle, MAX_IDLE_CPU_PERCENT, "%");
    failures += check("kanata RSS", rss_kb(kanata_pid)? as f64, MAX_RSS_KB as f64, "kB");

    // Measure nicola-imesync only when fcitx5 is running (--dry-run, so nothing is sent to kanata).
    let fcitx_running = Command::new("pgrep").args(["-x", "fcitx5"]).output()?.status.success();
    let imesync = std::env::current_exe()?.with_file_name("nicola-imesync");
    if fcitx_running && imesync.exists() {
        let mut child = Command::new(&imesync).arg("--dry-run").stdout(Stdio::null()).stderr(Stdio::null()).spawn()?;
        thread::sleep(Duration::from_millis(500));
        let c = cpu_seconds(child.id())?;
        thread::sleep(Duration::from_secs_f64(idle_secs));
        let idle = (cpu_seconds(child.id())? - c) / idle_secs * 100.0;
        let rss = rss_kb(child.id())?;
        let _ = child.kill();
        let _ = child.wait();
        failures += check("nicola-imesync CPU idle", idle, MAX_IDLE_CPU_PERCENT, "%");
        failures += check("nicola-imesync RSS", rss as f64, MAX_RSS_KB as f64, "kB");
    } else {
        println!("skip  nicola-imesync (fcitx5 not running or binary not built: cargo build -p nicola-imesync)");
    }
    Ok(failures)
}

/// Checks that no other running kanata will grab the E2E virtual keyboard.
/// If one did, the injected keys would pass through that kanata and be typed into the user's desktop.
fn check_other_kanata() -> Res<()> {
    for entry in std::fs::read_dir("/proc")? {
        let Ok(entry) = entry else { continue };
        let pid = entry.file_name();
        let Some(pid) = pid.to_str().filter(|p| p.chars().all(|c| c.is_ascii_digit())) else { continue };
        let Ok(cmdline) = std::fs::read(format!("/proc/{pid}/cmdline")) else { continue };
        let args: Vec<String> = cmdline
            .split(|b| *b == 0)
            .filter(|a| !a.is_empty())
            .map(|a| String::from_utf8_lossy(a).into_owned())
            .collect();
        // Any program whose file name contains "kanata" (also builds such as kanata_cmd_allowed). By its
        // executable; by its first argument only when the executable cannot be read (another user's process):
        // a program may rewrite its arguments (VS Code puts its folder there, such as .../kanata-nicola).
        let is_kanata = |p: &Path| p.file_name().is_some_and(|n| n.to_string_lossy().contains("kanata"));
        let kanata = match std::fs::read_link(format!("/proc/{pid}/exe")) {
            Ok(exe) => is_kanata(&exe),
            Err(_) => args.first().is_some_and(|a| is_kanata(Path::new(a))),
        };
        if !kanata {
            continue;
        }
        let cwd = std::fs::read_link(format!("/proc/{pid}/cwd")).unwrap_or_default();
        let cfgs: Vec<std::path::PathBuf> =
            args.windows(2).filter(|w| w[0] == "-c" || w[0] == "--cfg").map(|w| cwd.join(&w[1])).collect();
        // A running kanata uses the config it read at startup. Do not trust a config file modified after startup.
        let started = process_start_time(pid);
        let safe = !cfgs.is_empty()
            && cfgs.iter().all(|c| {
                let modified = std::fs::metadata(c).and_then(|m| m.modified()).ok();
                // run.sh starts kanata right after writing the config, so allow a few ms of difference (start time has 10ms resolution).
                let after_start = matches!((started, modified), (Some(s), Some(m)) if m > s + Duration::from_secs(2));
                if after_start || started.is_none() {
                    return false;
                }
                std::fs::read_to_string(c).is_ok_and(|t| {
                    let exclude = t.lines().any(|l| l.contains("linux-dev-names-exclude") && l.contains(INPUT_NAME));
                    let include_only =
                        t.lines().any(|l| l.contains("linux-dev-names-include") && !l.contains(INPUT_NAME));
                    exclude || include_only
                })
            });
        if !safe {
            return Err(format!(
                "kanata (pid {pid}) is running and its config does not exclude the E2E virtual keyboard '{INPUT_NAME}'.\n\
                 Aborting, because the injected keys could be typed into the desktop.\n\
                 (If the config file was updated after kanata started, the exclusion cannot be confirmed until it is restarted.)\n\
                 Stop that kanata, or add (linux-dev-names-exclude \"{INPUT_NAME}\") to its config and restart it\n\
                 (configs generated by the scripts in platform/linux/ already include it)."
            )
            .into());
        }
        println!("note: kanata (pid {pid}) is running but excludes '{INPUT_NAME}'; running alongside it");
    }
    Ok(())
}

/// Process start time (computed from starttime in `/proc/<pid>/stat` and btime in `/proc/stat`).
fn process_start_time(pid: &str) -> Option<std::time::SystemTime> {
    let stat = std::fs::read_to_string(format!("/proc/{pid}/stat")).ok()?;
    let rest = &stat[stat.rfind(')')? + 2..];
    let ticks: u64 = rest.split_whitespace().nth(19)?.parse().ok()?;
    let btime: u64 = std::fs::read_to_string("/proc/stat")
        .ok()?
        .lines()
        .find_map(|l| l.strip_prefix("btime ").and_then(|v| v.trim().parse().ok()))?;
    Some(std::time::UNIX_EPOCH + Duration::from_secs(btime) + Duration::from_millis(ticks * 10))
}

fn enumerate_by_name(name: &str) -> Option<Device> {
    evdev::enumerate().map(|(_, d)| d).find(|d| d.name() == Some(name))
}

fn wait_for_device(name: &str, timeout: Duration) -> Res<Device> {
    let start = Instant::now();
    while start.elapsed() < timeout {
        if let Some(d) = enumerate_by_name(name) {
            return Ok(d);
        }
        thread::sleep(Duration::from_millis(50));
    }
    Err(format!("device '{name}' did not appear (kanata failed to start?)").into())
}

#[cfg(test)]
mod tests {
    use super::DeviceLog;

    #[test]
    fn parses_kanata_device_log_lines() {
        let parse = |l: &str| DeviceLog::parse(l);
        assert_eq!(
            parse(r#"06:07:00.5858 [INFO] registering /dev/input/event17: "nicola-e2e-input""#),
            Some((DeviceLog::Registering, "/dev/input/event17".into()))
        );
        assert_eq!(
            parse(r#"06:07:00.5 [WARN] found device /dev/input/event17 but could not register it Os { code: 16 }"#),
            Some((DeviceLog::Failed, "/dev/input/event17".into()))
        );
        assert_eq!(
            parse("06:07:00.7 [WARN] removing kbd device: /dev/input/event17"),
            Some((DeviceLog::Removed, "/dev/input/event17".into()))
        );
        assert_eq!(
            parse("06:07:00.7 [WARN] removing stale kbd device: /dev/input/event17"),
            Some((DeviceLog::Removed, "/dev/input/event17".into()))
        );
        assert_eq!(parse("06:07:00.4 [INFO] device [/dev/input/event17:nicola-e2e-input] is included"), None);
    }
}
