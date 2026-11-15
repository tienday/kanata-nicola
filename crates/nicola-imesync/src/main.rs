//! `nicola-imesync` — makes the NICOLA layer follow the actual IME state (docs/design.md, IME sync; OQ-05).
//!
//! Runs in the user session, reads the fcitx5 state over D-Bus, and sends layer changes to the kanata TCP port.
//!
//! To keep the load low, it reads the state only when fcitx5 sends a change notification (D-Bus signals: any signal
//! of fcitx5, whichever user interface it uses, and those of its tray item and the item's menu; other applications'
//! tray items are filtered out by sender) (docs/design.md, IME sync). For one change, fcitx5 sends several signals at
//! once; one read covers them ([`BURST_GAP`]). In case a notification is missed, it also reads at a low rate
//! (`--fallback-ms`). The log shows which of the two found the change.
//! This replaces toggling the IME and NICOLA separately with a key (docs/design.md, IME sync), which drifts
//! out of step with per-window input state (fcitx5 ShareInputState=No), the mouse, and Mozc's internal switching.
//!
//! NICOLA is ON when all of these hold:
//!   1. an input context has the focus (Controller1.State is not 0)
//!   2. the input method is the one given by `--im`
//!   3. the Mozc input mode is a kana mode (hiragana, katakana). OFF in direct input and alphanumeric modes
//!      (read from the tray menu, or else taken from the signals of fcitx5 [`mode_in_signal`]; if neither tells it,
//!      decided by 1 and 2 only)
//!
//! Not "fcitx5 is active" (State = 2): fcitx5 calls it active when the input method is not the first of the group,
//! so with Mozc first in the list, Mozc is in use while fcitx5 is inactive (State = 1).
//!
//! When fcitx5 is not running, NICOLA is OFF (another failure to read the state keeps the layer once,
//! see [`off_after_error`]). The calls to fcitx5 do not start it through D-Bus activation.
//! When kanata closes the connection (it exited or restarted), this connects again and sends the current
//! layer, since a new kanata starts on its first layer (docs/design.md, resident services).
//!
//! Options: see [`usage`] (`--help`).

use std::collections::BTreeSet;
use std::io::{BufRead, BufReader, Write};
use std::net::{Shutdown, TcpStream};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use dbus::arg::{RefArg, Variant};
use dbus::blocking::stdintf::org_freedesktop_dbus::Properties;
use dbus::blocking::Connection;
use dbus::channel::MatchingReceiver;
use dbus::message::{MatchRule, MessageType};
use dbus::strings::BusName;
use dbus::Message;

type Res<T> = Result<T, Box<dyn std::error::Error>>;

// Defaults of the options (the help shows them from here).
const DEFAULT_PORT: u16 = 23800;
const DEFAULT_FALLBACK_MS: u64 = 1000;
const DEFAULT_IM: &str = "mozc";
const DEFAULT_ON_LAYER: &str = "nicola";
const DEFAULT_OFF_LAYER: &str = "base";

/// `nicola-imesync <version>`: printed by `--version` and logged at the start.
const VERSION_LINE: &str = concat!("nicola-imesync ", env!("CARGO_PKG_VERSION"));

fn usage() -> String {
    format!(
        "\
usage: nicola-imesync [options]
  Start kanata beforehand with `kanata -c <config> -p 127.0.0.1:<port>`.

options:
  --port <n>          kanata TCP port (default {DEFAULT_PORT})
  --fallback-ms <n>   interval for reading the state just in case, when no notification arrives
                      (default {DEFAULT_FALLBACK_MS}; 0 disables)
  --im <name>         input method to use NICOLA with (default {DEFAULT_IM})
  --on-layer <name>   layer when ON (default {DEFAULT_ON_LAYER})
  --off-layer <name>  layer when OFF (default {DEFAULT_OFF_LAYER})
  --dry-run           do not send to kanata; only print state changes
  -h, --help          show this help
  -V, --version       show the version
"
    )
}

struct Opts {
    port: u16,
    fallback: Option<Duration>,
    im: String,
    on_layer: String,
    off_layer: String,
    dry_run: bool,
}

fn parse_args() -> Res<Opts> {
    let mut o = Opts {
        port: DEFAULT_PORT,
        fallback: Some(Duration::from_millis(DEFAULT_FALLBACK_MS)),
        im: DEFAULT_IM.into(),
        on_layer: DEFAULT_ON_LAYER.into(),
        off_layer: DEFAULT_OFF_LAYER.into(),
        dry_run: false,
    };
    let mut args = std::env::args().skip(1);
    while let Some(a) = args.next() {
        let mut val = || args.next().ok_or_else(|| format!("{a} needs a value"));
        match a.as_str() {
            "--port" => o.port = val()?.parse().map_err(|e| format!("--port: {e}"))?,
            "--fallback-ms" => {
                let ms: u64 = val()?.parse().map_err(|e| format!("--fallback-ms: {e}"))?;
                o.fallback = (ms > 0).then(|| Duration::from_millis(ms));
            }
            "--im" => o.im = val()?,
            "--on-layer" => o.on_layer = layer_name(&a, val()?)?,
            "--off-layer" => o.off_layer = layer_name(&a, val()?)?,
            "--dry-run" => o.dry_run = true,
            "-h" | "--help" => {
                print!("{}", usage());
                std::process::exit(0);
            }
            "-V" | "--version" => {
                println!("{VERSION_LINE}");
                std::process::exit(0);
            }
            _ => return Err(format!("unknown argument {a}").into()),
        }
    }
    Ok(o)
}

/// A layer name given with `flag`. It goes into a JSON string to kanata as it is, so it cannot have what would
/// need escaping there (nor can a kanata layer name).
fn layer_name(flag: &str, name: String) -> Res<String> {
    if name.is_empty() || name.chars().any(|c| c == '"' || c == '\\' || c.is_control()) {
        return Err(format!("{flag}: not a layer name: {name:?}").into());
    }
    Ok(name)
}

/// Snapshot of the IME state.
#[derive(Clone, Debug, PartialEq, Eq)]
struct ImeState {
    /// Controller1.State: 0 = no input context; 1 = inactive (the group's first input method), 2 = active (another one)
    state: i32,
    im: String,
    /// Mozc input mode (`hiragana`, `direct`, etc.). None if it cannot be read.
    mozc_mode: Option<String>,
}

impl ImeState {
    fn wants_nicola(&self, im: &str) -> bool {
        let kana_mode = match self.mozc_mode.as_deref() {
            None => true,
            Some(m) => m.starts_with("hiragana") || m.starts_with("katakana"),
        };
        self.state != 0 && self.im == im && kana_mode
    }
}

