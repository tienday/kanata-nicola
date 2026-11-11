//! Generator of the kanata configuration (`.kbd`) for NICOLA thumb-shift.
//!
//! Writes a kanata `.kbd` from the layout definition (`layouts/*.toml`) and the options.
//! The layout definition and this generator are the source of truth; the `.kbd` is generated
//! (docs/design.md).
//!
//! The thumb-prefix mode (ADR-18): each thumb key turns on, while it is held, a layer with that
//! thumb's faces. A character key gives the thumb face if a thumb key is held when it is pressed, the
//! solo face otherwise, and is sent at once (no decision time). A chord whose character key comes
//! first is not recognised, and every character key pressed while a thumb key is held gives that
//! thumb's face (continuous shift, beyond the standard): the thumb keys are used like Shift keys
//! (docs/design.md §3.1). It does not follow the NICOLA standard; the NICOLA engine (developed
//! separately, not yet public; docs/design.md §1.2) does.
//!
//! The generated file (its comments are Japanese, as it is a NICOLA setting) is in numbered sections, listed in
//! its header (`Generator::header`; a test checks the list against the section headings). Section 1 is the
//! environment (`defcfg` and the Ctrl-equivalent keys); the others are the common part.
//! The environment section is either inline ([`Generated::single`]) or a separate file ([`ENV_FILE`])
//! read with `include` ([`Generated::main_file`], [`Generated::env_file`]), so that a hand-edited
//! environment survives an update of the common part.
//!
//! Names of the structure (templates, variables, virtual keys, layers, aliases) are English; the
//! alias of an output character and its sending virtual keys are named by the character itself.

pub mod decode;
pub mod encoder;

use nicola_layout::{KeyDef, LayoutDef};

pub use encoder::Encoder;

#[derive(Clone, Debug)]
pub struct Options {
    /// Romaji; Unicode only for the tests.
    pub encoder: Encoder,
    /// Thumb keys (evdev names).
    pub thumb_left: String,
    pub thumb_right: String,
    /// Key that toggles the NICOLA layer (evdev name). The key's own output is sent too.
    pub toggle_key: Option<String>,
    /// Keys treated as Ctrl besides left and right Ctrl (evdev names).
    /// With xkb `ctrl:nocaps` and the like, CapsLock is Ctrl for the desktop but kanata, which runs
    /// below xkb, still sees CapsLock (OQ-13). Specify `KEY_CAPSLOCK` there.
    pub keys_as_ctrl: Vec<String>,
    /// Whether the NICOLA layer is on at start (for the tests; the command always starts with it off).
    pub start_enabled: bool,
    /// `linux-dev-names-include`. Empty means every keyboard (including ones connected later).
    pub include_device_names: Vec<String>,
    /// `linux-dev-names-exclude`. Keyboards not to grab (virtual devices of other input tools etc.).
    pub exclude_device_names: Vec<String>,
    /// `linux-output-device-name`. Name of kanata's output device (kanata's default is "kanata").
    pub output_device_name: Option<String>,
    /// The user's own kanata file, read with `include` at the end (key bindings with `defoverrides` and
    /// the like; docs/settings.md). An absolute path: kanata reads a relative one from the directory of
    /// the generated file.
    pub extra_kbd: Option<String>,
}

impl Default for Options {
    fn default() -> Self {
        Self {
            encoder: Encoder::Romaji,
            thumb_left: "KEY_MUHENKAN".into(),
            thumb_right: "KEY_HENKAN".into(),
            // The Hankaku/Zenkaku key of a JIS keyboard sends KEY_GRAVE on Linux.
            toggle_key: Some("KEY_GRAVE".into()),
            keys_as_ctrl: Vec::new(),
            start_enabled: false,
            include_device_names: Vec::new(),
            exclude_device_names: Vec::new(),
            output_device_name: None,
            extra_kbd: None,
        }
    }
}

/// Modifiers. They are never redefined; while one is held, character and thumb keys pass through
/// (ADR-14).
const MODIFIERS: &[&str] = &["lctl", "rctl", "lalt", "ralt", "lmet", "rmet", "lsft", "rsft"];

/// Always in `defsrc`, so that it can be made Ctrl-equivalent in the environment section alone.
const CAPS: &str = "caps";

/// File name of the environment section when the output is split. The code and the generated files take the
/// name from here; a test checks the few places of the docs that name it.
pub const ENV_FILE: &str = "user-custom.kbd";

/// Heading of section 1 of the generated file, written by [`Generated`] as the section is inline or in
/// [`ENV_FILE`] (the other sections are written whole by the methods of [`Generator`]).
const ENV_HEADING: &str = ";; ==== 1. 環境固有設定 ====";

/// License line of the comment at the top of each generated file: the text of the generated files comes from
/// this crate, and the files are copied apart from the repository (to run kanata directly, on Windows too).
/// The license is that of `Cargo.toml`.
const LICENSE_LINE: &str = concat!(";; SPDX-License-Identifier: ", env!("CARGO_PKG_LICENSE"));

