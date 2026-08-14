//! Constructors for every diagnostic aether emits

use crate::parser::RParser;
use aether_r_syntax::RSyntaxKind;
use biome_parser::Parser;
use biome_parser::diagnostic::{ParseDiagnostic, expected_any, expected_node};
use biome_rowan::TextRange;

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

/// An expression is required and absent
#[allow(dead_code, reason = "driven by the syntax modules in Phase 7")]
pub(crate) fn expected_expression(p: &RParser, range: TextRange) -> ParseDiagnostic {
    expected_node("expression", range, p)
}

/// The name on the right of `$`, `@`, `::`, or `:::` is missing
///
/// All four funnel through here, so the diagnostic names the one that was
/// actually written and points a secondary label at it. `operator_range` has to
/// be captured before the operator is bumped.
#[allow(dead_code, reason = "driven by the syntax modules in Phase 10")]
pub(crate) fn expected_selector(
    p: &RParser,
    operator: RSyntaxKind,
    operator_range: TextRange,
    range: TextRange,
) -> ParseDiagnostic {
    // Punctuation kinds render as their own text, so this stays correct if the
    // token set changes
    let operator = operator.to_string().unwrap_or("operator");

    expected_any(&["identifier", "string", "`...`", "`..i`"], range, p).with_detail(
        operator_range,
        format!("`{operator}` requires a name on the right."),
    )
}

/// The whole `(…)` group of a function definition is missing, as in `function 1`
#[allow(dead_code, reason = "driven by the syntax modules in Phase 11")]
pub(crate) fn expected_parameters(p: &RParser, range: TextRange) -> ParseDiagnostic {
    expected_node("parameter list", range, p)
}

/// A hole in a parameter list, as in `function(a,,b)`
///
/// R rejects this even though the same shape is legal in an argument list.
#[allow(dead_code, reason = "driven by the syntax modules in Phase 11")]
pub(crate) fn expected_parameter(p: &RParser, range: TextRange) -> ParseDiagnostic {
    expected_node("parameter", range, p)
}

/// An argument is required and absent
#[allow(dead_code, reason = "driven by the syntax modules in Phase 9")]
pub(crate) fn expected_argument(p: &RParser, range: TextRange) -> ParseDiagnostic {
    expected_node("argument", range, p)
}

/// Two expressions with nothing between them, as in `1 2`
///
/// R separates statements by a newline or a `;` and accepts nothing else.
/// `previous` labels the expression that already ended. The argument list
/// analogue (`f(a b)`) is a different rule, enforced by `expect(T![,])`.
#[allow(dead_code, reason = "driven by the syntax modules in Phase 13")]
pub(crate) fn expected_statement_separator(
    p: &RParser,
    previous: TextRange,
    range: TextRange,
) -> ParseDiagnostic {
    p.err_builder(
        "Expected a newline or a `;` between two expressions.",
        range,
    )
    .with_detail(previous, "This expression ends here.")
}

/// The condition of an `if` or `while` is missing
#[allow(dead_code, reason = "driven by the syntax modules in Phase 12")]
pub(crate) fn expected_condition(p: &RParser, range: TextRange) -> ParseDiagnostic {
    expected_node("condition", range, p)
}
