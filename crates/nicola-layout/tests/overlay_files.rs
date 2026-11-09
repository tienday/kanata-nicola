//! The overlay files shipped with the repository load and give the intended layout.

use nicola_layout::{LayoutDef, Thumb, NICOLA_J};

const PRACTICAL: &str = include_str!("../../../layouts/overlays/practical.toml");
const LAG_TOLERANT: &str = include_str!("../../../layouts/overlays/lag-tolerant.toml");

fn faces(l: &LayoutDef, key: &str) -> [Option<String>; 3] {
    match l.keys.iter().find(|k| k.name == key) {
        Some(k) => [k.faces.solo.clone(), k.faces.left.clone(), k.faces.right.clone()],
        None => [None, None, None],
    }
}

#[test]
fn practical_and_lag_tolerant_give_the_intended_layout() {
    let mut l = LayoutDef::parse(NICOLA_J).unwrap();
    l.apply_overlay(PRACTICAL).unwrap();
    l.apply_overlay(LAG_TOLERANT).unwrap();
    let s = |v: &str| Some(v.to_string());
    // The keys that the two files change (docs/layout-overlays.md).
    assert_eq!(faces(&l, "KEY_SEMICOLON"), [s("ん"), None, None]);
    assert_eq!(faces(&l, "KEY_COMMA"), [s("っ"), s("ぺ"), s("む")]);
    assert_eq!(faces(&l, "KEY_X"), [s("ひ"), s("ね"), s("び")]);
    assert_eq!(faces(&l, "KEY_C"), [s("す"), s("ろ"), s("す")]);
    assert_eq!(faces(&l, "KEY_Z"), [s("ぽ"), s("ぅ"), s("ず")]);
    assert_eq!(faces(&l, "KEY_L"), [s("い"), s("い"), s("ょ")]);
    assert_eq!(faces(&l, "KEY_P"), [s("、"), s("ぴ"), s("ぇ")]);
    assert_eq!(faces(&l, "KEY_Q"), [s("。"), s("ぁ"), None]);
    assert_eq!(faces(&l, "KEY_RIGHTBRACE"), [None, None, None]);
    assert_eq!(faces(&l, "KEY_SLASH"), [s("・"), None, s("ぉ")]);
    assert_eq!(faces(&l, "KEY_0"), [None, None, None]);
    assert_eq!(faces(&l, "KEY_MINUS"), [None, None, None]);
    // Keys not written stay standard.
    assert_eq!(l.lookup("KEY_W", Some(Thumb::Right)).unwrap(), "が");
}

const DOC: &str = include_str!("../../../docs/layout-overlays.md");

/// The cells of the table of docs/layout-overlays.md whose header line starts with `header`: the header, then the rows.
fn table(header: &str) -> Vec<Vec<&'static str>> {
    let mut lines = DOC.lines().skip_while(|l| !l.starts_with(header));
    let head = lines.next().unwrap_or_else(|| panic!("no table headed {header:?} in docs/layout-overlays.md"));
    std::iter::once(head)
        .chain(lines.skip(1).take_while(|l| l.starts_with('|')))
        .map(|l| l.trim().trim_matches('|').split('|').map(str::trim).collect())
        .collect()
}

/// The JIS position of a key cell such as "`,`（B08）".
fn pos(cell: &str) -> &str {
    cell.rsplit_once('（').and_then(|(_, p)| p.strip_suffix('）')).unwrap_or_else(|| panic!("no position in {cell:?}"))
}

/// The positions of the rows of a table, sorted.
fn positions(rows: &[Vec<&str>]) -> Vec<String> {
    let mut v: Vec<String> = rows[1..].iter().map(|r| pos(r[0]).to_string()).collect();
    v.sort();
    v
}

fn at(l: &LayoutDef, pos: &str) -> [Option<String>; 3] {
    l.keys.iter().find(|k| k.pos == pos).map_or([None, None, None], |k| faces(l, &k.name))
}

/// The positions of the keys whose faces differ between two layouts, sorted.
fn changed(before: &LayoutDef, after: &LayoutDef) -> Vec<String> {
    let mut v: Vec<String> =
        after.keys.iter().filter(|k| at(before, &k.pos) != at(after, &k.pos)).map(|k| k.pos.clone()).collect();
    v.sort();
    v
}

