//! `checkpoint()` and `rewind()` have to carry every field of
//! [`LexerCheckpoint`], and the two line-break fields are not redundant:
//! `current_flags` records what was stamped on the checkpointed token, while
//! `after_line_break` is the sticky state lexing resumes from
//!
//! The lexer holds no mutable state outside that checkpoint, so a rewind is
//! total. `RLexer` is a function of its position and the context it is called
//! with, exactly as `JsLexer` is

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
    assert!(lexer.after_line_break);

    // Checkpointed with the break still pending, and consumed straight after
    let checkpoint = lexer.checkpoint();
    assert_eq!(lexer.next_token(RLexContext::Regular), R_DOUBLE_LITERAL);
    assert!(lexer.has_preceding_line_break());
    assert!(!lexer.after_line_break);

    lexer.rewind(checkpoint);
    assert!(lexer.after_line_break);
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
/// the parser reporting an error about text it decided not to lex that way
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

/// `BufferedLexer` lexes lookahead in `RLexContext::default()` and rewinds the
/// moment a non-regular context is requested, which is every string body. A
/// lexer that remembered the open string would come back from that rewind
/// remembering the *last* string the lookahead ran over — here the raw one — and
/// lex the body against the wrong delimiter. Reading the delimiter off the
/// `STRING_OPEN` token instead is what makes the rewind total.
#[test]
fn a_lookahead_over_a_string_leaves_nothing_behind() {
    let source = "\"x\" r\"(y)\"";
    let mut lexer = RLexer::from_str(source);

    assert_eq!(lexer.next_token(RLexContext::Regular), STRING_OPEN);
    let delimiter = RStringDelimiter::from_open(&source[lexer.current_range()]);
    assert_eq!(delimiter, RStringDelimiter::Double);
    let checkpoint = lexer.checkpoint();

    // The lookahead pass, every token of it in the regular context
    for _ in 0..4 {
        lexer.next_token(RLexContext::Regular);
    }

    lexer.rewind(checkpoint);
    assert_eq!(lexer.current(), STRING_OPEN);

    let content = RLexContext::StringContent(RStringDelimiter::Double);
    assert_eq!(lexer.next_token(content), STRING_CONTENT);
    assert_eq!(lexer.current_range(), TextRange::new(1.into(), 2.into()));

    let end = RLexContext::StringEnd(RStringDelimiter::Double);
    assert_eq!(lexer.next_token(end), STRING_CLOSE);
    assert_eq!(lexer.current_range(), TextRange::new(2.into(), 3.into()));
}

/// Every form of `STRING_OPEN` the lexer emits reads back as the delimiter it
/// was lexed with
#[test]
fn a_string_open_reads_back_as_its_delimiter() {
    assert_eq!(RStringDelimiter::from_open("'"), RStringDelimiter::Single);
    assert_eq!(RStringDelimiter::from_open("\""), RStringDelimiter::Double);
    assert_eq!(
        RStringDelimiter::from_open("r\"("),
        RStringDelimiter::Raw {
            quote: b'"',
            hyphens: 0,
            bracket: RawStringBracket::Paren,
        }
    );
    assert_eq!(
        RStringDelimiter::from_open("R'---["),
        RStringDelimiter::Raw {
            quote: b'\'',
            hyphens: 3,
            bracket: RawStringBracket::Bracket,
        }
    );
}

/// The `None` that sends an `r` down the identifier arm instead
#[test]
fn a_name_is_not_a_raw_string_open() {
    assert_eq!(RStringDelimiter::try_from_raw_string_open(""), None);
    assert_eq!(RStringDelimiter::try_from_raw_string_open("rnorm"), None);
    assert_eq!(RStringDelimiter::try_from_raw_string_open("r\"-x("), None);
    assert_eq!(RStringDelimiter::try_from_raw_string_open("\""), None);
}
