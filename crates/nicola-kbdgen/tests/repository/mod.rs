//! The files and documents of the repository (shared by the tests that read the documents).

use std::fs;
use std::path::Path;

pub fn repo() -> &'static Path {
    Path::new(concat!(env!("CARGO_MANIFEST_DIR"), "/../.."))
}

/// The files of the repository, as paths from its root ("docs/usage.md"), without the build output (`target/`) and
/// `.git/`.
pub fn files() -> Vec<String> {
    fn walk(dir: &Path, prefix: &str, files: &mut Vec<String>) {
        for e in fs::read_dir(dir).unwrap() {
            let e = e.unwrap();
            let name = e.file_name().into_string().unwrap();
            let path = format!("{prefix}{name}");
            if e.file_type().unwrap().is_dir() {
                if !matches!(name.as_str(), "target" | ".git") {
                    walk(&e.path(), &format!("{path}/"), files);
                }
            } else {
                files.push(path);
            }
        }
    }
    let mut files = vec![];
    walk(repo(), "", &mut files);
    files.sort();
    files
}

/// The Markdown documents: every `.md` file of the repository (README.md, docs/, platform/, the issue templates), so
/// that a new one is checked without a change here.
pub fn documents() -> Vec<String> {
    files().into_iter().filter(|f| f.ends_with(".md")).collect()
}