/// evdev name -> kanata key name (Linux).
pub fn kanata_key(evdev: &str) -> Option<&'static str> {
    Some(match evdev {
        "KEY_GRAVE" => "grv",
        "KEY_MINUS" => "min",
        "KEY_EQUAL" => "=",
        "KEY_YEN" => "¥",
        "KEY_LEFTBRACE" => "[",
        "KEY_RIGHTBRACE" => "]",
        "KEY_SEMICOLON" => "scln",
        "KEY_APOSTROPHE" => "'",
        "KEY_BACKSLASH" => "\\",
        "KEY_COMMA" => "comm",
        "KEY_DOT" => ".",
        "KEY_SLASH" => "/",
        "KEY_RO" => "ro",
        "KEY_HENKAN" => "henk",
        "KEY_MUHENKAN" => "mhnk",
        "KEY_KATAKANAHIRAGANA" => "kana",
        "KEY_SPACE" => "spc",
        "KEY_ENTER" => "ret",
        "KEY_PAUSE" => "pause",
        "KEY_CAPSLOCK" => "caps",
        "KEY_LEFTSHIFT" => "lsft",
        "KEY_RIGHTSHIFT" => "rsft",
        "KEY_LEFTCTRL" => "lctl",
        "KEY_RIGHTCTRL" => "rctl",
        "KEY_LEFTALT" => "lalt",
        "KEY_RIGHTALT" => "ralt",
        "KEY_LEFTMETA" => "lmet",
        "KEY_RIGHTMETA" => "rmet",
        _ => {
            let rest = evdev.strip_prefix("KEY_")?;
            const SIMPLE: &[&str] = &[
                "a", "b", "c", "d", "e", "f", "g", "h", "i", "j", "k", "l", "m", "n", "o", "p", "q", "r", "s", "t",
                "u", "v", "w", "x", "y", "z", "0", "1", "2", "3", "4", "5", "6", "7", "8", "9",
            ];
            let lower = rest.to_ascii_lowercase();
            return SIMPLE.iter().find(|s| **s == lower).copied();
        }
    })
}

/// Human-readable name of a thumb or toggle key, for the comments of the generated file.
fn labelled(kanata: &str) -> String {
    let label = match kanata {
        "mhnk" => "無変換",
        "henk" => "変換",
        "grv" => "半角/全角",
        "spc" => "Space",
        "caps" => "CapsLock",
        "kana" => "カタカナ/ひらがな",
        _ => return kanata.to_string(),
    };
    format!("{kanata}（{label}）")
}

#[derive(Debug)]
pub struct Error(pub String);

impl std::fmt::Display for Error {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}

impl std::error::Error for Error {}

fn key(evdev: &str) -> Result<&'static str, Error> {
    kanata_key(evdev).ok_or_else(|| Error(format!("no kanata name for {evdev}")))
}

/// A name written into the `.kbd`: no control characters (a line break would end a comment), and no
/// double quote in a quoted string.
fn check_name(what: &str, name: &str, quoted: bool) -> Result<(), Error> {
    if name.chars().any(char::is_control) || (quoted && name.contains('"')) {
        let chars = if quoted { "a double quote or a control character" } else { "a control character" };
        return Err(Error(format!("{what} {name:?} contains {chars}")));
    }
    Ok(())
}

/// A generated configuration, in three parts.
pub struct Generated {
    header: String,
    env: String,
    common: String,
}

impl Generated {
    /// One self-contained file, with the environment section inline.
    pub fn single(&self) -> String {
        let Generated { header, env, common } = self;
        format!("{header}{ENV_HEADING}\n{env}\n{common}")
    }

    /// The main file of the split form. It reads the environment section from [`ENV_FILE`]
    /// (looked up next to the main file).
    pub fn main_file(&self) -> String {
        let Generated { header, common, .. } = self;
        format!(
            "\
{header}{ENV_HEADING}
;; {ENV_FILE} に記述する。
(include {ENV_FILE})

{common}"
        )
    }

    /// The environment section of the split form ([`ENV_FILE`]).
    pub fn env_file(&self) -> String {
        let env = &self.env;
        format!(
            "\
;; kanata-nicola — 環境固有設定（{ENV_FILE}）
{LICENSE_LINE}
;;
;; 配列プロファイル（nicola.kbd 等）が include で読み込む。配列プロファイルを更新した場合も、このファイルはそのまま使用できる。
;; 項目と記述方法は docs/kanata-kbd.md。

{env}"
        )
    }
}

/// Generates one self-contained `.kbd`.
pub fn generate(layout: &LayoutDef, opt: &Options) -> Result<String, Error> {
    Ok(generate_parts(layout, opt)?.single())
}

/// Generates the configuration in parts (see [`Generated`]).
pub fn generate_parts(layout: &LayoutDef, opt: &Options) -> Result<Generated, Error> {
    let g = Generator::new(layout, opt)?;
    let common = [g.keys_handled(), g.layers(), g.thumb_keys(), g.on_off(), g.sending(), g.extra()].concat();
    Ok(Generated { header: g.header(), env: g.environment(), common })
}

