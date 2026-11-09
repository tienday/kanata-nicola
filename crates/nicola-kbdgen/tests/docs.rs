//! References between the documents (every `.md` file of the repository): a link to a section
//! (`[設計書 §3](design.md#3-nicola-規格との違い)`, `[§2.1](#21-権限の設定)` in the same document) names a heading
//! that exists, with the same number; a section is always a link, except the NICOLA specification's and those in a
//! diagram, which cannot have one; and the other files (scripts, sources, `kbd/`, `layouts/`) name documents and
//! sections that exist (`docs/settings.md`, `docs/usage.md §2.1`).

mod repository;

use std::collections::HashMap;
use std::fs;
use std::path::{Component, Path};

use repository::{documents, files, repo};

/// The lines of a document with their numbers (from 1), each with the info string of the fenced code block it is in
/// (`Some("mermaid")`; `Some("")` for a block without one), or `None` outside code blocks.
fn lines(text: &str) -> Vec<(usize, &str, Option<&str>)> {
    let mut block = None;
    let mut found = vec![];
    for (i, line) in text.lines().enumerate() {
        if let Some(info) = line.trim_start().strip_prefix("```") {
            block = if block.is_some() { None } else { Some(info.trim()) };
        } else {
            found.push((i + 1, line, block));
        }
    }
    found
}

/// The anchor GitHub makes for a heading (as github-slugger, which VS Code uses too): lowercase; letters, numbers,
/// `_`, `-` and spaces are kept, the rest (punctuation, symbols) dropped; spaces become `-`. Marks and connectors
/// other than `_` are dropped here, unlike GitHub; the documents have none in their headings.
fn slug(heading: &str) -> String {
    heading
        .to_lowercase()
        .chars()
        .filter(|c| c.is_alphanumeric() || matches!(c, '_' | '-' | ' '))
        .map(|c| if c == ' ' { '-' } else { c })
        .collect()
}

/// The anchors of a document's headings, each with the section number of its heading if it has one ("3.1" for
/// "### 3.1 …", "1" for "## 1. …"). A repeated anchor gets "-1", "-2", … as on GitHub.
fn anchors(text: &str) -> HashMap<String, Option<String>> {
    let mut found = HashMap::new();
    let mut repeats: HashMap<String, usize> = HashMap::new();
    for (_, line, _) in lines(text).into_iter().filter(|(_, _, block)| block.is_none()) {
        let level = line.chars().take_while(|&c| c == '#').count();
        let Some(heading) = line.get(level..).and_then(|h| h.strip_prefix(' ')) else { continue };
        if level == 0 || level > 6 {
            continue;
        }
        let heading = heading.trim_end().trim_end_matches('#').trim_end();
        let base = slug(heading);
        let mut anchor = base.clone();
        while found.contains_key(&anchor) {
            let n = repeats.entry(base.clone()).or_default();
            *n += 1;
            anchor = format!("{base}-{n}");
        }
        let digits: String = heading.chars().take_while(|c| c.is_ascii_digit() || *c == '.').collect();
        let number = (!digits.is_empty() && heading[digits.len()..].starts_with(' '))
            .then(|| digits.trim_end_matches('.').to_string());
        found.insert(anchor, number);
    }
    found
}

/// The numbered sections of a document: the anchor of each section number ("2.1": "21-権限の設定").
fn sections(text: &str) -> HashMap<String, String> {
    anchors(text).into_iter().filter_map(|(anchor, number)| Some((number?, anchor))).collect()
}

/// The section number right after a "§" (at `at`, the byte index of the "§"), if any: "2.1" in "§2.1 の".
fn number_after(line: &str, at: usize) -> Option<String> {
    let rest = &line[at + '§'.len_utf8()..];
    let n: String = rest.chars().take_while(|c| c.is_ascii_digit() || *c == '.').collect();
    let n = n.trim_end_matches('.');
    (!n.is_empty()).then(|| n.to_string())
}

