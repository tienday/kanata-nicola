//! The options of nicola-imesync that docs/implementation.md §4 gives (the command line and the table, with the
//! defaults) are those of its `--help`.

use std::collections::BTreeMap;
use std::process::Command;

const DOC: &str = include_str!("../../../docs/implementation.md");

/// The options of `--help`, each with its default if it has one ("--port": Some("23800")).
fn help_options() -> BTreeMap<String, Option<String>> {
    let out = Command::new(env!("CARGO_BIN_EXE_nicola-imesync")).arg("--help").output().unwrap();
    let help = String::from_utf8(out.stdout).unwrap();
    let mut options = BTreeMap::new();
    let mut current = None;
    for line in help.lines() {
        // An option starts a line after two spaces; its description may go on in the next lines.
        if let Some(option) = line.strip_prefix("  -").and_then(|l| l.split_whitespace().next()) {
            current = Some(format!("-{option}"));
            options.insert(format!("-{option}"), None);
        }
        let default = line.split("(default ").nth(1).map(|d| d.split([')', ';']).next().unwrap().to_string());
        if let (Some(option), Some(default)) = (&current, default) {
            options.insert(option.clone(), Some(default));
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
    assert_eq!(in_line, names(&help), "the command line of §4");

    // The table: "| `--port` | … | 23800 |", "| `--on-layer` / `--off-layer` | … | `nicola` / `base` |". Only the
    // rows of options: §4 has other tables.
    let mut table = BTreeMap::new();
    for row in section.lines().filter(|l| l.starts_with("| `-")) {
        let cells: Vec<&str> = row.trim_matches('|').split(" | ").map(str::trim).collect();
        let options: Vec<&str> = cells[0].split('`').skip(1).step_by(2).collect();
        let defaults: Vec<&str> = cells[2].split(" / ").map(|d| d.trim_matches('`')).collect();
        for (i, option) in options.into_iter().filter(|o| !matches!(*o, "-h" | "--help")).enumerate() {
            let default = defaults.get(i).filter(|d| **d != "—").map(|d| d.to_string());
            table.insert(option.to_string(), default);
        }
    }
    assert_eq!(table, help, "the table of §4 (option and default)");
}