/// The layout and the options, checked and resolved to kanata key names. Each section of the generated
/// file is written by one method, in the order of the file.
///
/// The methods write format strings that read like the output. A value that holds whole lines, each
/// ending with a line break (an optional line, or a block), starts a line of the format string and is
/// followed by `\`, which drops the line break after it, and also the indentation of the next line (so
/// that line must start at the first column). Without the `\`, the line break makes a blank line after
/// the value. The rows of `defsrc` and of the layers have no last line break, and are written like one
/// line.
struct Generator<'a> {
    layout: &'a LayoutDef,
    opt: &'a Options,
    /// Left and right.
    thumbs: [&'static str; 2],
    toggle: Option<&'static str>,
    keys_as_ctrl: Vec<&'static str>,
    /// Keys with at least one face, in layout order. A key whose faces were all removed by an overlay
    /// is left out of `defsrc` and passes through as it is.
    keys: Vec<(&'static str, &'a KeyDef)>,
    /// Output characters with an alias (the ones the encoder can send), in order of appearance.
    symbols: Vec<&'a str>,
}

impl<'a> Generator<'a> {
    fn new(layout: &'a LayoutDef, opt: &'a Options) -> Result<Self, Error> {
        let thumbs = [key(&opt.thumb_left)?, key(&opt.thumb_right)?];
        let toggle = opt.toggle_key.as_deref().map(key).transpose()?;
        let keys_as_ctrl: Vec<&str> = opt.keys_as_ctrl.iter().map(|k| key(k)).collect::<Result<_, _>>()?;
        // A key has one of these roles. kanata rejects a key twice in defsrc (the two thumbs, a thumb as the
        // toggle key), and a thumb or the toggle key treated as Ctrl would let the character keys through
        // as they are. A key given twice as Ctrl is harmless.
        let mut roles = vec![
            (thumbs[0], &opt.thumb_left, "the left thumb key (--thumbs)"),
            (thumbs[1], &opt.thumb_right, "the right thumb key (--thumbs)"),
        ];
        roles.extend(toggle.zip(opt.toggle_key.as_ref()).map(|(k, name)| (k, name, "the toggle key (--toggle)")));
        roles.extend(
            keys_as_ctrl.iter().zip(&opt.keys_as_ctrl).map(|(&k, name)| (k, name, "treated as Ctrl (--key-as-ctrl)")),
        );
        for (i, (k, name, role)) in roles.iter().enumerate() {
            if let Some((_, _, other)) = roles[..i].iter().find(|(o, _, r)| o == k && r != role) {
                return Err(Error(format!("{name} is {other} and {role}; give them different keys")));
            }
        }
        // While a modifier is held, the character keys pass through as they are (ADR-14): a modifier as a thumb key
        // would never give its faces.
        for (k, name, side) in [(thumbs[0], &opt.thumb_left, "left"), (thumbs[1], &opt.thumb_right, "right")] {
            if MODIFIERS.contains(&k) {
                return Err(Error(format!(
                    "{name} is a modifier key, so it cannot be the {side} thumb key (--thumbs): while a modifier is \
                     held, the character keys pass through as they are"
                )));
            }
        }
        let keys: Vec<(&'static str, &KeyDef)> =
            layout.active_keys().map(|k| key(&k.name).map(|n| (n, k))).collect::<Result<_, _>>()?;
        // Faces on these keys would take their place (kanata would only reject the config, with a less clear message).
        // The mistake can be in the option as well as in the layout, so the message names the option.
        for (n, k) in &keys {
            let (role, option) = if thumbs.contains(n) {
                ("a thumb key", Some("--thumbs"))
            } else if Some(*n) == toggle {
                ("the toggle key", Some("--toggle"))
            } else if MODIFIERS.contains(n) {
                ("a modifier key", None)
            } else if keys_as_ctrl.contains(n) {
                ("a key treated as Ctrl", Some("--key-as-ctrl"))
            } else {
                continue;
            };
            let fix = match option {
                Some(o) => format!("choose another key for {o}, or remove its faces in the layout and overlay files"),
                None => "check the layout and overlay files".to_string(),
            };
            return Err(Error(format!("{} is {role}, so it cannot have faces ({fix})", k.name)));
        }
        for name in opt.include_device_names.iter().chain(&opt.exclude_device_names).chain(&opt.output_device_name) {
            check_name("keyboard name", name, true)?;
        }
        check_name("layout name", &layout.meta.name, false)?;
        check_name("layout source", &layout.meta.source, false)?;
        for name in &layout.overlays {
            check_name("overlay name", name, false)?;
        }
        if let Some(path) = &opt.extra_kbd {
            check_name("extra .kbd path", path, true)?;
        }
        // A face that would not give what is written is an error: the key itself would be typed instead.
        for (_, k) in &keys {
            // A key that an overlay added without a position is named alone.
            let at = if k.pos.is_empty() { String::new() } else { format!(" ({})", k.pos) };
            for (face, sym) in ["solo", "lthumb", "rthumb"].into_iter().zip(faces(k)) {
                if let Some(sym) = sym {
                    encoder::check(opt.encoder, sym)
                        .map_err(|why| Error(format!("{}{at} {face} = {sym:?}: {why}", k.name)))?;
                }
            }
        }

        let mut symbols: Vec<&str> = Vec::new();
        for (_, k) in &keys {
            for sym in faces(k).into_iter().flatten() {
                if encoder::supports(opt.encoder, sym) && !symbols.contains(&sym) {
                    symbols.push(sym);
                }
            }
        }
        // Two faces of one name in the .kbd ([`name`]): romaji ( and ) are named （ and ）, the characters of the
        // number row. They send the same keys, so one definition serves both (`per_key_definitions`); faces of one
        // name that sent other keys would need two definitions of that name, which kanata rejects.
        for (i, a) in symbols.iter().enumerate() {
            if let Some(b) = symbols[..i].iter().find(|b| name(b) == name(a)) {
                if encoder::keys(opt.encoder, a) != encoder::keys(opt.encoder, b) {
                    return Err(Error(format!(
                        "the faces {b:?} and {a:?} have the same name in the .kbd ({}) but send other keys",
                        name(a)
                    )));
                }
            }
        }
        Ok(Self { layout, opt, thumbs, toggle, keys_as_ctrl, keys, symbols })
    }

    /// The header: the choices the file was generated with, and the list of the sections.
    fn header(&self) -> String {
        let (name, source) = (&self.layout.meta.name, &self.layout.meta.source);
        let overlays = match self.layout.overlays.as_slice() {
            [] => String::new(),
            names => format!(";; 差分:     {}\n", names.join(", ")),
        };
        let (encoder, sending) = match self.opt.encoder {
            Encoder::Romaji => ("ローマ字（IME はローマ字入力にする）", "文字 → IME に送出するキー列"),
            Encoder::Unicode => ("Unicode（テスト用）", "文字 → kanata の unicode 出力"),
        };
        let [left, right] = self.thumbs.map(labelled);
        let toggle = match self.toggle {
            None => "キーでは切り替えない（IME 同期などが kanata のレイヤを切り替える）".to_string(),
            Some(t) => format!("{}で NICOLA の ON/OFF（キー本来の出力も送出する）", labelled(t)),
        };
        let start = if self.opt.start_enabled { "ON（nicola レイヤ）" } else { "OFF（base レイヤ）" };
        // A line of the list (with its line break), or empty: the blank line after the list follows it.
        let extra = match self.opt.extra_kbd {
            Some(_) => ";;   7. 追加の kanata の設定ファイル  キーの再割り当て等（include で読み込む）\n",
            None => "",
        };
        // The sections are in the order of the table; kanata needs an order only within a template or a
        // virtual key that uses another one (docs/kanata-kbd-design.md).
        format!(
            "\
;; kanata-nicola — 親指シフト（NICOLA）の kanata 設定
{LICENSE_LINE}
;;
;; kanata 設定生成器（このリポジトリの crates/nicola-kbdgen）で生成した設定ファイル。
;; 編集と再生成の方法は docs/kanata-kbd.md、仕組みは docs/kanata-kbd-design.md。
;;
;; 配列:     {name}（{source}）
{overlays}\
;; 判定方式: 親指先行（親指キーを押してから文字キーを押すとシフト面。文字は押した瞬間に入力される）
;; 送出:     {encoder}
;; 親指キー: 左 = {left}、右 = {right}
;; 切り替え: {toggle}
;; 起動時:   NICOLA は {start}
;; 非常停止: 左Ctrl+Space+Esc（kanata の標準）
;;
;; 構成:
;;   1. 環境固有設定      対象にするキーボード、Ctrl として扱うキー
;;   2. 扱うキー          defsrc、修飾キーの条件
;;   3. 配列              レイヤ（キー → 文字）
;;   4. 親指キー          親指の面の切り替え、親指の単独打鍵
;;   5. NICOLA の ON/OFF  基本レイヤ（base・nicola）の切り替え
;;   6. 文字の送出        {sending}
{extra}
"
        )
    }

    /// Section 1, the environment: the keyboards to grab (`defcfg`), and the keys treated as Ctrl.
    fn environment(&self) -> String {
        let quote = |v: &[String]| v.iter().map(|n| format!("\"{n}\"")).collect::<Vec<_>>().join(" ");
        let include = match self.opt.include_device_names.as_slice() {
            [] => String::new(),
            names => format!(
                "  ;; 対象にするキーボード（名前の調べ方は docs/settings.md）\n  linux-dev-names-include ({})\n",
                quote(names)
            ),
        };
        let exclude = match self.opt.exclude_device_names.as_slice() {
            [] => String::new(),
            names => format!(
                "  ;; 除外するキーボード（NICOLA に使用しない、キーのある入力デバイス等）\n  linux-dev-names-exclude ({})\n",
                quote(names)
            ),
        };
        let output = match &self.opt.output_device_name {
            Some(name) => format!("  linux-output-device-name \"{name}\"\n"),
            None => String::new(),
        };
        let keys_as_ctrl: String = self.keys_as_ctrl.iter().map(|k| format!(" (input real {k})")).collect();
        // kanata logs every layer change by default, which here means every thumb key press.
        format!(
            "\
(defcfg
  ;; 変更しない（defsrc に無いキー〔Enter 等〕も kanata で処理し、文字の送出より先に出力されないようにするため）。
  process-unmapped-keys yes
  ;; レイヤの切り替えをログに出力しない（親指キーを押すたびに出力されるため）。
  log-layer-changes no
{include}\
{exclude}\
{output}\
)

(defvar
  ;; 左右の Ctrl に加えて、Ctrl として扱うキー（デスクトップのキー配列の設定〔xkb〕で Ctrl に変更したキー）。
  ;; 例: CapsLock の場合は (or (input real caps))。指定しない場合は (or)。
  keys-as-ctrl (or{keys_as_ctrl})
)
"
        )
    }

