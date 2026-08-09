//! Raw strings: `r`, a quote, 0–255 hyphens, and one of `([{`.
//!
//! Ported from `scan_raw_string_open` and `scan_raw_string_content_or_close` in
//! tree-sitter-r's `src/scanner.c`.

use super::*;

#[test]
fn every_bracket_pair() {
    assert_lex!("r\"(abc)\"", STRING_OPEN: 3, STRING_CONTENT: 3, STRING_CLOSE: 2);
    assert_lex!("r\"[abc]\"", STRING_OPEN: 3, STRING_CONTENT: 3, STRING_CLOSE: 2);
    assert_lex!("r\"{abc}\"", STRING_OPEN: 3, STRING_CONTENT: 3, STRING_CLOSE: 2);
}

#[test]
fn hyphen_counts() {
    assert_lex!("r\"(a)\"", STRING_OPEN: 3, STRING_CONTENT: 1, STRING_CLOSE: 2);
    assert_lex!("r\"-(a)-\"", STRING_OPEN: 4, STRING_CONTENT: 1, STRING_CLOSE: 3);
    assert_lex!("r\"---(a)---\"", STRING_OPEN: 6, STRING_CONTENT: 1, STRING_CLOSE: 5);
}

#[test]
fn uppercase_prefix_and_single_quotes() {
    assert_lex!("R'---[x]---'", STRING_OPEN: 6, STRING_CONTENT: 1, STRING_CLOSE: 5);
    assert_lex!("R\"(x)\"", STRING_OPEN: 3, STRING_CONTENT: 1, STRING_CLOSE: 2);
}

#[test]
fn an_empty_body_has_no_content_token() {
    assert_lex!("r\"()\"", STRING_OPEN: 3, STRING_CLOSE: 2);
    assert_lex!("r\"-()-\"", STRING_OPEN: 4, STRING_CLOSE: 3);
}

/// A closing bracket that turns out not to start the closing sequence is
/// content, and the scan resumes from where the attempt stopped rather than from
/// the byte after the bracket. Getting that wrong makes both of these
/// unterminated (tree-sitter-r's #162).
#[test]
fn a_bracket_that_does_not_close() {
    assert_lex!("r\"(())\"", STRING_OPEN: 3, STRING_CONTENT: 2, STRING_CLOSE: 2);
    assert_lex!("r\"-())-\"", STRING_OPEN: 4, STRING_CONTENT: 1, STRING_CLOSE: 3);
}

/// The bracket matches but the hyphen count does not, so the string never
/// closes and the content runs to the end of the file.
#[test]
fn the_hyphen_count_has_to_match() {
    assert_lex!("r\"-(a)\"", STRING_OPEN: 4, STRING_CONTENT: 3);
}

#[test]
fn unterminated_runs_to_the_end_of_the_file() {
    assert_lex!("r\"(abc", STRING_OPEN: 3, STRING_CONTENT: 3);
}

/// The prefix has to be complete, so anything short of it falls through to the
/// identifier and punctuation arms.
#[test]
fn an_incomplete_prefix_is_just_a_name() {
    assert_lex!("rnorm", IDENT: 5);
    assert_lex!("r + 1", IDENT: 1, WHITESPACE: 1, PLUS: 1, WHITESPACE: 1, R_DOUBLE_LITERAL: 1);
    // A quote with no bracket after it: an ordinary name and an ordinary string.
    assert_lex!(
        "r\"abc\"",
        IDENT: 1,
        STRING_OPEN: 1,
        STRING_CONTENT: 3,
        STRING_CLOSE: 1,
    );
    assert_lex!("r\"-abc\"", IDENT: 1, STRING_OPEN: 1, STRING_CONTENT: 4, STRING_CLOSE: 1);
}

/// Escapes are not special inside a raw string; that is the point of one.
#[test]
fn backslashes_are_literal() {
    assert_lex!("r\"(a\\)\"", STRING_OPEN: 3, STRING_CONTENT: 2, STRING_CLOSE: 2);
}

#[test]
fn raw_strings_may_span_newlines() {
    assert_lex!("r\"(a\nb)\"", STRING_OPEN: 3, STRING_CONTENT: 3, STRING_CLOSE: 2);
}
