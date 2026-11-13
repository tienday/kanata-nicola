//! The options of nicola-imesync that docs/implementation.md §4 gives (the command line and the table, with the
//! defaults) are those of its `--help`; `--version` prints the version of the package, and `-h` the same as `--help`.

use std::collections::BTreeMap;
use std::process::Command;

const DOC: &str = include_str!("../../../docs/implementation.md");

/// The options of `--help`, each with its default if it has one ("--port": Some("23800")).
fn help_options() -> BTreeMap<String, Option<String>> {
    let out = Command::new(env!("CARGO_BIN_EXE_nicola-imesync")).arg("--help").output().unwrap();
    let help = String::from_utf8(out.stdout).unwrap();
    let mut options = BTreeMap::new();
    let mut current: Vec<&str> = vec![];
    for line in help.lines() {
        // An option starts a line after two spaces, and its names end at the next two ("  -h, --help   …"); its
        // description may go on in the next lines.
        if let Some(names) = line.strip_prefix("  ").filter(|l| l.starts_with('-')) {
            current = names.split("  ").next().unwrap().split(", ").map(|o| o.split(' ').next().unwrap()).collect();
            for option in &current {
                options.insert(option.to_string(), None);
            }
        }
        if let Some(default) = line.split("(default ").nth(1).map(|d| d.split([')', ';']).next().unwrap()) {
            for option in &current {
                options.insert(option.to_string(), Some(default.to_string()));
            }
        }
    }
    options
}

#[test]
fn docs_list_the_imesync_options() {
    let section = DOC.split("\n## 4. ").nth(1).expect("docs/implementation.md has §4").split("\n## ").next().unwrap();
    let help = help_options();
    let names = |v: &BTreeMap<String, Option<String>>| v.keys().cloned().collect::<Vec<_>>();

    // The command line: "nicola-imesync [--port <n>] … [--dry-run]".
    let line = section.lines().find(|l| l.starts_with("nicola-imesync [")).expect("the command line in §4");
    let mut in_line: Vec<String> = line
        .split('[')
        .skip(1)
        .map(|o| o.trim_end_matches([']', ' ']).split(' ').next().unwrap().to_string())
        .collect();
    in_line.sort();
    // Without -h and -V, which only print: the table has them.
    let runs = |o: &String| !matches!(o.as_str(), "-h" | "--help" | "-V" | "--version");
    assert_eq!(in_line, names(&help).into_iter().filter(runs).collect::<Vec<_>>(), "the command line of §4");

    // The table: "| `--port` | … | 23800 |", "| `--on-layer` / `--off-layer` | … | `nicola` / `base` |". Only the
    // rows of options: §4 has other tables.
    let mut table = BTreeMap::new();
    for row in section.lines().filter(|l| l.starts_with("| `-")) {
        let cells: Vec<&str> = row.trim_matches('|').split(" | ").map(str::trim).collect();
        let options: Vec<&str> = cells[0].split('`').skip(1).step_by(2).collect();
        let defaults: Vec<&str> = cells[2].split(" / ").map(|d| d.trim_matches('`')).collect();
        for (i, option) in options.into_iter().enumerate() {
            let default = defaults.get(i).filter(|d| **d != "—").map(|d| d.to_string());
            table.insert(option.to_string(), default);
        }
    }
    assert_eq!(table, help, "the table of §4 (option and default)");
}

#[test]
fn version_is_that_of_the_package() {
    for flag in ["--version", "-V"] {
        let out = Command::new(env!("CARGO_BIN_EXE_nicola-imesync")).arg(flag).output().unwrap();
        assert!(out.status.success(), "{flag}");
        assert_eq!(String::from_utf8(out.stdout).unwrap(), format!("nicola-imesync {}\n", env!("CARGO_PKG_VERSION")));
    }
}

#[test]
fn h_is_the_same_as_help() {
    let run = |flag: &str| Command::new(env!("CARGO_BIN_EXE_nicola-imesync")).arg(flag).output().unwrap();
    let (h, help) = (run("-h"), run("--help"));
    assert!(h.status.success() && help.status.success());
    assert_eq!(h.stdout, help.stdout);
}