    /// Section 2, the keys handled: `defsrc` (the character keys by row, then the thumb and toggle keys,
    /// then the modifiers), and the modifier condition.
    fn keys_handled(&self) -> String {
        let char_rows = self
            .keys
            .chunk_by(|(_, a), (_, b)| a.pos.chars().next() == b.pos.chars().next())
            .map(|row| {
                let names: Vec<&str> = row.iter().map(|(n, _)| *n).collect();
                format!("  {}", names.join(" "))
            })
            .collect::<Vec<_>>()
            .join("\n");
        // Each key once (a thumb key can also be the toggle key, a key treated as Ctrl can be a modifier).
        let mut seen: Vec<&str> = self.keys.iter().map(|(n, _)| *n).collect();
        let mut once = |keys: Vec<&'static str>| {
            let mut row = Vec::new();
            for k in keys {
                if !seen.contains(&k) {
                    seen.push(k);
                    row.push(k);
                }
            }
            row.join(" ")
        };
        let thumbs_and_toggle = once(self.thumbs.into_iter().chain(self.toggle).collect());
        let modifier_keys =
            once(MODIFIERS.iter().copied().chain([CAPS]).chain(self.keys_as_ctrl.iter().copied()).collect());
        let modifiers = MODIFIERS.iter().map(|k| format!("(input real {k})")).collect::<Vec<_>>().join(" ");
        format!(
            "\
;; ==== 2. 扱うキー ====
;; defsrc に無いキー（Enter・BackSpace 等）は、そのまま出力される。
(defsrc
{char_rows}
  {thumbs_and_toggle}
  {modifier_keys}
)

(defvar
  ;; 修飾キー（Shift を含む）。押している間は、文字キー・親指キーをそのまま出力する。
  modifiers ({modifiers} $keys-as-ctrl)
)

"
        )
    }

    /// Section 3, the layout: the face templates, and the layers (the first one is active at start).
    fn layers(&self) -> String {
        let cell_char = match self.opt.encoder {
            Encoder::Romaji => "文字は、節 6 で定義する alias の名前（kanata が送出するのは、その文字のキー列）",
            Encoder::Unicode => "文字は、節 6 で定義する alias の名前",
        };
        let raw_thumb_face = self
            .keys
            .iter()
            .any(|(_, k)| [&k.faces.lthumb, &k.faces.rthumb].into_iter().any(|f| self.resolve(k, f).is_none()));
        let (raw_note, raw_template) = if raw_thumb_face {
            (
                ";;   shifted-raw  送出できる文字の無い親指の面。キーをそのまま出力し、親指の単独打鍵を取り消す\n",
                "\
(deftemplate shifted-raw (key)
  (multi (on-press press-vkey lthumb-used) (on-press press-vkey rthumb-used) $key))
",
            )
        } else {
            ("", "")
        };
        let (base, nicola) = (self.base_layer(), self.nicola_layer());
        let (first, second) = if self.opt.start_enabled { (nicola, base) } else { (base, nicola) };
        let [l, r] = self.thumbs;
        let layers = [
            first,
            second,
            self.thumb_layer("l", "左", l, |k| &k.faces.lthumb),
            self.thumb_layer("r", "右", r, |k| &k.faces.rthumb),
        ]
        .join("\n");
        format!(
            "\
;; ==== 3. 配列 ====
;; キーの定義は「キー (t! テンプレート キー 文字)」。{cell_char}。
;; テンプレート（修飾キーを押している間は、どれもキーをそのまま出力する）:
;;   unshifted    単独面の文字
;;   shifted      親指の面の文字。親指の単独打鍵を取り消す（節 4）
{raw_note}\
;; 最初に定義したレイヤが、起動時のレイヤになる。
(deftemplate unshifted (key char)
  (switch $modifiers $key break () (concat @ $char) break))
(deftemplate shifted (key char)
  (multi (on-press press-vkey lthumb-used) (on-press press-vkey rthumb-used)
         (switch $modifiers $key break () (concat @ $char) break)))
{raw_template}
{layers}
"
        )
    }

    /// The `base` layer (NICOLA off): only the toggle key.
    fn base_layer(&self) -> String {
        match self.toggle {
            Some(t) => format!(
                "\
;; base: NICOLA OFF。NICOLA 切替キーだけを定義する（ほかのキーはそのまま出力される）。
(deflayermap (base)
  {t} @nicola-on
)
"
            ),
            // deflayermap needs at least one entry; a transparent thumb key changes nothing.
            None => {
                let th = self.thumbs[0];
                format!(
                    "\
;; base: NICOLA OFF。キーはそのまま出力される（レイヤには 1 つ以上のキーの定義が必要なので、透過の {th} _ を記述する）。
(deflayermap (base)
  {th} _
)
"
                )
            }
        }
    }

    /// The `nicola` layer (NICOLA on): the solo faces, the thumb keys and the toggle key.
    fn nicola_layer(&self) -> String {
        let note = match self.keys.iter().find(|(_, k)| self.resolve(k, &k.faces.solo).is_none()) {
            Some((n, _)) => format!("送出できる文字の無い単独面は、キーそのものを記述する（例: {n} {n}）。"),
            None => String::new(),
        };
        // A solo face that cannot be sent is the physical key itself (with or without modifiers).
        let rows = self.rows(|n, k| match self.resolve(k, &k.faces.solo) {
            Some(sym) => format!("(t! unshifted {n} {})", name(sym)),
            None => n.to_string(),
        });
        let [l, r] = self.thumbs;
        let toggle = self.toggle.map(|t| format!("  {t} @nicola-off")).unwrap_or_default();
        format!(
            "\
;; nicola: NICOLA ON。単独面。{note}
(deflayermap (nicola)
{rows}
  {l} @lthumb  {r} @rthumb{toggle}
)
"
        )
    }

    /// The layer of a thumb key (`side` is `l` or `r`): its faces.
    fn thumb_layer(&self, side: &str, label: &str, th: &str, face: impl Fn(&KeyDef) -> &Option<String>) -> String {
        let rows = self.rows(|n, k| match self.resolve(k, face(k)) {
            Some(sym) => format!("(t! shifted {n} {})", name(sym)),
            None => format!("(t! shifted-raw {n})"),
        });
        format!(
            "\
;; nicola-{side}thumb: {label}の親指キー（{th}）を押している間。{label}親指の面（記述していないキーは nicola のまま）。
(deflayermap (nicola-{side}thumb)
{rows}
)
"
        )
    }

    /// The cells of a layer, one line per row and hand (columns 01-05 are the left hand), so that the
    /// layer reads like the keyboard.
    fn rows(&self, cell: impl Fn(&str, &KeyDef) -> String) -> String {
        let place = |k: &KeyDef| {
            let column: u8 = k.pos.get(1..).and_then(|x| x.parse().ok()).unwrap_or(0);
            (k.pos.chars().next(), column <= 5)
        };
        self.keys
            .chunk_by(|(_, a), (_, b)| place(a) == place(b))
            .map(|row| {
                let cells: Vec<String> = row.iter().map(|(n, k)| format!("{n} {}", cell(n, k))).collect();
                format!("  {}", cells.join("  "))
            })
            .collect::<Vec<_>>()
            .join("\n")
    }

    /// Section 4, the thumb keys: the marks and the solo taps (virtual keys), and the aliases of the
    /// thumb keys.
    fn thumb_keys(&self) -> String {
        let [l, r] = self.thumbs;
        // A modifier pressed after the thumb key must not apply to the deferred tap: a plain keyboard would have
        // sent the key on press, before the modifier (Shift+Muhenkan or Ctrl+Muhenkan can switch the IME).
        format!(
            "\
;; ==== 4. 親指キー ====
;; 押している間、その親指の面のレイヤを有効にする。文字キーを押さずに離したときは、親指の単独打鍵（{l}・{r}）を送出する。
(defvirtualkeys
  lthumb-used XX
  lthumb-tap (switch ((input virtual lthumb-used)) XX break $modifiers XX break () {l} break)
  rthumb-used XX
  rthumb-tap (switch ((input virtual rthumb-used)) XX break $modifiers XX break () {r} break)
)
(defalias
  lthumb (switch
    $modifiers {l} break
    () (multi (on-press release-vkey lthumb-used) (layer-while-held nicola-lthumb) (on-release tap-vkey lthumb-tap)) break)
  rthumb (switch
    $modifiers {r} break
    () (multi (on-press release-vkey rthumb-used) (layer-while-held nicola-rthumb) (on-release tap-vkey rthumb-tap)) break)
)

"
        )
    }

    /// Section 5, NICOLA on/off: the aliases of the toggle key.
    fn on_off(&self) -> String {
        let body = match self.toggle {
            None => {
                ";; キーでは切り替えない。IME 同期（nicola-imesync）が、kanata の TCP ポートから基本レイヤを切り替える。\n"
                    .to_string()
            }
            Some(t) => {
                let label = labelled(t);
                format!(
                    "\
;; {label}で切り替える（キー本来の出力も送出する）。
(defalias
  nicola-on (multi {t} (layer-switch nicola))
  nicola-off (multi {t} (layer-switch base))
)
"
                )
            }
        };
        format!(
            "\
;; ==== 5. NICOLA の ON/OFF ====
{body}
"
        )
    }

    /// Section 6, sending the characters: the sending templates, and per key one `defvirtualkeys` and
    /// one `defalias` with a template expansion per character. Not one large list, nor one top-level
    /// template per character: kanata 1.11.0 copies the whole enclosing list for each template expansion
    /// in it (the whole configuration at the top level), which makes the parse two to five times slower
    /// (docs/design.md, kanata constraints).
    fn sending(&self) -> String {
        let mut arities: Vec<usize> =
            self.symbols.iter().filter_map(|s| encoder::keys(self.opt.encoder, s)).map(|k| k.len()).collect();
        arities.sort();
        arities.dedup();
        // Faces written as romaji (ASCII) and their keys, in order of appearance.
        let romaji: Vec<(&str, Vec<String>)> = self
            .symbols
            .iter()
            .filter(|s| encoder::is_romaji(s))
            .filter_map(|s| Some((*s, encoder::keys(self.opt.encoder, s)?)))
            .collect();
        // Shift pressed by a character (？) or a romaji face (z*) stays pressed until the next tick, so the
        // next character waits for it as for a key of its own; otherwise its keys could be typed with Shift.
        let shift = self
            .symbols
            .iter()
            .filter_map(|s| encoder::keys(self.opt.encoder, s))
            .any(|keys| keys.iter().any(|k| k == "lsft"));
        let about = if arities.is_empty() {
            ";; 文字ごとに、kanata の unicode で文字を出力する（テスト用。IME を通らない）。\n".to_string()
        } else {
            let same = self.same_keys_note();
            let romaji_note = match romaji.first() {
                None => String::new(),
                Some((face, keys)) => {
                    let keys = keys.join(" ");
                    let renamed = if romaji.iter().any(|(s, _)| name(s) != *s) {
                        "名前では \" ( ) を全角にする。"
                    } else {
                        ""
                    };
                    format!(";; ASCII で記述した面（{face} 等）は IME に入力するローマ字で、記述したとおりのキーを送出する（{face} = {keys}）。{renamed}\n")
                }
            };
            format!(
                "\
;; 文字ごとに、IME（ローマ字入力）に送出するキー列を、次の 2 行で定義する（2 行の引数は同じにする）:
;;   (defvirtualkeys (t! keys2 か k a))
;;   (defalias       (t! send2 か k a))
;; 数字は、送出のキー列のキーの数（lsft も数える。か = k a は 2、？ = lsft / は 2）。
{romaji_note}\
{same}"
            )
        };
        let templates: String =
            arities.iter().map(|&n| keys_template(n)).chain(arities.iter().map(|&n| send_template(n, shift))).collect();
        let per_key = self.per_key_definitions();
        format!(
            "\
;; ==== 6. 文字の送出 ====
{about}\
{templates}\
;; キーごとに、そのキーの面に記述されている文字を定義する（同じ文字は最初のキーで 1 回）。
;; コメントは「位置 キー: 単独 / 左親指 / 右親指」（未定義の親指の面では、単独面の文字が入力される）。
{per_key}"
        )
    }

    /// Section 7, the user's own kanata file, if given: read with `include` (quoted: the path may have
    /// spaces). kanata reports a mistake in it with the file's name and line.
    fn extra(&self) -> String {
        let Some(path) = &self.opt.extra_kbd else { return String::new() };
        format!(
            "
;; ==== 7. 追加の kanata の設定ファイル ====
;; 設定 NICOLA_EXTRA_KBD（kanata 設定生成器では --extra-kbd）に指定したファイル。キーの再割り当て（defoverrides）等。
;; 記述方法と使用できない定義は docs/settings.md。
(include \"{path}\")
"
        )
    }

    /// The note on characters sent with the same keys (、 and ， both with comm, ・ and ／ with /): which one
    /// appears is up to the IME. A line, or empty. Romaji faces are left out: they are the keys written, not
    /// characters.
    fn same_keys_note(&self) -> String {
        let mut by_keys: Vec<(String, Vec<&str>)> = Vec::new();
        for sym in self.symbols.iter().filter(|s| !encoder::is_romaji(s)) {
            if let Some(keys) = encoder::keys(self.opt.encoder, sym) {
                let keys = keys.join(" ");
                match by_keys.iter_mut().find(|(k, _)| *k == keys) {
                    Some((_, syms)) => syms.push(sym),
                    None => by_keys.push((keys, vec![sym])),
                }
            }
        }
        let same: Vec<String> = by_keys
            .iter()
            .filter(|(_, s)| s.len() > 1)
            .map(|(k, s)| format!("「{}」（{k}）", s.join("」と「")))
            .collect();
        if same.is_empty() {
            return String::new();
        }
        let same = same.join("、");
        // The IME settings that decide which of a pair comes out: punctuation for 、。，．, symbols for ／・.
        let has = |chars: &[&str]| by_keys.iter().any(|(_, s)| s.len() > 1 && s.iter().any(|c| chars.contains(c)));
        let mut settings = Vec::new();
        if has(&["、", "。", "，", "．"]) {
            settings.push("句読点");
        }
        if has(&["／", "・"]) {
            settings.push("記号の入力");
        }
        let settings = if settings.is_empty() { String::new() } else { format!("（{}）", settings.join("、")) };
        format!(";; 同じキー列を送出する文字: {same}。どちらが入力されるかは IME の設定{settings}で決まる。\n")
    }

    /// Per key, a comment with its faces, and the definitions of the characters that first appear on it. A face
    /// whose name is already defined (romaji ( after （, which sends the same keys; see [`Generator::new`]) uses
    /// that definition.
    fn per_key_definitions(&self) -> String {
        let mut out = String::new();
        let mut defined: Vec<String> = Vec::new();
        for (n, k) in &self.keys {
            // A key that an overlay added without a position is shown by its name alone.
            let at = if k.pos.is_empty() { String::new() } else { format!("{} ", k.pos) };
            let shown = faces(k).map(|f| f.unwrap_or("未定義")).join(" / ");
            let unsupported: Vec<&str> =
                faces(k).into_iter().flatten().filter(|s| !encoder::supports(self.opt.encoder, s)).collect();
            let note = match unsupported.as_slice() {
                [] => String::new(),
                syms => format!("（{} は送出できないため、キーそのものを出力する）", syms.join(" ")),
            };
            out += &format!(";; {at}{n}: {shown}{note}\n");
            let (mut vkeys, mut aliases) = (Vec::new(), Vec::new());
            for sym in faces(k).into_iter().flatten() {
                if !self.has_alias(sym) || defined.contains(&name(sym)) {
                    continue;
                }
                defined.push(name(sym));
                match encoder::keys(self.opt.encoder, sym) {
                    Some(keys) => {
                        let args = format!("{} {} {}", keys.len(), name(sym), keys.join(" "));
                        vkeys.push(format!("(t! keys{args})"));
                        aliases.push(format!("(t! send{args})"));
                    }
                    None => aliases.push(format!("{sym} (unicode {sym})")),
                }
            }
            if !vkeys.is_empty() {
                out += &format!("(defvirtualkeys {})\n", vkeys.join("  "));
            }
            if !aliases.is_empty() {
                // Aligned with the defvirtualkeys line above.
                let pad = if vkeys.is_empty() { "" } else { "      " };
                out += &format!("(defalias {pad}{})\n", aliases.join("  "));
            }
        }
        out
    }

    /// Whether a character has an alias (the encoder can send it).
    fn has_alias(&self, sym: &str) -> bool {
        self.symbols.contains(&sym)
    }

    /// What a face of a key outputs: the character of the face (or of the solo face when the face is
    /// undefined, ADR-15), or `None` (the physical key) when the encoder cannot send it.
    fn resolve<'k>(&self, k: &'k KeyDef, face: &'k Option<String>) -> Option<&'k str> {
        let sym = face.as_deref().or(k.faces.solo.as_deref())?;
        self.has_alias(sym).then_some(sym)
    }
}