struct Fcitx {
    conn: Connection,
    /// Bus name of the tray item (StatusNotifierItem) and the path of its menu.
    menu: Option<(String, String)>,
    /// id of the "character type" item in the menu (looked up the first time).
    mode_item: Option<i32>,
    /// After a lookup that did not find that item: not before this time ([`MODE_LOOKUP_INTERVAL`]).
    next_mode_lookup: Option<Instant>,
    /// Match rules for the notifications of the tray item and its menu (renewed with the tray item).
    tray_rules: Vec<String>,
    /// What the signals received tell (recorded by the handler that [`Fcitx::new`] sets up).
    signals: Arc<Mutex<Signals>>,
}

/// What the signals received tell the main loop.
#[derive(Default)]
struct Signals {
    /// Name of the last signal received. The main loop takes it and reads the state.
    last: Option<String>,
    /// A tray item was registered: the main loop looks the fcitx5 tray item up again.
    item_registered: bool,
    /// The Mozc input mode named by the last signal that named one ([`mode_in_signal`]).
    mozc_mode: Option<String>,
}

impl Signals {
    fn record(&mut self, msg: &Message) {
        // Only signals to all (fcitx5 and the tray send those), which the bus passes by the senders of the match rules.
        // A signal addressed to this program would come from any program of the user.
        if msg.destination().is_some() {
            return;
        }
        let name = msg.member().map(|m| m.to_string()).unwrap_or_default();
        if name == ITEM_REGISTERED {
            self.item_registered = true;
        } else if let Some(mode) = mode_in_signal(msg) {
            self.mozc_mode = Some(mode);
        }
        self.last = Some(name);
    }
}

const TIMEOUT: Duration = Duration::from_millis(300);

/// Bus name of fcitx5.
const FCITX5: &str = "org.fcitx.Fcitx5";

/// How long to wait for more messages after a signal, so that the state is read once for the signals that fcitx5
/// sends together for one change (within a millisecond or so; for each key press, some fcitx5 user interfaces send
/// several). It delays the switch by this much after the last of them.
const BURST_GAP: Duration = Duration::from_millis(2);

/// At most this many messages (of any kind, not only signals) are taken in before the read, so that a steady stream
/// does not hold it off (fcitx5 sends some 15 signals for one change). The delay is thus at most
/// `BURST_MAX` × [`BURST_GAP`], reached only while messages keep coming less than [`BURST_GAP`] apart.
const BURST_MAX: usize = 100;

/// How long the item of the Mozc modes is not looked up again after a lookup did not find it (unless a tray
/// item registers): each lookup asks every tray item for its id and reads the whole menu, with AboutToShow.
/// Short, as the mode is unknown meanwhile (conditions 1 and 2 decide).
const MODE_LOOKUP_INTERVAL: Duration = Duration::from_secs(5);

/// Name of the Mozc input method in fcitx5. Its input mode can be read from its icons ([`Fcitx::mozc_mode`]).
const MOZC_IM: &str = "mozc";

/// Signal of the tray host when an application (re)registers a tray item. fcitx5 registers its item
/// again under a new bus name when it restarts, the tray host comes back (e.g. after unlocking), or fcitx5 goes back
/// to the tray from another user interface (an input panel of the desktop).
const ITEM_REGISTERED: &str = "StatusNotifierItemRegistered";

/// The tray host (its bus name and interface; it lists the tray items).
const TRAY_WATCHER: &str = "org.kde.StatusNotifierWatcher";
/// Interface of a tray item.
const TRAY_ITEM: &str = "org.kde.StatusNotifierItem";
/// Interface of the menu of a tray item.
const TRAY_MENU: &str = "com.canonical.dbusmenu";

/// A menu item in a reply of `GetLayout`: its id, properties and children.
type Node = (i32, dbus::arg::PropMap, Vec<Variant<Box<dyn RefArg>>>);

impl Fcitx {
    fn new() -> Res<Self> {
        Self::with_connection(Connection::new_session()?)
    }

    /// On a given bus connection (the tests use a bus of their own).
    fn with_connection(conn: Connection) -> Res<Self> {
        let mut f = Self {
            conn,
            menu: None,
            mode_item: None,
            next_mode_lookup: None,
            tray_rules: Vec::new(),
            signals: Arc::default(),
        };
        // One handler for the signals of all the match rules below. With a handler per rule, the library would give a
        // signal to the first rule that matches it only, and in the library, a rule with a well-known sender name
        // (FCITX5) matches the signals of every sender (the bus does that check).
        let s = Arc::clone(&f.signals);
        f.conn.start_receive(
            MatchRule::new().with_type(MessageType::Signal),
            Box::new(move |msg, _| {
                s.lock().unwrap().record(&msg);
                true
            }),
        );
        let registered = MatchRule::new_signal(TRAY_WATCHER, ITEM_REGISTERED).with_sender(TRAY_WATCHER);
        f.conn.add_match_no_cb(&registered.match_str())?;
        // Any signal of fcitx5, whatever its interface: what it sends depends on the user interface it uses. The bus
        // matches the name to its current owner, also after fcitx5 restarts.
        f.conn.add_match_no_cb(&MatchRule::new().with_type(MessageType::Signal).with_sender(FCITX5).match_str())?;
        f.find_menu();
        Ok(f)
    }

    /// The name of the last signal received, if any, once the messages that follow it closely ([`BURST_GAP`], at
    /// most [`BURST_MAX`]) are processed too. Looks the fcitx5 tray item up again if a tray item was registered.
    fn take_signal(&mut self) -> Result<Option<String>, dbus::Error> {
        if self.signals.lock().unwrap().last.is_none() {
            return Ok(None);
        }
        for _ in 0..BURST_MAX {
            if !self.conn.process(BURST_GAP)? {
                break;
            }
        }
        let (last, registered) = {
            let mut s = self.signals.lock().unwrap();
            (s.last.take(), std::mem::take(&mut s.item_registered))
        };
        if registered {
            self.find_menu();
        }
        Ok(last)
    }

    /// Calls a Controller1 method without D-Bus activation: the session bus would otherwise start fcitx5
    /// from its D-Bus service file when it is not running (for example after the user quit it).
    fn controller<R: dbus::arg::Arg + for<'a> dbus::arg::Get<'a>>(&self, method: &str) -> Result<R, dbus::Error> {
        let mut msg = Message::new_method_call(FCITX5, "/controller", "org.fcitx.Fcitx.Controller1", method)
            .map_err(|e| dbus::Error::new_failed(&e))?;
        msg.set_auto_start(false);
        let reply = self.conn.channel().send_with_reply_and_block(msg, TIMEOUT)?;
        reply.read1().map_err(|e| dbus::Error::new_failed(&e.to_string()))
    }

    fn read(&mut self) -> Result<ImeState, dbus::Error> {
        let state: i32 = self.controller("State")?;
        let im: String = self.controller("CurrentInputMethod")?;
        // Other input methods have no such mode; conditions 1 and 2 decide for them.
        let mozc_mode = if im == MOZC_IM { self.mozc_mode() } else { None };
        Ok(ImeState { state, im, mozc_mode })
    }