/// The inline links of a line: (byte range of the whole link, text, target).
fn links(line: &str) -> Vec<(std::ops::Range<usize>, &str, &str)> {
    let mut found = vec![];
    for (mid, _) in line.match_indices("](") {
        // The "[" that opens the text: the nearest one before, past nested pairs.
        let mut depth = 0;
        let mut start = None;
        for (i, c) in line[..mid].char_indices().rev() {
            match c {
                ']' => depth += 1,
                '[' if depth == 0 => {
                    start = Some(i);
                    break;
                }
                '[' => depth -= 1,
                _ => {}
            }
        }
        let (Some(start), Some(len)) = (start, line[mid + 2..].find(')')) else { continue };
        let end = mid + 2 + len + 1;
        found.push((start..end, &line[start + 1..mid], &line[mid + 2..end - 1]));
    }
    found
}

/// A path relative to a document, as one from the repository root ("../README.md" from "docs/usage.md": "README.md").
fn resolve(doc: &str, path: &str) -> String {
    let mut parts: Vec<String> = vec![];
    for c in Path::new(doc).parent().unwrap().join(path).components() {
        match c {
            Component::ParentDir => {
                parts.pop();
            }
            Component::Normal(p) => parts.push(p.to_str().unwrap().to_string()),
            _ => {}
        }
    }
    parts.join("/")
}

#[test]
fn docs_anchors_are_made_as_on_github() {
    // Checked against github-slugger.
    assert_eq!(slug("3. 例の見出し（abc.def）"), "3-例の見出しabcdef");
    assert_eq!(slug("2.1 テスト・例：A/B"), "21-テスト例ab");
    assert_eq!(slug("4.5 長音ー・繰り返し々「」、＋〜"), "45-長音ー繰り返し々");
    assert_eq!(slug("3.1 NICOLA 入力の ON/OFF"), "31-nicola-入力の-onoff");
    assert_eq!(slug("1. Foo  Bar_baz"), "1-foo--bar_baz");
    let a = anchors("## 1. Foo\n\n```sh\n# not a heading\n```\n\n## 1. Foo\n");
    assert_eq!(a.get("1-foo"), Some(&Some("1".to_string())));
    assert_eq!(a.get("1-foo-1"), Some(&Some("1".to_string())));
    assert_eq!(a.len(), 2);
}

/// Links to a section point to a heading that exists, and the "§N" of the link text is the number of that heading. A
/// section is always a link, of the same document too (`[§2.1](#21-権限の設定)`; not "[設計書](design.md) §3",
/// "同 §3" or a bare "§2.1"), so that a renumbered or renamed heading fails here. The exceptions are the sections of
/// the NICOLA specification ("規格書 §6.1"), and those in a diagram (a mermaid block), which name a section of the same
/// document.
#[test]
fn docs_section_references_exist() {
    let docs = documents();
    let texts: HashMap<&str, String> =
        docs.iter().map(|d| (d.as_str(), fs::read_to_string(repo().join(d)).unwrap())).collect();
    let mut wrong = vec![];
    for doc in &docs {
        let own = sections(&texts[doc.as_str()]);
        for (n, line, block) in lines(&texts[doc.as_str()]) {
            let at = format!("{doc}:{n}");
            if block.is_some() {
                if block == Some("mermaid") {
                    for (i, _) in line.match_indices('§') {
                        if let Some(s) = number_after(line, i).filter(|s| !own.contains_key(s)) {
                            wrong.push(format!("{at}: no §{s} in this document (in a diagram)"));
                        }
                    }
                }
                continue;
            }
            let mut linked = vec![];
            for (range, text, target) in links(line) {
                linked.push(range.clone());
                if target.contains("://") || target.starts_with("mailto:") {
                    continue;
                }
                let (path, fragment) = target.split_once('#').map_or((target, None), |(p, f)| (p, Some(f)));
                let file = if path.is_empty() { doc.clone() } else { resolve(doc, path) };
                if !repo().join(&file).exists() {
                    wrong.push(format!("{at}: no such file: {target}"));
                    continue;
                }
                let section = text.rfind('§').and_then(|i| number_after(text, i));
                if line[range.end..].trim_start().starts_with('§') {
                    wrong.push(format!("{at}: a section of {file} without a link to it (put it in the link)"));
                }
                let Some(fragment) = fragment else {
                    if section.is_some() {
                        wrong.push(format!("{at}: \"{text}\" without the heading's anchor (#…)"));
                    }
                    continue;
                };
                let Some(target_text) = texts.get(file.as_str()) else {
                    wrong.push(format!("{at}: an anchor into a file that is not a document: {target}"));
                    continue;
                };
                match anchors(target_text).get(fragment) {
                    None => wrong.push(format!("{at}: {file} has no heading #{fragment} (renamed?)")),
                    Some(number) if section.is_some() && *number != section => wrong.push(format!(
                        "{at}: \"{text}\", but #{fragment} is §{}",
                        number.as_deref().unwrap_or("(no number)")
                    )),
                    Some(_) => {}
                }
            }
            for (i, _) in line.match_indices('§') {
                // In a link (checked above), a path's section (`docs/usage.md §2.1`, checked below), or a section of
                // the specification.
                let before = line[..i].trim_end();
                if linked.iter().any(|r| r.contains(&i)) || before.ends_with(".md") || before.ends_with("規格書") {
                    continue;
                }
                if before.ends_with('同') {
                    wrong.push(format!("{at}: \"同 §\" is another document's section: link it"));
                } else if let Some(s) = number_after(line, i) {
                    match own.get(&s) {
                        Some(anchor) => wrong.push(format!("{at}: §{s} without a link: [§{s}](#{anchor})")),
                        None => wrong.push(format!("{at}: no §{s} in this document")),
                    }
                }
            }
        }
    }
    assert!(wrong.is_empty(), "section references (docs-style: link to the heading's anchor):\n{}", wrong.join("\n"));
}

