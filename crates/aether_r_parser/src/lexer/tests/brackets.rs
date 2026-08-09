//! `[[` and `]]` are lexed greedily and split by the parser.
//!
//! tree-sitter-r resolves them in its external scanner with a scope stack,
//! because `x[[1]]`, `x[ [1] ]` and `x[[1], 2]` are genuinely ambiguous. Aether
//! keeps the lexer ignorant and gives the parser [`RReLexContext::SplitBrackets`]
//! for the one place the nesting demands a split.

use super::*;

#[test]
fn adjacent_brackets_are_one_token() {
    assert_lex!("[[", L_BRACK2: 2);
    assert_lex!("]]", R_BRACK2: 2);
}

#[test]
fn separated_brackets_are_not() {
    assert_lex!(
        "x[ [1] ]",
        IDENT: 1,
        L_BRACK: 1,
        WHITESPACE: 1,
        L_BRACK: 1,
        R_DOUBLE_LITERAL: 1,
        R_BRACK: 1,
        WHITESPACE: 1,
        R_BRACK: 1,
    );
}

#[test]
fn a_third_bracket_stands_alone() {
    assert_lex!("[[[", L_BRACK2: 2, L_BRACK: 1);
    assert_lex!("]]]", R_BRACK2: 2, R_BRACK: 1);
}

#[test]
fn split_leaves_the_second_bracket_for_the_next_token() {
    let mut lexer = RLexer::from_str("[[a");

    assert_eq!(lexer.next_token(RLexContext::Regular), L_BRACK2);
    assert_eq!(lexer.re_lex(RReLexContext::SplitBrackets), L_BRACK);
    assert_eq!(lexer.current_range(), TextRange::new(0.into(), 1.into()));

    assert_eq!(lexer.next_token(RLexContext::Regular), L_BRACK);
    assert_eq!(lexer.current_range(), TextRange::new(1.into(), 2.into()));
    assert_eq!(lexer.next_token(RLexContext::Regular), IDENT);
}

#[test]
fn split_is_idempotent() {
    let mut lexer = RLexer::from_str("]]");

    assert_eq!(lexer.next_token(RLexContext::Regular), R_BRACK2);
    assert_eq!(lexer.re_lex(RReLexContext::SplitBrackets), R_BRACK);
    assert_eq!(lexer.re_lex(RReLexContext::SplitBrackets), R_BRACK);
    assert_eq!(lexer.current_range(), TextRange::new(0.into(), 1.into()));
}

#[test]
fn split_is_a_no_op_on_anything_else() {
    let mut lexer = RLexer::from_str("foo");

    assert_eq!(lexer.next_token(RLexContext::Regular), IDENT);
    assert_eq!(lexer.re_lex(RReLexContext::SplitBrackets), IDENT);
    assert_eq!(lexer.current_range(), TextRange::new(0.into(), 3.into()));
}