    /// Finds the fcitx5 tray item, remembers its menu path, and subscribes to its notifications.
    fn find_menu(&mut self) {
        let found = self.lookup_menu();
        if found == self.menu {
            return;
        }
        self.menu = found;
        self.mode_item = None;
        self.next_mode_lookup = None;
        for rule in self.tray_rules.drain(..) {
            let _ = self.conn.remove_match_no_cb(&rule);
        }
        let Some((dest, _)) = &self.menu else { return };
        let Ok(sender) = BusName::new(dest.clone()) else { return };
        for iface in [TRAY_ITEM, TRAY_MENU] {
            let rule = MatchRule::new()
                .with_type(MessageType::Signal)
                .with_interface(iface)
                .with_sender(sender.clone())
                .match_str();
            match self.conn.add_match_no_cb(&rule) {
                Ok(()) => self.tray_rules.push(rule),
                Err(e) => eprintln!("cannot subscribe to the notifications of the fcitx5 tray item: {e}"),
            }
        }
    }

    fn lookup_menu(&self) -> Option<(String, String)> {
        let watcher = self.conn.with_proxy(TRAY_WATCHER, "/StatusNotifierWatcher", TIMEOUT);
        let Ok(items) = watcher.get::<Vec<String>>(TRAY_WATCHER, "RegisteredStatusNotifierItems") else {
            return None;
        };
        for item in items {
            // An item of another program that is not a valid name is skipped (with_proxy would panic on it).
            let Some((dest, path)) = tray_item_address(&item) else { continue };
            let sni = self.conn.with_proxy(dest.as_str(), path.as_str(), TIMEOUT);
            let id: Result<String, _> = sni.get(TRAY_ITEM, "Id");
            if id.ok().as_deref() != Some("Fcitx") {
                continue;
            }
            if let Ok(menu) = sni.get::<dbus::Path>(TRAY_ITEM, "Menu") {
                return Some((dest, menu.to_string()));
            }
        }
        None
    }

    /// The Mozc input mode: read from the tray menu, or else the one that the signals of fcitx5 named last.
    fn mozc_mode(&mut self) -> Option<String> {
        self.tray_mode().or_else(|| self.signals.lock().unwrap().mozc_mode.clone())
    }

    /// Reads the Mozc input mode from the tray menu: the item whose children are the modes (one is the direct
    /// mode). fcitx5-mozc 2.26 shows the current mode in that item's icon; 2.29 (Ubuntu 26.04) marks the
    /// selected child (a radio item) and names the icons with _ instead of - ([`mode_of`]).
    fn tray_mode(&mut self) -> Option<String> {
        // Without a tray item, it is looked up again when one is registered (ITEM_REGISTERED).
        self.menu.as_ref()?;
        if let Some(mode) = self.mode_item.and_then(|id| self.read_mode_icon(id)) {
            return Some(mode);
        }
        // Not looked up yet, or the menu was rebuilt, or the tray item went away (fcitx5 restarted, or it
        // registered the item again under a new name): look the item up in the menu, then once more after
        // looking the tray item up again, so no stale reference survives long-running use. If neither finds it
        // (a menu of another form), not again for a while: see MODE_LOOKUP_INTERVAL.
        if self.next_mode_lookup.is_some_and(|t| Instant::now() < t) {
            return None;
        }
        for again in [false, true] {
            if again {
                self.find_menu();
            }
            self.mode_item = self.find_mode_item();
            if let Some(mode) = self.mode_item.and_then(|id| self.read_mode_icon(id)) {
                self.next_mode_lookup = None;
                return Some(mode);
            }
        }
        self.next_mode_lookup = Some(Instant::now() + MODE_LOOKUP_INTERVAL);
        None
    }

    /// The current mode, from the item of the modes (`id`) and its children.
    fn read_mode_icon(&self, id: i32) -> Option<String> {
        let (dest, path) = self.menu.as_ref()?;
        let menu = self.conn.with_proxy(dest.as_str(), path.as_str(), TIMEOUT);
        // The item and its children (fcitx5 does not implement GetProperty). The toggle states are current
        // without AboutToShow, which would make fcitx5 send LayoutUpdated, and that would make us read again.
        let r: Result<(u32, Node), _> =
            menu.method_call(TRAY_MENU, "GetLayout", (id, 1i32, vec!["icon-name", "toggle-state"]));
        let (_rev, (_id, props, children)) = r.ok()?;
        let parent_icon = props.get("icon-name").and_then(|v| v.0.as_str().map(String::from));
        let kids: Vec<MenuItem> = children.iter().filter_map(|c| item(c as &dyn RefArg)).collect();
        let kids: Vec<(Option<&str>, bool)> = kids.iter().map(|k| (k.icon.as_deref(), k.toggled)).collect();
        mode_of(parent_icon.as_deref(), &kids)
    }

    fn find_mode_item(&self) -> Option<i32> {
        let (dest, path) = self.menu.clone()?;
        let menu = self.conn.with_proxy(dest.as_str(), path.as_str(), TIMEOUT);
        let _: Result<(bool,), _> = menu.method_call(TRAY_MENU, "AboutToShow", (0i32,));
        let r: Result<(u32, Node), _> = menu.method_call(TRAY_MENU, "GetLayout", (0i32, -1i32, Vec::<String>::new()));
        let (_rev, root) = r.ok()?;
        find_in(&root.2)
    }
}

/// Whether NICOLA goes OFF after a failure to read the fcitx5 state (`consecutive` failures so far).
/// fcitx5 not running (its name has no owner): at once, as there is no IME to type kana into.
/// Any other failure (e.g. a time-out while fcitx5 is busy): only if it happens again.
fn off_after_error(error_name: Option<&str>, consecutive: u32) -> bool {
    let absent = matches!(
        error_name,
        Some("org.freedesktop.DBus.Error.ServiceUnknown" | "org.freedesktop.DBus.Error.NameHasNoOwner")
    );
    absent || consecutive >= 2
}

/// The Mozc input mode that a signal names, if it names exactly one: an icon name of a mode ([`mode_name`]) as a word
/// in the strings of its arguments. fcitx5 sends the icon of the current input method, which shows the Mozc mode, to
/// a user interface that is not the tray (e.g. an input panel of the desktop), so the mode is known without the tray.
/// A signal that names several modes lists them (e.g. a menu of the modes) and does not tell the current one.
/// The strings can hold the text being typed (an input panel shows it): only the mode is kept, and nothing is logged.
/// That text, or a candidate, that holds an icon name as a word is taken as the mode too, until a signal names
/// another (a known limit, docs/implementation.md §4; matching the form of what fcitx5 sends would depend on one
/// user interface).
fn mode_in_signal(msg: &Message) -> Option<String> {
    fn strings(arg: &dyn RefArg, out: &mut Vec<String>) {
        if let Some(s) = arg.as_str() {
            out.push(s.to_string());
        } else if let Some(items) = arg.as_iter() {
            items.for_each(|a| strings(a, out));
        }
    }
    let mut all = Vec::new();
    msg.iter_init().for_each(|arg| strings(&*arg, &mut all));
    mode_in(all.iter().map(String::as_str))
}

