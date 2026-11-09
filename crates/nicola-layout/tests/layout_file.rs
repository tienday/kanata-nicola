//! The example of the layout definition file in docs/implementation.md is the standard layout as it is.

use nicola_layout::{LayoutDef, NICOLA_J};

const DOC: &str = include_str!("../../../docs/implementation.md");

/// Each part of the example (the `[meta]` table, a key), separated by a blank line, is in layouts/nicola-j.toml
/// unchanged, and the example loads.
#[test]
fn docs_implementation_example_is_the_standard_layout() {
    let examples: Vec<&str> = DOC.split("```toml\n").skip(1).map(|b| b.split("```").next().unwrap()).collect();
    assert_eq!(examples.len(), 1, "the ```toml blocks of docs/implementation.md");
    for part in examples[0].split("\n\n") {
        assert!(NICOLA_J.contains(part.trim_end()), "not in layouts/nicola-j.toml as it is:\n{part}");
    }
    LayoutDef::parse(examples[0]).unwrap();
}