/// The parameters of a sending template for `n` keys: their names, the keys pressed, the keys released.
fn send_params(n: usize) -> (String, String, String) {
    let names: Vec<String> = (1..=n).map(|i| format!("k{i}")).collect();
    let press = names.iter().map(|k| format!("${k}")).collect::<Vec<_>>().join(" ");
    let release = names.iter().map(|k| format!("(release-key ${k})")).collect::<Vec<_>>().join(" ");
    (names.join(" "), press, release)
}

/// The template `keysN`: the virtual keys `up-<char>` (release the keys) and `down-<char>` (press them,
/// and release them with `up-<char>`) of a character sent with `n` keys.
fn keys_template(n: usize) -> String {
    let (params, press, release) = send_params(n);
    format!(
        "\
(deftemplate keys{n} (char {params})
  (concat up- $char) (multi {release})
  (concat down- $char) (multi {press} (on-press tap-vkey (concat up- $char))))
"
    )
}

/// The template `sendN`: the alias of a character sent with `n` keys. It presses the keys and releases
/// them with `up-<char>`; if one of them is still pressed (by the previous character), it releases them
/// first and presses them again with `down-<char>`. With `shift`, it does so also while Shift is pressed
/// (by a face that types a character with Shift).
fn send_template(n: usize, shift: bool) -> String {
    let (params, press, release) = send_params(n);
    let shift = if shift { " lsft" } else { "" };
    format!(
        "\
(deftemplate send{n} (char {params})
  $char (switch ({press}{shift})
    (multi {release} (on-press tap-vkey (concat down- $char))) break
    () (multi {press} (on-press tap-vkey (concat up- $char))) break))
"
    )
}

/// The name of a face in the `.kbd` (its alias, and its sending virtual keys after `up-` and `down-`):
/// the character, or the romaji as written, with the characters that kanata does not take in a name
/// made full-width (`z(` → `z（`).
fn name(sym: &str) -> String {
    sym.chars()
        .map(|c| match c {
            '"' => '＂',
            '(' => '（',
            ')' => '）',
            c => c,
        })
        .collect()
}

/// The three faces of a key: solo, left thumb, right thumb.
fn faces(k: &KeyDef) -> [Option<&str>; 3] {
    [k.faces.solo.as_deref(), k.faces.lthumb.as_deref(), k.faces.rthumb.as_deref()]
}