/// The Mozc input mode named by icon names ([`mode_name`]) as words in `strings`, if they name exactly one.
fn mode_in<'a>(strings: impl Iterator<Item = &'a str>) -> Option<String> {
    let in_word = |c: char| c.is_ascii_alphanumeric() || c == '-' || c == '_';
    let mut modes: BTreeSet<String> =
        strings.flat_map(|s| s.split(move |c| !in_word(c))).filter_map(mode_name).collect();
    if modes.len() == 1 {
        modes.pop_first()
    } else {
        None
    }
}

/// Unwraps nested variants.
fn strip_variants(arg: &dyn RefArg) -> &dyn RefArg {
    if arg.arg_type() == dbus::arg::ArgType::Variant {
        if let Some(inner) = arg.as_iter().and_then(|mut it| it.next()) {
            return strip_variants(inner);
        }
    }
    arg
}

/// The bus name and object path of a registered tray item: "busname", "busname/path", or "busname@path"
/// (the form of Ubuntu's AppIndicator extension of GNOME Shell, e.g. ":1.53@/org/ayatana/NotificationItem/x").
/// None if either part is not valid for D-Bus.
fn tray_item_address(item: &str) -> Option<(String, String)> {
    let (dest, path) = match (item.find('@'), item.find('/')) {
        (Some(at), Some(slash)) if at < slash => (&item[..at], &item[at + 1..]),
        (_, Some(slash)) => (&item[..slash], &item[slash..]),
        (_, None) => (item, "/StatusNotifierItem"),
    };
    BusName::new(dest).ok()?;
    dbus::Path::new(path).ok()?;
    Some((dest.to_string(), path.to_string()))
}

/// An item of the tray menu.
struct MenuItem<'a> {
    id: i32,
    icon: Option<String>,
    /// A selected radio item (toggle-state 1).
    toggled: bool,
    kids: Vec<&'a dyn RefArg>,
}

/// Reads a menu item (id, properties, children).
fn item(arg: &dyn RefArg) -> Option<MenuItem<'_>> {
    let mut it = strip_variants(arg).as_iter()?;
    let id = it.next()?.as_i64()? as i32;
    let props = it.next()?;
    let (mut icon, mut toggled) = (None, false);
    let mut p = props.as_iter()?;
    while let (Some(k), Some(v)) = (p.next(), p.next()) {
        match k.as_str() {
            Some("icon-name") => icon = strip_variants(v).as_str().map(String::from),
            Some("toggle-state") => toggled = strip_variants(v).as_i64() == Some(1),
            _ => {}
        }
    }
    let kids = it.next()?.as_iter()?.collect();
    Some(MenuItem { id, icon, toggled, kids })
}

/// The Mozc input mode that an icon name stands for ("hiragana", "katakana-full", "alpha-half", "direct", ...):
/// fcitx-mozc-hiragana (fcitx5-mozc 2.26 and 2.28) or fcitx_mozc_hiragana (2.29). None for other icons (the tool).
fn mode_name(icon: &str) -> Option<String> {
    let mode = icon.replace('_', "-").strip_prefix("fcitx-mozc-")?.to_string();
    let known = ["direct", "hiragana", "katakana", "alpha"].iter().any(|m| mode.starts_with(m));
    known.then_some(mode)
}

/// The current mode, from the item of the modes (its icon) and its children (icon, selected): the selected
/// child (2.29), or else the item's own icon (2.26).
fn mode_of(item_icon: Option<&str>, kids: &[(Option<&str>, bool)]) -> Option<String> {
    let selected = kids.iter().find(|(_, toggled)| *toggled).and_then(|(icon, _)| *icon);
    selected.and_then(mode_name).or_else(|| item_icon.and_then(mode_name))
}

/// Finds the id of the item whose children are the Mozc modes (one of them the direct mode).
fn find_in(children: &[Variant<Box<dyn RefArg>>]) -> Option<i32> {
    let refs: Vec<&dyn RefArg> = children.iter().map(|v| v as &dyn RefArg).collect();
    find_in_refs(&refs)
}

fn find_in_refs(children: &[&dyn RefArg]) -> Option<i32> {
    for c in children {
        let Some(it) = item(*c) else { continue };
        let has_direct = it
            .kids
            .iter()
            .filter_map(|k| item(*k))
            .any(|k| k.icon.as_deref().and_then(mode_name).as_deref() == Some("direct"));
        if has_direct {
            return Some(it.id);
        }
        if let Some(found) = find_in_refs(&it.kids) {
            return Some(found);
        }
    }
    None
}

/// Connection to the kanata TCP server.
struct Kanata {
    port: u16,
    conn: Option<KanataConn>,
    /// The layer sent on the current connection.
    sent: Option<String>,
}

struct KanataConn {
    stream: TcpStream,
    /// Set when kanata closes the connection (kanata exited or restarted).
    closed: Arc<AtomicBool>,
}

impl Kanata {
    fn new(port: u16) -> Self {
        Self { port, conn: None, sent: None }
    }

    /// Whether kanata closed the connection since the last [`sync`](Self::sync).
    fn lost(&self) -> bool {
        self.conn.as_ref().is_some_and(|c| c.closed.load(Ordering::Relaxed))
    }

    fn connected(&self) -> bool {
        self.conn.is_some() && !self.lost()
    }

    /// Makes kanata's layer `layer`. Sends it unless it was already sent on the current connection.
    /// A new connection always gets it: a kanata that restarted is back on its first layer.
    fn sync(&mut self, layer: &str) -> Res<()> {
        if self.lost() {
            self.conn = None;
        }
        if self.conn.is_none() {
            self.sent = None;
            self.conn = Some(KanataConn::connect(self.port)?);
        }
        if self.sent.as_deref() == Some(layer) {
            return Ok(());
        }
        let msg = format!("{{\"ChangeLayer\":{{\"new\":\"{layer}\"}}}}\n");
        let conn = self.conn.as_mut().expect("connected above");
        if let Err(e) = conn.stream.write_all(msg.as_bytes()) {
            self.conn = None;
            return Err(e.into());
        }
        self.sent = Some(layer.to_string());
        Ok(())
    }
}