/// Checks a cell of faces such as "単独・右親指" against a cell of their characters such as "゛・゜" ("未定義" for
/// faces left undefined).
fn check(l: &LayoutDef, table: &str, key: &str, names: &str, chars: &str) {
    let names: Vec<usize> = names
        .split('・')
        .map(|n| ["単独", "左親指", "右親指"].iter().position(|f| *f == n).unwrap_or_else(|| panic!("face {n:?}")))
        .collect();
    let chars: Vec<Option<String>> = match chars {
        "未定義" => vec![None; names.len()],
        _ => chars.split('・').map(|c| Some(c.to_string())).collect(),
    };
    assert_eq!(names.len(), chars.len(), "{table}: {key}");
    for (i, c) in names.into_iter().zip(chars) {
        assert_eq!(at(l, pos(key))[i], c, "{table}: {key}");
    }
}

/// The tables of docs/layout-overlays.md give the faces before and after its first example, and the faces that
/// the shipped files change (all the keys each file changes, before and after it); its examples load.
#[test]
fn docs_overlay_tables_give_the_faces() {
    let layout = |overlays: &[&str]| {
        let mut l = LayoutDef::parse(NICOLA_J).unwrap();
        for o in overlays {
            l.apply_overlay(o).unwrap();
        }
        l
    };
    let standard = layout(&[]);
    let practical = layout(&[PRACTICAL]);
    let lag_tolerant = layout(&[PRACTICAL, LAG_TOLERANT]);

    let rows = table("| キー（位置） | 面 | 標準 | practical |");
    for r in &rows[1..] {
        check(&standard, "practical", r[0], r[1], r[2]);
        check(&practical, "practical", r[0], r[1], r[3]);
    }
    assert_eq!(positions(&rows), changed(&standard, &practical), "the keys that practical changes");

    // Faces such as "ん / 未定義 / っ" (solo / left / right; "未定義" for none).
    let three = |cell: &str| -> [Option<String>; 3] {
        let v: Vec<Option<String>> = cell.split(" / ").map(|c| (c != "未定義").then(|| c.to_string())).collect();
        v.try_into().unwrap_or_else(|v| panic!("not three faces: {v:?}"))
    };
    let rows = table("| キー（位置） | 標準（単独 / 左親指 / 右親指） | lag-tolerant |");
    for r in &rows[1..] {
        assert_eq!(at(&standard, pos(r[0])), three(r[1]), "lag-tolerant: {}", r[0]);
        assert_eq!(at(&lag_tolerant, pos(r[0])), three(r[2]), "lag-tolerant: {}", r[0]);
    }
    assert_eq!(positions(&rows), changed(&practical, &lag_tolerant), "the keys that lag-tolerant changes");

    // The examples (```toml blocks) are overlay files that load.
    let examples: Vec<&str> = DOC.split("```toml\n").skip(1).map(|b| b.split("```").next().unwrap()).collect();
    assert_eq!(examples.len(), 2, "the ```toml blocks of docs/layout-overlays.md");
    for e in &examples {
        LayoutDef::parse(NICOLA_J).unwrap().apply_overlay(e).unwrap_or_else(|err| panic!("{err}\n{e}"));
    }

    // How overlaying works: the faces of a key (as "標準配列（Q キー）") in the standard layout, and with the first
    // example laid over it (a face in bold, `**…**`, is the face itself).
    let rows = table("| | 単独面 | 左親指の面 | 右親指の面 |");
    let key =
        rows[1][0].strip_prefix("標準配列（").and_then(|k| k.strip_suffix(" キー）")).expect("標準配列（<key> キー）");
    let key = format!("KEY_{key}");
    let overlaid = layout(&[examples[0]]);
    let cells = |r: &[&str]| -> [Option<String>; 3] { [1, 2, 3].map(|i| Some(r[i].trim_matches('*').to_string())) };
    assert_eq!((rows[1][0], faces(&standard, &key)), (rows[1][0], cells(&rows[1])));
    assert_eq!((rows[3][0], faces(&overlaid, &key)), ("重ねた結果", cells(&rows[3])));
}
