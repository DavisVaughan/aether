//! Constructors for every diagnostic aether emits

use biome_parser::diagnostic::ParseDiagnostic;
use biome_rowan::TextRange;

/// Anything at all, until Phase 7 teaches the parser a grammar
///
/// Delete this along with `parse_placeholder_statement()`
pub(crate) fn not_yet_implemented(range: TextRange) -> ParseDiagnostic {
    ParseDiagnostic::new("The parser does not support R syntax yet.", range)
}

/// A backtick-quoted name that runs to the end of the file
pub(crate) fn unterminated_backtick_identifier(open: TextRange, eof: TextRange) -> ParseDiagnostic {
    ParseDiagnostic::new("Unterminated backtick-quoted name.", open).with_detail(
        eof,
        "Reached the end of the file looking for a closing `` ` ``.",
    )
}

/// A `%...%` operator cut short by a newline or the end of the file
pub(crate) fn unterminated_special(open: TextRange) -> ParseDiagnostic {
    ParseDiagnostic::new("Unterminated `%` operator.", open)
        .with_hint("A `%...%` operator must be closed by a second `%` on the same line.")
}

/// A `0x` with no body, as in `0x` or `0xg`
pub(crate) fn unterminated_hexadecimal(range: TextRange) -> ParseDiagnostic {
    ParseDiagnostic::new("Unterminated hexadecimal literal.", range)
        .with_hint("`0x` must be followed by hexadecimal digits, a `.`, or both.")
}

/// A `p` exponent with no digits, as in `0x1p` or `0x1p+`
pub(crate) fn unterminated_hexadecimal_binary_exponent(range: TextRange) -> ParseDiagnostic {
    ParseDiagnostic::new("Unterminated binary exponent.", range)
        .with_hint("A `p` exponent must be followed by decimal digits.")
}

/// An `e` exponent with no digits, as in `1e` or `1e+`
pub(crate) fn unterminated_exponent(range: TextRange) -> ParseDiagnostic {
    ParseDiagnostic::new("Unterminated exponent.", range)
        .with_hint("An `e` exponent must be followed by decimal digits.")
}

/// A character that starts no known R token
pub(crate) fn unexpected_character(range: TextRange, character: char) -> ParseDiagnostic {
    ParseDiagnostic::new(format!("Unexpected character `{character}`."), range)
}