impl KanataConn {
    fn connect(port: u16) -> Res<Self> {
        let stream = TcpStream::connect(("127.0.0.1", port))?;
        stream.set_nodelay(true)?;
        let closed = Arc::new(AtomicBool::new(false));
        // Read and discard notifications from kanata (LayerChange, etc.); otherwise kanata's send buffer
        // fills up. The end of the stream means kanata closed the connection.
        let reader = stream.try_clone()?;
        let flag = Arc::clone(&closed);
        std::thread::spawn(move || {
            for line in BufReader::new(reader).lines() {
                if line.is_err() {
                    break;
                }
            }
            flag.store(true, Ordering::Relaxed);
        });
        Ok(Self { stream, closed })
    }
}

impl Drop for KanataConn {
    fn drop(&mut self) {
        // Ends the reader thread too.
        let _ = self.stream.shutdown(Shutdown::Both);
    }
}

fn main() {
    if let Err(e) = run() {
        eprintln!("nicola-imesync: {e}");
        std::process::exit(1);
    }
}

/// How often to retry when kanata cannot be reached and no fallback read is set, and the longest
/// sleep before checking whether kanata closed the connection.
const RETRY: Duration = Duration::from_secs(1);

fn run() -> Res<()> {
    let o = parse_args()?;
    // Which version runs, for the log (journalctl, systemctl status).
    println!("{VERSION_LINE}");
    let mut fcitx = Fcitx::new()?;
    if fcitx.menu.is_none() {
        eprintln!(
            "note: fcitx5 tray item not found; the Mozc input mode is read once it appears, or taken from the signals \
             of fcitx5"
        );
    }
    let mut kanata = Kanata::new(o.port);
    let start = Instant::now();
    let mut last_state: Option<ImeState> = None;
    let mut cause = "start".to_string();
    // Do not repeat the same error (so the log does not flood when running as a service, e.g. before fcitx5 starts).
    let mut last_error = String::new();
    // Whether the last send to kanata succeeded (None before the first success), to log changes only.
    let mut kanata_ok: Option<bool> = None;
    let mut last_want: Option<&String> = None;
    let mut read_failures = 0;
    loop {
        let want = match fcitx.read() {
            Ok(st) => {
                let want = if st.wants_nicola(&o.im) { &o.on_layer } else { &o.off_layer };
                last_error.clear();
                read_failures = 0;
                if last_state.as_ref() != Some(&st) {
                    println!(
                        "{:>8.3}s  fcitx5={} im={} mozc={} -> {}  (by {cause})",
                        start.elapsed().as_secs_f64(),
                        st.state,
                        st.im,
                        st.mozc_mode.as_deref().unwrap_or("-"),
                        want
                    );
                    last_state = Some(st);
                }
                want
            }
            Err(e) => {
                read_failures += 1;
                let keep = last_want.filter(|_| !off_after_error(e.name(), read_failures));
                let msg = format!(
                    "cannot read the fcitx5 state ({}): {} -> {}",
                    e.name().unwrap_or("?"),
                    e.message().unwrap_or(""),
                    if keep.is_some() { "keeping the layer" } else { o.off_layer.as_str() }
                );
                if msg != last_error {
                    eprintln!("{msg}");
                    last_error = msg;
                }
                if keep.is_none() {
                    last_state = None;
                }
                keep.unwrap_or(&o.off_layer)
            }
        };
        last_want = Some(want);
        if !o.dry_run {
            if kanata.lost() && kanata_ok == Some(true) {
                eprintln!("kanata closed the connection (it exited or restarted); connecting again");
                kanata_ok = Some(false);
            }
            match kanata.sync(want) {
                Ok(()) => {
                    if kanata_ok != Some(true) {
                        println!("connected to kanata (port {}); layer {want}", o.port);
                    }
                    kanata_ok = Some(true);
                }
                Err(e) => {
                    if kanata_ok == Some(true) {
                        eprintln!("failed to send to kanata (is kanata running with -p 127.0.0.1:{}?): {e}", o.port);
                    }
                    kanata_ok = Some(false);
                }
            }
        }
        // Sleep until the next notification. Without the fallback read, still retry once a second while
        // kanata cannot be reached.
        let wait = match o.fallback {
            Some(f) => f,
            None if !o.dry_run && !kanata.connected() => RETRY,
            None => Duration::from_secs(3600),
        };
        let deadline = Instant::now() + wait;
        cause = loop {
            let left = deadline.saturating_duration_since(Instant::now());
            if left.is_zero() {
                break "poll".to_string();
            }
            // At least 1 ms: the library waits whole milliseconds, so less would not wait and the loop would spin.
            fcitx.conn.process(left.clamp(Duration::from_millis(1), RETRY))?;
            if let Some(sig) = fcitx.take_signal()? {
                break format!("signal {sig}");
            }
            if kanata.lost() {
                break "kanata reconnect".to_string();
            }
        };
    }
}

#[cfg(test)]
mod tests {
    use std::io::{BufRead, BufReader};
    use std::net::{TcpListener, TcpStream};
    use std::path::PathBuf;
    use std::process::{Child, Command, Stdio};
    use std::time::{Duration, Instant};

    use dbus::arg::{PropMap, RefArg, Variant};
    use dbus::blocking::Connection;
    use dbus::channel::Channel;
    use dbus::strings::BusName;
    use dbus::Message;

    use super::{
        find_in, layer_name, mode_in, mode_in_signal, mode_name, mode_of, off_after_error, tray_item_address, Fcitx,
        ImeState, Kanata, Signals, FCITX5, ITEM_REGISTERED, TRAY_WATCHER,
    };

    /// The Mozc mode from the tray menu of fcitx5-mozc 2.26 (the item's icon) and 2.29 (Ubuntu 26.04: the
    /// selected child, icons named with _).
    #[test]
    fn mozc_mode_from_the_tray_menu() {
        let m = |s: &str| Some(s.to_string());
        assert_eq!(mode_name("fcitx-mozc-hiragana"), m("hiragana"));
        assert_eq!(mode_name("fcitx_mozc_katakana_full"), m("katakana-full"));
        assert_eq!(mode_name("fcitx_mozc_alpha_half"), m("alpha-half"));
        assert_eq!(mode_name("fcitx_mozc_direct"), m("direct"));
        assert_eq!(mode_name("fcitx_mozc_tool"), None);
        assert_eq!(mode_name("input-keyboard"), None);
        // 2.26: the item's icon is the mode.
        assert_eq!(mode_of(Some("fcitx-mozc-hiragana"), &[(Some("fcitx-mozc-direct"), false)]), m("hiragana"));
        // 2.29: the item's icon is the tool; the selected child is the mode.
        let kids = [(Some("fcitx_mozc_direct"), false), (Some("fcitx_mozc_hiragana"), true)];
        assert_eq!(mode_of(Some("fcitx_mozc_tool"), &kids), m("hiragana"));
        let kids = [(Some("fcitx_mozc_direct"), true), (Some("fcitx_mozc_hiragana"), false)];
        assert_eq!(mode_of(Some("fcitx_mozc_tool"), &kids), m("direct"));
        // Nothing selected and no mode in the item's icon: unknown.
        assert_eq!(mode_of(Some("fcitx_mozc_tool"), &[(Some("fcitx_mozc_direct"), false)]), None);
    }