/// The files that name a document by its path (the scripts and their messages, the comments of the sources, `kbd/`,
/// `layouts/`, `config.env.example`) name one that exists: a path from the repository root (`docs/settings.md`,
/// `platform/linux/kanata/README.md`), also inside a longer one (`$repo/docs/usage.md`). The sections after it
/// (`docs/usage.md §2.1`, and those listed after it: `§2.4・§2.6`) exist too. A link of a document
/// (`[…](usage.md#…)`) is checked by the test above.
#[test]
fn docs_named_by_path_exist() {
    let mut wrong = vec![];
    for file in files() {
        let Ok(text) = fs::read_to_string(repo().join(&file)) else { continue }; // not text
        for (n, line) in text.lines().enumerate() {
            for (i, _) in line.match_indices(".md") {
                if line[i + 3..].starts_with(|c: char| c.is_ascii_alphanumeric()) {
                    continue; // Not the end of a name (".mdx").
                }
                let name_len =
                    line[..i].chars().rev().take_while(|c| c.is_ascii_alphanumeric() || "-_/.".contains(*c)).count();
                if line[..i - name_len].ends_with("](") {
                    continue; // A link of a document, checked by the test above.
                }
                let path = &line[i - name_len..i + 3]; // ASCII, one byte each

                // The section after the path, and those listed after it in the same document ("§2.4・§2.6").
                let mut named = vec![];
                if line[i + 3..].starts_with(" §") {
                    let mut at = i + 4;
                    while let Some(section) = number_after(line, at) {
                        let end = at + '§'.len_utf8() + section.len();
                        named.push(section);
                        if !line[end..].starts_with("・§") {
                            break;
                        }
                        at = end + '・'.len_utf8();
                    }
                }

                // The path from its "docs/" or "platform/" ("docs/usage.md" in "$repo/docs/usage.md"). Other names
                // are a URL ("//github.com/…/setup-linux.md"), or a file next to the one naming it ("README.md" in
                // platform/linux/kanata/build.sh).
                let from_root = ["docs/", "platform/"]
                    .iter()
                    .filter_map(|d| path.match_indices(d).map(|(j, _)| j).find(|&j| j == 0 || path[..j].ends_with('/')))
                    .min();
                let doc = match from_root {
                    Some(j) if !path.starts_with("//") => &path[j..],
                    _ if !named.is_empty() => path, // A section of a document not named from the root: reported.
                    _ => continue,
                };
                match fs::read_to_string(repo().join(doc)) {
                    Err(_) => wrong.push(format!("{file}:{}: no such document: {path}", n + 1)),
                    Ok(t) => {
                        let present = sections(&t);
                        for section in named.iter().filter(|s| !present.contains_key(*s)) {
                            wrong.push(format!("{file}:{}: {path} has no §{section}", n + 1));
                        }
                    }
                }
            }
        }
    }
    assert!(wrong.is_empty(), "documents named by path:\n{}", wrong.join("\n"));
}
