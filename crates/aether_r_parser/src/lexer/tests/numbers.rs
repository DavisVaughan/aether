use super::*;

#[test]
fn decimals() {
    assert_lex!("1", R_DOUBLE_LITERAL: 1);
    assert_lex!("123", R_DOUBLE_LITERAL: 3);
    assert_lex!("1.", R_DOUBLE_LITERAL: 2);
    assert_lex!("1.5", R_DOUBLE_LITERAL: 3);
    assert_lex!(".5", R_DOUBLE_LITERAL: 2);
}

#[test]
fn suffixes() {
    assert_lex!("1L", R_INTEGER_LITERAL: 2);
    assert_lex!("1i", R_COMPLEX_LITERAL: 2);
    assert_lex!("1.5L", R_INTEGER_LITERAL: 4);
    assert_lex!("1e5L", R_INTEGER_LITERAL: 4);
    assert_lex!("1L(", R_INTEGER_LITERAL: 2, L_PAREN: 1);
}

/// tree-sitter-r spells `L` and `i` as tokens of their own rather than as part
/// of the literal's regex, so an identifier that merely starts with one wins on
/// length. Taking the suffix unconditionally would make `1Li` an integer
/// followed by the name `i`, which is not what any of R, tree-sitter-r, or the
/// person who wrote it means.
#[test]
fn a_suffix_that_starts_a_name_is_a_name() {
    assert_lex!("1Li", R_DOUBLE_LITERAL: 1, IDENT: 2);
    assert_lex!("1LL", R_DOUBLE_LITERAL: 1, IDENT: 2);
    assert_lex!("1iL", R_DOUBLE_LITERAL: 1, IDENT: 2);
    assert_lex!("1L_", R_DOUBLE_LITERAL: 1, IDENT: 2);
    // `.` continues an identifier too
    assert_lex!("1L.", R_DOUBLE_LITERAL: 1, IDENT: 2);
    assert_lex!(".1L.", R_DOUBLE_LITERAL: 2, IDENT: 2);
}

/// A decimal exponent's digits are optional in tree-sitter-r's regex
/// (`[eE][+-]?\d*`), so `1e` and `1e+` are whole literals rather than a number
/// followed by a name. That is deliberate upstream, not a bug here.
#[test]
fn exponents_may_be_empty() {
    assert_lex!("1e", R_DOUBLE_LITERAL: 2);
    assert_lex!("1e+", R_DOUBLE_LITERAL: 3);
    assert_lex!("1e-3", R_DOUBLE_LITERAL: 4);
    assert_lex!("1e5", R_DOUBLE_LITERAL: 3);
    assert_lex!("1.e5", R_DOUBLE_LITERAL: 4);
    // Which is also why these are a literal and a name, not one name and not one
    // literal with a subtraction in it
    assert_lex!("1east", R_DOUBLE_LITERAL: 2, IDENT: 3);
    assert_lex!("1e-a", R_DOUBLE_LITERAL: 3, IDENT: 1);
}

#[test]
fn hexadecimals() {
    assert_lex!("0xFF", R_DOUBLE_LITERAL: 4);
    assert_lex!("0XFF", R_DOUBLE_LITERAL: 4);
    assert_lex!("0xFFL", R_INTEGER_LITERAL: 5);
    assert_lex!("0x1.8", R_DOUBLE_LITERAL: 5);
    // R accepts a hexadecimal with nothing after the dot, surprisingly
    assert_lex!("0x.", R_DOUBLE_LITERAL: 3);
    assert_lex!("0x.8", R_DOUBLE_LITERAL: 4);
    // `e` is a hexadecimal digit, not the start of a decimal exponent
    assert_lex!("0x1e", R_DOUBLE_LITERAL: 4);
}

/// A binary exponent's digits are *not* optional, unlike a decimal one's, so a
/// bare `p` falls out of the literal and becomes a name
#[test]
fn binary_exponents() {
    assert_lex!("0x1p3", R_DOUBLE_LITERAL: 5);
    assert_lex!("0x.8p1", R_DOUBLE_LITERAL: 6);
    assert_lex!("0x1p+3", R_DOUBLE_LITERAL: 6);
    assert_lex!("0x.p3", R_DOUBLE_LITERAL: 5);
    assert_lex!("0x1.8p3", R_DOUBLE_LITERAL: 7);
    assert_lex!("0x1p", R_DOUBLE_LITERAL: 3, IDENT: 1);
    assert_lex!("0x.p", R_DOUBLE_LITERAL: 3, IDENT: 1);
    assert_lex!("0x.8p", R_DOUBLE_LITERAL: 4, IDENT: 1);
    // A sign with no digits after it is no exponent either
    assert_lex!("0x1p+", R_DOUBLE_LITERAL: 3, IDENT: 1, PLUS: 1);
}

/// `0x` with no body is not a hexadecimal at all: the `0` is a literal and the
/// `x` starts a name
#[test]
fn a_hexadecimal_prefix_with_no_body() {
    assert_lex!("0x", R_DOUBLE_LITERAL: 1, IDENT: 1);
    assert_lex!("0xg", R_DOUBLE_LITERAL: 1, IDENT: 2);
    // The body may not start with the binary exponent either
    assert_lex!("0xp3", R_DOUBLE_LITERAL: 1, IDENT: 3);
}

#[test]
fn a_second_dot_starts_a_second_literal() {
    assert_lex!("1.5.3", R_DOUBLE_LITERAL: 3, R_DOUBLE_LITERAL: 2);
}