    /// The Mozc mode from the strings of a signal: what fcitx5 5.1 with fcitx5-mozc 2.28 and 2.29 sends to the input
    /// panel of the desktop (GNOME's Input Method Panel extension) when the input method, the mode, or the focus
    /// changes.
    #[test]
    fn mozc_mode_from_the_strings_of_a_signal() {
        let m = |s: &str| Some(s.to_string());
        let mode = |strings: &[&str]| mode_in(strings.iter().copied());
        let tool = "/Fcitx/mozc-tool:Mozc の設定:fcitx-mozc-tool:Mozc の設定:menu";
        assert_eq!(mode(&["/Fcitx/im:Mozc:fcitx-mozc-hiragana:全角かな:menu,label=あ", tool]), m("hiragana"));
        assert_eq!(mode(&["/Fcitx/im:Mozc:fcitx-mozc-katakana-full:全角カナ:menu,label=あ"]), m("katakana-full"));
        assert_eq!(mode(&["/Fcitx/im:Mozc:fcitx-mozc-direct:直接入力:menu,label=あ"]), m("direct"));
        // Icons named with _ (fcitx5-mozc 2.29).
        assert_eq!(mode(&["/Fcitx/im:Mozc:fcitx_mozc_alpha_half:Half ASCII:menu,label=あ"]), m("alpha-half"));
        // Another input method, the tool, the input method's own icon, and text that is not an icon name.
        assert_eq!(mode(&["/Fcitx/im:キーボード - 日本語:input-keyboard-symbolic::menu,label=ja"]), None);
        assert_eq!(mode(&[tool, "/Fcitx/im/mozc:Mozc:fcitx-mozc::", "[Tabキーで選択]", ""]), None);
        assert_eq!(mode(&["/Fcitx/mozc-mode-hiragana:全角かな:x:全角かな:"]), None);
        // The menu of the modes names them all: not the current one.
        let menu = [
            "/Fcitx/mozc-mode-direct:直接入力:fcitx-mozc-direct:直接入力:",
            "/Fcitx/mozc-mode-hiragana:全角かな:fcitx-mozc-hiragana:全角かな:",
        ];
        assert_eq!(mode(&menu), None);
        // The same mode twice is still one.
        assert_eq!(mode(&["a:fcitx-mozc-hiragana:b", "fcitx-mozc-hiragana"]), m("hiragana"));
        // A known limit: text being typed (the preedit text and its attributes) that holds an icon name is taken as
        // the mode.
        assert_eq!(mode(&["fcitx-mozc-direct", ""]), m("direct"));
    }

    /// The strings of a signal are found in its arguments of any type (arrays, variants, dictionaries).
    #[test]
    fn strings_of_a_signal_in_any_argument() {
        let signal = || Message::new_signal("/panel", "org.example.Panel", "Update").unwrap();
        let msg = signal().append2(vec!["x".to_string(), "/a:Mozc:fcitx-mozc-katakana-full:b".to_string()], 1i32);
        assert_eq!(mode_in_signal(&msg).as_deref(), Some("katakana-full"));
        let mut props = PropMap::new();
        props.insert("icon".into(), Variant(Box::new("fcitx_mozc_direct".to_string()) as Box<dyn RefArg>));
        assert_eq!(mode_in_signal(&signal().append1(props)).as_deref(), Some("direct"));
        assert_eq!(mode_in_signal(&signal().append1(Variant("fcitx-mozc-hiragana"))).as_deref(), Some("hiragana"));
        assert_eq!(mode_in_signal(&signal().append2(7u32, true)), None);
        assert_eq!(mode_in_signal(&signal()), None);
    }

    /// Signals to all are recorded; one addressed to this program is not (any program of the user could send it).
    #[test]
    fn only_signals_to_all_are_recorded() {
        let signal =
            || Message::new_signal("/panel", "org.example.Panel", "Update").unwrap().append1("fcitx-mozc-direct");
        let mut s = Signals::default();
        let mut to_us = signal();
        to_us.set_destination(Some(":1.42".into()));
        s.record(&to_us);
        assert_eq!((s.last.as_deref(), s.mozc_mode.as_deref()), (None, None));
        s.record(&signal());
        assert_eq!((s.last.as_deref(), s.mozc_mode.as_deref()), (Some("Update"), Some("direct")));
        let registered = Message::new_signal("/w", super::TRAY_WATCHER, super::ITEM_REGISTERED).unwrap();
        s.record(&registered);
        assert!(s.item_registered);
    }

    /// The item of the Mozc modes in a menu shaped like the reply of GetLayout: the one with a child that has the
    /// icon of the direct mode, also when nested.
    #[test]
    fn item_of_the_modes_in_the_tray_menu() {
        fn node(id: i32, icon: &str, kids: Vec<Variant<Box<dyn RefArg>>>) -> Variant<Box<dyn RefArg>> {
            let mut props = PropMap::new();
            props.insert("icon-name".into(), Variant(Box::new(icon.to_string()) as Box<dyn RefArg>));
            Variant(Box::new((id, props, kids)) as Box<dyn RefArg>)
        }
        let modes = node(
            5,
            "fcitx_mozc_tool",
            vec![node(6, "fcitx_mozc_direct", vec![]), node(7, "fcitx_mozc_hiragana", vec![])],
        );
        let menu =
            vec![node(1, "input-keyboard", vec![]), node(2, "", vec![node(3, "fcitx_mozc_tool", vec![]), modes])];
        assert_eq!(find_in(&menu), Some(5));
        // No child with the direct mode: not found (the mode is then not read; conditions 1 and 2 decide).
        assert_eq!(find_in(&[node(1, "fcitx_mozc_tool", vec![node(2, "fcitx_mozc_hiragana", vec![])])]), None);
    }

    /// The forms of a registered tray item; what is not valid for D-Bus is skipped instead of making the
    /// D-Bus library panic (Ubuntu 26.04's GNOME lists ":1.53@/org/ayatana/NotificationItem/...").
    #[test]
    fn tray_item_addresses() {
        let a = |item: &str| tray_item_address(item);
        let pair = |d: &str, p: &str| Some((d.to_string(), p.to_string()));
        assert_eq!(a(":1.53@/org/ayatana/NotificationItem/x"), pair(":1.53", "/org/ayatana/NotificationItem/x"));
        assert_eq!(a(":1.7/StatusNotifierItem"), pair(":1.7", "/StatusNotifierItem"));
        assert_eq!(
            a("org.kde.StatusNotifierItem-12-1"),
            pair("org.kde.StatusNotifierItem-12-1", "/StatusNotifierItem")
        );
        // A path that has @ in it is the path's own.
        assert_eq!(a(":1.7/a@b"), None);
        for bad in ["", ":1.53@", "@/x", ":1.53@not-a-path", "bad name/x"] {
            assert_eq!(a(bad), None, "{bad:?}");
        }
    }

