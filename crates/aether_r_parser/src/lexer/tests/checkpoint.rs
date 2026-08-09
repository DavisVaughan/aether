//! `checkpoint()` and `rewind()` have to carry every field of
//! [`LexerCheckpoint`], and the two line-break fields are not redundant:
//! `current_flags` records what was stamped on the checkpointed token, while
//! `after_line_break` is the sticky state lexing resumes from.

use super::*;

#[test]
fn rewind_restores_the_current_token() {
    let mut lexer = RLexer::from_str("a + b");

    assert_eq!(lexer.next_token(RLexContext::Regular), IDENT);
    let checkpoint = lexer.checkpoint();

    assert_eq!(lexer.next_token(RLexContext::Regular), WHITESPACE);
    assert_eq!(lexer.next_token(RLexContext::Regular), PLUS);

    lexer.rewind(checkpoint);
    assert_eq!(lexer.current(), IDENT);
    assert_eq!(lexer.current_range(), TextRange::new(0.into(), 1.into()));
    assert_eq!(lexer.next_token(RLexContext::Regular), WHITESPACE);
}

/// Restore `after_line_break` or a rewind across a newline silently loses the
/// break. That bug is invisible until `else` attachment depends on it, and
/// silent even then.
#[test]
fn rewind_restores_the_sticky_line_break() {
    let mut lexer = RLexer::from_str("1\n2");

    assert_eq!(lexer.next_token(RLexContext::Regular), R_DOUBLE_LITERAL);
    assert_eq!(lexer.next_token(RLexContext::Regular), NEWLINE);
    assert!(lexer.after_newline);

    // Checkpointed with the break still pending, and consumed straight after.
    let checkpoint = lexer.checkpoint();
    assert_eq!(lexer.next_token(RLexContext::Regular), R_DOUBLE_LITERAL);
    assert!(lexer.has_preceding_line_break());
    assert!(!lexer.after_newline);

    lexer.rewind(checkpoint);
    assert!(lexer.after_newline);
    assert_eq!(lexer.next_token(RLexContext::Regular), R_DOUBLE_LITERAL);
    assert!(lexer.has_preceding_line_break());
}

#[test]
fn rewind_restores_the_current_flags() {
    let mut lexer = RLexer::from_str("1\n2 3");

    assert_eq!(lexer.next_token(RLexContext::Regular), R_DOUBLE_LITERAL);
    assert_eq!(lexer.next_token(RLexContext::Regular), NEWLINE);
    assert_eq!(lexer.next_token(RLexContext::Regular), R_DOUBLE_LITERAL);
    let checkpoint = lexer.checkpoint();

    assert_eq!(lexer.next_token(RLexContext::Regular), WHITESPACE);
    assert_eq!(lexer.next_token(RLexContext::Regular), R_DOUBLE_LITERAL);
    assert!(!lexer.has_preceding_line_break());

    lexer.rewind(checkpoint);
    assert!(lexer.has_preceding_line_break());
}

#[test]
fn rewind_restores_the_bom_length() {
    let mut lexer = RLexer::from_str("\u{FEFF}1");

    assert_eq!(lexer.next_token(RLexContext::Regular), UNICODE_BOM);
    assert_eq!(lexer.unicode_bom_length, 3);
    let checkpoint = lexer.checkpoint();

    assert_eq!(lexer.next_token(RLexContext::Regular), R_DOUBLE_LITERAL);

    lexer.rewind(checkpoint);
    assert_eq!(lexer.unicode_bom_length, 3);
}

/// Diagnostics raised after the checkpoint are dropped, so a rewind cannot leave
/// the parser reporting an error about text it decided not to lex that way.
#[test]
fn rewind_truncates_diagnostics() {
    let mut lexer = RLexer::from_str("a %in");

    assert_eq!(lexer.next_token(RLexContext::Regular), IDENT);
    assert_eq!(lexer.next_token(RLexContext::Regular), WHITESPACE);
    let checkpoint = lexer.checkpoint();

    assert_eq!(lexer.next_token(RLexContext::Regular), SPECIAL);
    assert_eq!(lexer.diagnostics.len(), 1);

    lexer.rewind(checkpoint);
    assert_eq!(lexer.diagnostics.len(), 0);
}

/// The string delimiter is *not* part of the checkpoint, which is sound only
/// because lookahead lexes in the regular context and so can never be used to
/// peek past a `STRING_OPEN`. Pin the states that are reachable.
#[test]
fn the_string_delimiter_tracks_the_open_and_close() {
    let mut lexer = RLexer::from_str("\"a\"");
    assert_eq!(lexer.string_delimiter, None);

    assert_eq!(lexer.next_token(RLexContext::Regular), STRING_OPEN);
    assert_eq!(lexer.string_delimiter, Some(RStringDelimiter::Double));

    let content = RLexContext::StringContent(RStringDelimiter::Double);
    assert_eq!(lexer.next_token(content), STRING_CONTENT);
    assert_eq!(lexer.string_delimiter, Some(RStringDelimiter::Double));

    let end = RLexContext::StringEnd(RStringDelimiter::Double);
    assert_eq!(lexer.next_token(end), STRING_CLOSE);
    assert_eq!(lexer.string_delimiter, None);
}
