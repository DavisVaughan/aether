//! The four-way dispatch on a leading `.`.
//!
//! `dots` and `dot_dot_i` overlap with the identifier rule in tree-sitter-r, and
//! tree-sitter resolves the overlap by match length, with equal-length matches
//! going to whichever rule is declared first. Their grammar has a comment saying
//! exactly that. Every expectation below was checked against `tree-sitter parse`
//! at the pinned revision.

use super::*;

#[test]
fn dispatch() {
    assert_lex!(
        "... ..1 .5 .foo . ..",
        DOTS: 3,
        WHITESPACE: 1,
        DOTDOTI: 3,
        WHITESPACE: 1,
        R_DOUBLE_LITERAL: 2,
        WHITESPACE: 1,
        IDENT: 4,
        WHITESPACE: 1,
        IDENT: 1,
        WHITESPACE: 1,
        IDENT: 2,
    );
}

#[test]
fn dot_dot_i_takes_every_digit() {
    assert_lex!("..1", DOTDOTI: 3);
    assert_lex!("..12", DOTDOTI: 4);
}

/// The overlap with the identifier rule, resolved by length. A longer identifier
/// match wins outright; an equal-length one loses to the more specific rule.
#[test]
fn a_longer_identifier_match_wins() {
    assert_lex!("...", DOTS: 3);
    assert_lex!("....", IDENT: 4);
    assert_lex!("...a", IDENT: 4);
    assert_lex!("...1", IDENT: 4);
    assert_lex!("..1a", IDENT: 4);
    assert_lex!("..1.", IDENT: 4);
    assert_lex!("..1e5", IDENT: 5);
}

/// A digit after the `.` always makes a number, never a name. tree-sitter-r
/// orders the rules that way on purpose: otherwise `.1L` and `.1i` lex as
/// identifiers (their #190).
#[test]
fn a_digit_after_the_dot_makes_a_number() {
    assert_lex!(".5", R_DOUBLE_LITERAL: 2);
    assert_lex!(".1L", R_INTEGER_LITERAL: 3);
    assert_lex!(".1i", R_COMPLEX_LITERAL: 3);
    // The number stops at the first character it cannot use, and a name starts
    // there — `.` matches only one byte of an identifier, so the number is
    // always the longer match
    assert_lex!(".5abc", R_DOUBLE_LITERAL: 2, IDENT: 3);
}

#[test]
fn dots_in_context() {
    assert_lex!(
        "f(..., ..1)",
        IDENT: 1,
        L_PAREN: 1,
        DOTS: 3,
        COMMA: 1,
        WHITESPACE: 1,
        DOTDOTI: 3,
        R_PAREN: 1,
    );
}