    fn st(state: i32, im: &str, mode: Option<&str>) -> ImeState {
        ImeState { state, im: im.into(), mozc_mode: mode.map(String::from) }
    }

    #[test]
    fn nicola_only_when_mozc_in_kana_mode() {
        assert!(st(2, "mozc", Some("hiragana")).wants_nicola("mozc"));
        assert!(st(2, "mozc", Some("katakana-full")).wants_nicola("mozc"));
        // If the mode cannot be read, decide by the fcitx5 state alone.
        assert!(st(2, "mozc", None).wants_nicola("mozc"));
        // Mozc first in the input method list: fcitx5 is inactive (1) while Mozc is in use, and active (2) with the
        // keyboard. Whichever the order, it is the input method and its mode that count.
        assert!(st(1, "mozc", Some("hiragana")).wants_nicola("mozc"));
        assert!(!st(2, "keyboard-jp", None).wants_nicola("mozc"));
        assert!(!st(1, "keyboard-jp", None).wants_nicola("mozc"));
        // No input context (e.g. the desktop has the focus).
        assert!(!st(0, "", None).wants_nicola("mozc"));
        // Mozc's own direct-input and alphanumeric modes.
        assert!(!st(2, "mozc", Some("direct")).wants_nicola("mozc"));
        assert!(!st(2, "mozc", Some("alpha-half")).wants_nicola("mozc"));
        assert!(!st(2, "mozc", Some("alpha-full")).wants_nicola("mozc"));
    }

    #[test]
    fn layer_names_go_into_json_as_they_are() {
        assert_eq!(layer_name("--on-layer", "nicola".into()).unwrap(), "nicola");
        for bad in ["", "a\"b", "a\\b", "a\nb"] {
            assert!(layer_name("--on-layer", bad.into()).is_err(), "{bad:?}");
        }
    }

    #[test]
    fn off_at_once_without_fcitx5_otherwise_on_a_repeated_failure() {
        assert!(off_after_error(Some("org.freedesktop.DBus.Error.ServiceUnknown"), 1));
        assert!(off_after_error(Some("org.freedesktop.DBus.Error.NameHasNoOwner"), 1));
        assert!(!off_after_error(Some("org.freedesktop.DBus.Error.NoReply"), 1));
        assert!(off_after_error(Some("org.freedesktop.DBus.Error.NoReply"), 2));
        assert!(!off_after_error(None, 1));
    }

    fn read_line(s: &TcpStream) -> String {
        s.set_read_timeout(Some(Duration::from_secs(2))).unwrap();
        let mut line = String::new();
        BufReader::new(s).read_line(&mut line).unwrap();
        line.trim_end().to_string()
    }

