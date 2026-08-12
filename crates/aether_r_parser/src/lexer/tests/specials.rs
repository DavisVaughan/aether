use super::*;

#[test]
fn special_operators() {
    assert_lex!("%%", SPECIAL: 2);
    assert_lex!("%/%", SPECIAL: 3);
    assert_lex!("%in%", SPECIAL: 4);
    assert_lex!("%o%", SPECIAL: 3);
    assert_lex!("%>%", SPECIAL: 3);
}

#[test]
fn in_context() {
    assert_lex!(
        "a %in% b",
        IDENT: 1,
        WHITESPACE: 1,
        SPECIAL: 4,
        WHITESPACE: 1,
        IDENT: 1,
    );
}

/// R's `SpecialValue` ungets the newline before reporting, so the newline
/// survives to terminate the statement and the next line parses normally. Aether
/// stops before it for the same reason.
///
/// `%foo` is still a `SPECIAL` carrying a diagnostic, not an `ERROR_TOKEN`: the
/// text is a well-formed operator that was cut short, so the parser can still
/// use it. `ERROR_TOKEN` is for characters that start nothing at all.
#[test]
fn stops_at_a_newline() {
    assert_lex_err!(
        "%foo\n1",
        SPECIAL: 4, NEWLINE: 1, R_DOUBLE_LITERAL: 1
        => "Unterminated"
    );
}

/// R accepts this — its loop exits on EOF without ever seeing the closing `%`
/// and still returns a `SPECIAL`. tree-sitter-r's regex requires the `%`, so it
/// errors, and aether follows tree-sitter. See `docs/deviations.md`.
#[test]
fn unterminated_at_end_of_file() {
    assert_lex_err!("x %in", IDENT: 1, WHITESPACE: 1, SPECIAL: 3 => "Unterminated");
}

/// R's `SpecialValue` does not exclude backslash, so `%a\b%` is a valid operator
/// in R. tree-sitter-r's character class `[^%\\\n]` excludes it, and their own
/// comment concedes the regex is approximate. Aether follows R. See
/// `docs/deviations.md`.
#[test]
fn a_backslash_is_ordinary_text() {
    assert_lex!(
        "a %a\\b% b",
        IDENT: 1,
        WHITESPACE: 1,
        SPECIAL: 5,
        WHITESPACE: 1,
        IDENT: 1,
    );
}
