//! Scratchpad. Run it with `just quick`.

use aether_r_parser::{RParserOptions, parse};

#[ignore]
#[test]
fn quick_test() {
    let source = r#"
1$foo
1$2
"#;

    let parsed = parse(source, RParserOptions::default());
    println!("{:#?}", parsed.tree());

    for diagnostic in parsed.diagnostics() {
        println!("{diagnostic:?}");
    }
}