    /// A kanata that restarted is back on its first layer, so the layer is sent again.
    #[test]
    fn layer_is_sent_again_to_a_restarted_kanata() {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let port = listener.local_addr().unwrap().port();
        let mut kanata = Kanata::new(port);
        kanata.sync("nicola").unwrap();
        let (conn, _) = listener.accept().unwrap();
        assert_eq!(read_line(&conn), r#"{"ChangeLayer":{"new":"nicola"}}"#);
        // Not sent again on the same connection.
        kanata.sync("nicola").unwrap();

        // kanata exits: the connection closes. A new kanata listens on the same port.
        drop(conn);
        drop(listener);
        let t0 = Instant::now();
        while !kanata.lost() {
            assert!(t0.elapsed() < Duration::from_secs(2), "closed connection not noticed");
            std::thread::sleep(Duration::from_millis(10));
        }
        assert!(kanata.sync("nicola").is_err(), "no kanata listening yet");
        let listener = TcpListener::bind(("127.0.0.1", port)).unwrap();
        kanata.sync("nicola").unwrap();
        let (conn, _) = listener.accept().unwrap();
        assert_eq!(read_line(&conn), r#"{"ChangeLayer":{"new":"nicola"}}"#);
        kanata.sync("base").unwrap();
        assert_eq!(read_line(&conn), r#"{"ChangeLayer":{"new":"base"}}"#);
    }

    /// The configuration of a bus of a test's own: no services to start, any connection may own a name, send and
    /// receive. The socket goes to `{dir}`.
    const BUS_CONFIG: &str = r#"<!DOCTYPE busconfig PUBLIC "-//freedesktop//DTD D-Bus Bus Configuration 1.0//EN"
 "http://www.freedesktop.org/standards/dbus/1.0/busconfig.dtd">
<busconfig>
  <type>session</type>
  <listen>unix:dir={dir}</listen>
  <auth>EXTERNAL</auth>
  <policy context="default">
    <allow send_destination="*"/>
    <allow receive_sender="*"/>
    <allow own="*"/>
  </policy>
</busconfig>
"#;

    /// An interface of the input panel's signals in the tests: nicola-imesync takes signals of any interface.
    const PANEL: &str = "org.example.Panel";

    /// A dbus-daemon of a test's own (not the session bus), stopped when dropped. Its configuration and socket are in a
    /// directory of its own, removed when dropped: the daemon is killed, so it cannot remove its socket. The directory
    /// is in the user's runtime directory (`XDG_RUNTIME_DIR`): only the user can use it, whereas in the shared /tmp
    /// another user could prepare the predictable path (with a link in it, for example). Its path is also short enough
    /// for a socket.
    struct TestBus {
        daemon: Child,
        address: String,
        dir: PathBuf,
    }

    impl TestBus {
        /// None where dbus-daemon is not installed or `XDG_RUNTIME_DIR` is not set: the test is then skipped.
        fn start(name: &str) -> Option<Self> {
            let Some(runtime) = std::env::var_os("XDG_RUNTIME_DIR") else {
                eprintln!("skipped: XDG_RUNTIME_DIR is not set");
                return None;
            };
            let dir = PathBuf::from(runtime).join(format!("nicola-imesync-test-{}-{name}", std::process::id()));
            std::fs::create_dir_all(&dir).unwrap();
            let config = dir.join("bus.conf");
            if let Err(e) = std::fs::write(&config, BUS_CONFIG.replace("{dir}", &dir.display().to_string())) {
                let _ = std::fs::remove_dir_all(&dir);
                panic!("cannot write {}: {e}", config.display());
            }
            // Its errors go to the test's output, so that a failure to start says why.
            let spawned = Command::new("dbus-daemon")
                .arg(format!("--config-file={}", config.display()))
                .args(["--nofork", "--print-address"])
                .stdout(Stdio::piped())
                .spawn();
            let daemon = match spawned {
                Ok(daemon) => daemon,
                Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
                    let _ = std::fs::remove_dir_all(&dir);
                    eprintln!("skipped: dbus-daemon is not installed");
                    return None;
                }
                Err(e) => panic!("cannot start dbus-daemon: {e}"),
            };
            // From here on, a failure drops the bus, which stops the daemon and removes the directory.
            let mut bus = Self { daemon, address: String::new(), dir };
            BufReader::new(bus.daemon.stdout.take().unwrap()).read_line(&mut bus.address).unwrap();
            bus.address = bus.address.trim().to_string();
            assert!(!bus.address.is_empty(), "dbus-daemon printed no address");
            Some(bus)
        }

        fn connect(&self) -> Connection {
            let mut channel = Channel::open_private(&self.address).unwrap();
            channel.register().unwrap();
            Connection::from(channel)
        }
    }

    impl Drop for TestBus {
        fn drop(&mut self) {
            let _ = self.daemon.kill();
            let _ = self.daemon.wait();
            let _ = std::fs::remove_dir_all(&self.dir);
        }
    }

    /// Sends a signal with one string, to all or to `to`, and returns once the bus has routed it (the bus handles
    /// the messages of a connection in order, so its answer to a later call comes after).
    fn send(conn: &Connection, iface: &str, member: &str, arg: &str, to: Option<&str>) {
        let mut msg = Message::new_signal("/test", iface, member).unwrap().append1(arg);
        msg.set_destination(to.map(|t| BusName::new(t).unwrap()));
        conn.channel().send(msg).unwrap();
        let bus = conn.with_proxy("org.freedesktop.DBus", "/org/freedesktop/DBus", Duration::from_secs(2));
        let _: (String,) = bus.method_call("org.freedesktop.DBus", "GetId", ()).unwrap();
    }

    /// The name that `take_signal` returns within `wait`, processing the messages that arrive meanwhile.
    fn next_signal(fcitx: &mut Fcitx, wait: Duration) -> Option<String> {
        let end = Instant::now() + wait;
        while Instant::now() < end {
            fcitx.conn.process(Duration::from_millis(10)).unwrap();
            if let Some(name) = fcitx.take_signal().unwrap() {
                return Some(name);
            }
        }
        None
    }

    /// Over a bus, without a tray (as with an input panel): the signals that fcitx5 sends together for one change, of
    /// any interface, make one read, and the mode they name is kept.
    #[test]
    fn signals_of_fcitx5_are_taken_together() {
        let Some(bus) = TestBus::start("together") else { return };
        let mut fcitx = Fcitx::with_connection(bus.connect()).unwrap();
        let fcitx5 = bus.connect();
        fcitx5.request_name(FCITX5, false, true, true).unwrap();
        let direct = "/Fcitx/im:Mozc:fcitx-mozc-direct:直接入力:menu,label=あ";
        send(&fcitx5, PANEL, "RegisterProperties", direct, None);
        send(&fcitx5, PANEL, "UpdateProperty", direct, None);
        send(&fcitx5, "org.example.Other", "Enable", "", None);
        assert_eq!(next_signal(&mut fcitx, Duration::from_secs(2)).as_deref(), Some("Enable"));
        assert_eq!(fcitx.mozc_mode().as_deref(), Some("direct"));
        assert_eq!(next_signal(&mut fcitx, Duration::from_millis(100)), None, "one read for the signals of one change");
    }

    /// Over a bus: signals of another program, and signals addressed to nicola-imesync (also from fcitx5, and the
    /// bus's own on connecting), are not taken. A signal of fcitx5 to all is.
    #[test]
    fn only_signals_of_fcitx5_to_all_are_taken() {
        let Some(bus) = TestBus::start("senders") else { return };
        let mut fcitx = Fcitx::with_connection(bus.connect()).unwrap();
        let me = fcitx.conn.unique_name().to_string();
        let fcitx5 = bus.connect();
        fcitx5.request_name(FCITX5, false, true, true).unwrap();
        let other = bus.connect();
        let direct = "/Fcitx/im:Mozc:fcitx-mozc-direct:直接入力:menu,label=あ";
        send(&other, PANEL, "UpdateProperty", direct, None);
        send(&other, PANEL, "UpdateProperty", direct, Some(&me));
        send(&fcitx5, PANEL, "UpdateProperty", direct, Some(&me));
        // A mark that names no mode, sent after them: once it is taken, the signals above have been handled too.
        send(&fcitx5, PANEL, "Mark", "", None);
        assert_eq!(next_signal(&mut fcitx, Duration::from_secs(2)).as_deref(), Some("Mark"));
        assert_eq!(fcitx.mozc_mode(), None);

        send(&fcitx5, PANEL, "UpdateProperty", "/Fcitx/im:Mozc:fcitx-mozc-hiragana:全角かな:menu,label=あ", None);
        assert_eq!(next_signal(&mut fcitx, Duration::from_secs(2)).as_deref(), Some("UpdateProperty"));
        assert_eq!(fcitx.mozc_mode().as_deref(), Some("hiragana"));
    }

    /// Over a bus: the registration of a tray item is recorded and not lost when a signal of fcitx5 follows it
    /// closely: one read for both, which looks the fcitx5 tray item up again.
    #[test]
    fn registration_of_a_tray_item_is_taken_with_others() {
        let Some(bus) = TestBus::start("tray") else { return };
        let mut fcitx = Fcitx::with_connection(bus.connect()).unwrap();
        let fcitx5 = bus.connect();
        fcitx5.request_name(FCITX5, false, true, true).unwrap();
        let watcher = bus.connect();
        watcher.request_name(TRAY_WATCHER, false, true, true).unwrap();
        send(&watcher, TRAY_WATCHER, ITEM_REGISTERED, ":1.99@/StatusNotifierItem", None);
        // The tray host goes away, so that looking the tray item up fails at once instead of waiting for an answer.
        drop(watcher);
        send(&fcitx5, PANEL, "Enable", "", None);
        // Take both in without reading: the registration is recorded.
        let end = Instant::now() + Duration::from_secs(2);
        loop {
            if fcitx.signals.lock().unwrap().last.as_deref() == Some("Enable") {
                break;
            }
            assert!(Instant::now() < end, "the signals did not come");
            fcitx.conn.process(Duration::from_millis(10)).unwrap();
        }
        assert!(fcitx.signals.lock().unwrap().item_registered, "the registration is recorded");
        assert_eq!(fcitx.take_signal().unwrap().as_deref(), Some("Enable"));
        assert!(!fcitx.signals.lock().unwrap().item_registered, "the tray item is looked up again");
    }
}
