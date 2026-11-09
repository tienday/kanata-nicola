//! `nicola-kbdgen` — generates a kanata `.kbd` from the layout definition (docs/implementation.md, generator).
//!
//! Options: see [`usage`] (`--help`).

use std::process::ExitCode;

use nicola_kbdgen::{generate_parts, Options, ENV_FILE};
use nicola_layout::{LayoutDef, NICOLA_J};

fn usage() -> String {
    let d = Options::default();
    let (left, right) = (&d.thumb_left, &d.thumb_right);
    let toggle = d.toggle_key.as_deref().unwrap_or("none");
    format!(
        "\
usage: nicola-kbdgen [options] > nicola.kbd
       nicola-kbdgen [options] --split -o kbd/nicola.kbd   # also writes kbd/{ENV_FILE}
  From the repository root: cargo run --release -p nicola-kbdgen -- [options]
  (or the built target/release/nicola-kbdgen; it is not installed anywhere).

options:
  --layout <file>          layout definition (default: the bundled NICOLA J type)
  --overlay <file>         apply a layout overlay file (repeatable; applied in the order given)
  --thumbs <L> <R>         evdev names of the thumb keys (default {left} {right})
  --toggle <KEY|none>      NICOLA ON/OFF toggle key (default {toggle}; the Hankaku/Zenkaku key is KEY_GRAVE)
  --key-as-ctrl <KEY>      key to treat as Ctrl besides left and right Ctrl (repeatable;
                           e.g. KEY_CAPSLOCK with xkb ctrl:nocaps)
  --include-device <name>  keyboard name to grab (repeatable; how to find it: docs/settings.md). All keyboards if omitted
  --exclude-device <name>  keyboard name not to grab (repeatable)
  --extra-kbd <file>       the user's own kanata file, read with include at the end (key bindings with
                           defoverrides and the like; docs/settings.md). Written as an absolute path
  -o <file>                output file (default: standard output)
  --split                  write the environment section (defcfg, Ctrl-equivalent keys) to {ENV_FILE}
                           next to the -o file, and make the -o file read it with include (needs -o).
                           An existing {ENV_FILE} that differs is kept (it may have been edited by hand)
  --force                  with --split, overwrite an existing {ENV_FILE}
"
    )
}

fn main() -> ExitCode {
    match run() {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("nicola-kbdgen: {e}");
            ExitCode::FAILURE
        }
    }
}

fn run() -> Result<(), Box<dyn std::error::Error>> {
    let mut opt = Options::default();
    let mut layout_src = NICOLA_J.to_string();
    let mut out: Option<String> = None;
    let mut split = false;
    let mut force = false;
    let mut overlays: Vec<String> = Vec::new();

    let mut args = std::env::args().skip(1);
    let value =
        |args: &mut dyn Iterator<Item = String>, flag: &str| args.next().ok_or_else(|| format!("{flag} needs a value"));
    while let Some(a) = args.next() {
        match a.as_str() {
            "--layout" => layout_src = std::fs::read_to_string(value(&mut args, &a)?)?,
            "--overlay" => overlays.push(value(&mut args, &a)?),
            "--thumbs" => {
                opt.thumb_left = value(&mut args, &a)?;
                opt.thumb_right = value(&mut args, &a)?;
            }
            "--toggle" => {
                let k = value(&mut args, &a)?;
                opt.toggle_key = (k != "none").then_some(k);
            }
            "--key-as-ctrl" => opt.keys_as_ctrl.push(value(&mut args, &a)?),
            "--include-device" => opt.include_device_names.push(value(&mut args, &a)?),
            "--exclude-device" => opt.exclude_device_names.push(value(&mut args, &a)?),
            "--extra-kbd" => {
                // kanata ignores an include that does not exist, and reads a relative path from the directory
                // of the generated file: check it here, and write it absolute.
                let file = value(&mut args, &a)?;
                let path = std::fs::canonicalize(&file).map_err(|e| format!("--extra-kbd {file}: {e}"))?;
                if !path.is_file() {
                    return Err(format!("--extra-kbd {file}: not a file").into());
                }
                opt.extra_kbd =
                    Some(path.to_str().ok_or_else(|| format!("--extra-kbd {file}: not UTF-8"))?.to_string());
            }
            "-o" => out = Some(value(&mut args, &a)?),
            "--split" => split = true,
            "--force" => force = true,
            "-h" | "--help" => {
                print!("{}", usage());
                return Ok(());
            }
            _ => return Err(format!("unknown argument {a}").into()),
        }
    }

    let mut layout = LayoutDef::parse(&layout_src)?;
    for path in &overlays {
        let src = std::fs::read_to_string(path).map_err(|e| format!("{path}: {e}"))?;
        layout.apply_overlay(&src).map_err(|e| format!("{path}: {e}"))?;
    }
    let parts = generate_parts(&layout, &opt)?;
    match (out, split) {
        (Some(path), true) => {
            let env_path = std::path::Path::new(&path).with_file_name(ENV_FILE);
            std::fs::write(&path, parts.main_file())?;
            let generated = parts.env_file();
            match std::fs::read_to_string(&env_path) {
                Ok(existing) if existing == generated => {}
                Ok(_) if !force => eprintln!(
                    "nicola-kbdgen: kept {} (it differs from the generated one; --force overwrites it)",
                    env_path.display()
                ),
                _ => std::fs::write(&env_path, generated)?,
            }
        }
        (None, true) => return Err("--split needs -o".into()),
        (Some(path), false) => std::fs::write(path, parts.single())?,
        (None, false) => print!("{}", parts.single()),
    }
    Ok(())
}
