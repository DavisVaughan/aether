//! Constructors for every diagnostic aether emits

use crate::parser::RParser;
use aether_r_syntax::RSyntaxKind;
use biome_parser::diagnostic::{ParseDiagnostic, expected_any, expected_node};
use biome_rowan::TextRange;

/// What `AnyRSelector` accepts
const SELECTORS: &[&str] = &["identifier", "string", "`...`", "`..i`"];

/// A backtick-quoted name that runs to the end of the file
pub(crate) fn unterminated_backtick_identifier(open: TextRange, eof: TextRange) -> ParseDiagnostic {
    ParseDiagnostic::new("Unterminated backtick-quoted name.", open)
        .with_detail(eof, "Reached the end of the file looking for a closing `.")
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

/// A string that runs to the end of the file
pub(crate) fn unterminated_string(open: TextRange, eof: TextRange) -> ParseDiagnostic {
    ParseDiagnostic::new("Unterminated string.", open).with_detail(
        eof,
        "Reached the end of the file looking for a closing delimiter.",
    )
}

pub(crate) fn expected_expression(p: &RParser, range: TextRange) -> ParseDiagnostic {
    expected_node("expression", range, p)
}

/// The name on the right of `$`, `@`, `::`, or `:::` is missing
pub(crate) fn expected_selector(
    p: &RParser,
    operator: RSyntaxKind,
    operator_range: TextRange,
    range: TextRange,
) -> ParseDiagnostic {
    expected_any(SELECTORS, range, p).with_detail(
        operator_range,
        format!("`{}` requires a name on the right.", text_of(operator)),
    )
}

/// The name on the left of `::` or `:::` is not a name
pub(crate) fn expected_selector_on_the_left(
    p: &RParser,
    operator: RSyntaxKind,
    operator_range: TextRange,
    range: TextRange,
) -> ParseDiagnostic {
    expected_any(SELECTORS, range, p).with_detail(
        operator_range,
        format!("`{}` requires a name on the left.", text_of(operator)),
    )
}

/// A newline between `::` or `:::` and the name it needs
pub(crate) fn selector_after_line_break(
    operator: RSyntaxKind,
    operator_range: TextRange,
) -> ParseDiagnostic {
    let operator = text_of(operator);

    ParseDiagnostic::new(
        format!("A newline is not allowed between `{operator}` and the name that follows."),
        operator_range,
    )
    .with_hint(format!(
        "`{operator}` and its name must be on the same line."
    ))
}

fn text_of(operator: RSyntaxKind) -> &'static str {
    operator.to_string().unwrap_or("operator")
}

/// Two expressions with nothing between them, as in `1 2`
pub(crate) fn expected_statement_separator(
    previous: TextRange,
    range: TextRange,
) -> ParseDiagnostic {
    ParseDiagnostic::new(
        "Expected a newline or a `;` between two expressions.",
        range,
    )
    .with_detail(previous, "The first expression ends here.")
}
