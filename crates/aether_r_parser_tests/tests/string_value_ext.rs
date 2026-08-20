//! `RStringValue`'s content accessors, against the tree the parser really builds

use aether_r_parser::{RParserOptions, parse};
use aether_r_syntax::{RStringValue, TextRange, TextSize};
use biome_rowan::AstNode;

fn string_value(source: &str) -> RStringValue {
    let parsed = parse(source, RParserOptions::default());
    assert!(
        parsed.diagnostics().is_empty(),
        "{source:?} did not parse cleanly: {:?}",
        parsed.diagnostics(),
    );

    parsed
        .syntax()
        .descendants()
        .find_map(RStringValue::cast)
        .unwrap_or_else(|| panic!("{source:?} has no string"))
}

#[track_caller]
fn assert_inner(source: &str, expected: &str, range: TextRange) {
    let value = string_value(source);

    assert_eq!(value.inner_string_text().unwrap().text(), expected);
    assert_eq!(value.inner_string_range().unwrap(), range);
    assert_eq!(&source[range], expected);
}

fn range(start: u32, end: u32) -> TextRange {
    TextRange::new(TextSize::from(start), TextSize::from(end))
}

#[test]
fn quoted_strings() {
    assert_inner("\"foo\"", "foo", range(1, 4));
    assert_inner("'foo'", "foo", range(1, 4));
}

#[test]
fn empty_strings() {
    assert_inner("''", "", range(1, 1));
    assert_inner("\"\"", "", range(1, 1));
    assert_inner("r\"()\"", "", range(3, 3));
}

#[test]
fn raw_strings() {
    assert_inner("r\"(a)\"", "a", range(3, 4));
    assert_inner("r\"---(a)---\"", "a", range(6, 7));
    assert_inner("R'--[a]--'", "a", range(5, 6));
}

/// A closing bracket that does not close belongs to the content
#[test]
fn a_raw_string_containing_its_own_bracket() {
    assert_inner("r\"(())\"", "()", range(3, 5));
}

#[test]
fn multiline_strings() {
    assert_inner("\"a\nb\"", "a\nb", range(1, 4));
    assert_inner("r\"(a\nb)\"", "a\nb", range(3, 6));
}

/// Leading trivia is not content
#[test]
fn the_range_is_absolute() {
    assert_inner("f(\n  x = \"foo\"\n)", "foo", range(10, 13));
}

/// Escapes are handed back exactly as written
#[test]
fn nothing_is_unescaped() {
    assert_inner("\"a\\nb\"", "a\\nb", range(1, 5));
    assert_inner("\"a\\\"b\"", "a\\\"b", range(1, 5));
}
