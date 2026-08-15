use super::*;

#[test]
fn double_quoted() {
    assert_lex!("\"a\"", STRING_OPEN: 1, STRING_CONTENT: 1, STRING_CLOSE: 1);
}

#[test]
fn single_quoted() {
    assert_lex!("'abc'", STRING_OPEN: 1, STRING_CONTENT: 3, STRING_CLOSE: 1);
}

/// An empty body emits no `STRING_CONTENT` at all, which is what makes
/// `RStringValue`'s optional `content` slot meaningful
#[test]
fn empty_strings_have_no_content_token() {
    assert_lex!("\"\"", STRING_OPEN: 1, STRING_CLOSE: 1);
    assert_lex!("''", STRING_OPEN: 1, STRING_CLOSE: 1);
}

#[test]
fn escapes_stay_inside_the_content() {
    assert_lex!("\"a\\\"b\"", STRING_OPEN: 1, STRING_CONTENT: 4, STRING_CLOSE: 1);
    assert_lex!("\"a\\\\\"", STRING_OPEN: 1, STRING_CONTENT: 3, STRING_CLOSE: 1);
    assert_lex!("'a\\'b'", STRING_OPEN: 1, STRING_CONTENT: 4, STRING_CLOSE: 1);
}

#[test]
fn strings_may_span_newlines() {
    assert_lex!("\"foo\nbar\"", STRING_OPEN: 1, STRING_CONTENT: 7, STRING_CLOSE: 1);
}

#[test]
fn a_crlf_inside_a_string_is_content() {
    assert_lex!(
        "\"multiline\r\nstring\"\r\n",
        STRING_OPEN: 1,
        STRING_CONTENT: 17,
        STRING_CLOSE: 1,
        NEWLINE: 2
    );
}

#[test]
fn the_other_quote_is_inert() {
    assert_lex!("\"it's\"", STRING_OPEN: 1, STRING_CONTENT: 4, STRING_CLOSE: 1);
    assert_lex!("'say \"hi\"'", STRING_OPEN: 1, STRING_CONTENT: 8, STRING_CLOSE: 1);
}

#[test]
fn comments_and_operators_are_inert() {
    assert_lex!("\"# %in% `a`\"", STRING_OPEN: 1, STRING_CONTENT: 10, STRING_CLOSE: 1);
}

/// The lexer does not diagnose this. There is no closing delimiter to lex, so
/// the parser reports it when its `expect(STRING_CLOSE)` fails — which keeps the
/// error where the recovery happens.
#[test]
fn unterminated_runs_to_the_end_of_the_file() {
    assert_lex!("\"abc", STRING_OPEN: 1, STRING_CONTENT: 3);
    assert_lex!("\"", STRING_OPEN: 1);
    assert_lex!("'a\nb", STRING_OPEN: 1, STRING_CONTENT: 3);
}

/// A backslash at the very end of the file must not read past it
#[test]
fn a_trailing_escape_does_not_run_past_the_end() {
    assert_lex!("\"a\\", STRING_OPEN: 1, STRING_CONTENT: 2);
}

#[test]
fn in_context() {
    assert_lex!(
        "x <- \"a\"",
        IDENT: 1,
        WHITESPACE: 1,
        ASSIGN: 2,
        WHITESPACE: 1,
        STRING_OPEN: 1,
        STRING_CONTENT: 1,
        STRING_CLOSE: 1,
    );
}
