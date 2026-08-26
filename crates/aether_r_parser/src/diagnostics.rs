//! Constructors for every diagnostic aether emits

use crate::parser::MAX_NESTING_DEPTH;
use crate::parser::RParser;
use aether_r_syntax::RSyntaxKind;
use biome_parser::Parser;
use biome_parser::diagnostic::ParseDiagnostic;
use biome_parser::token_source::TokenSource;
use biome_rowan::TextLen;
use biome_rowan::TextRange;

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

/// Expressions nested past [`crate::parser::MAX_NESTING_DEPTH`]
pub(crate) fn nested_too_deeply(range: TextRange) -> ParseDiagnostic {
    ParseDiagnostic::new("This expression is nested too deeply.", range).with_hint(format!(
        "Expressions may not nest more than {MAX_NESTING_DEPTH} levels deep."
    ))
}

/// The name on the right of `$`, `@`, `::`, or `:::` is missing
pub(crate) fn expected_rhs_selector(
    operator: RSyntaxKind,
    operator_range: TextRange,
    range: TextRange,
) -> ParseDiagnostic {
    ParseDiagnostic::new("Expected a name.", range).with_detail(
        operator_range,
        format!("`{}` requires a name on the right.", text_of(operator)),
    )
}

/// The name on the left of `::` or `:::` is not a name
pub(crate) fn expected_lhs_selector(
    operator: RSyntaxKind,
    operator_range: TextRange,
    range: TextRange,
) -> ParseDiagnostic {
    ParseDiagnostic::new("Expected a name.", range).with_detail(
        operator_range,
        format!("`{}` requires a name on the left.", text_of(operator)),
    )
}

/// A newline between `::` or `:::` and the name it needs
pub(crate) fn namespace_rhs_selector_after_line_break(
    operator: RSyntaxKind,
    operator_range: TextRange,
) -> ParseDiagnostic {
    let operator = text_of(operator);

    ParseDiagnostic::new(
        format!("A newline is not allowed between `{operator}` and the name that follows."),
        operator_range,
    )
}

/// The whole `(...)` group of a function definition is missing, as in `function 1`
pub(crate) fn expected_parameters(p: &RParser, range: TextRange) -> ParseDiagnostic {
    expected_node("parameter list", range, p)
}

/// A parameter with no name, as in `function(a,,b)` or `function(1)`
pub(crate) fn expected_parameter_name(p: &RParser, range: TextRange) -> ParseDiagnostic {
    expected_node("parameter name", range, p)
}

/// The condition of an `if` or `while` is missing
pub(crate) fn expected_condition(p: &RParser, range: TextRange) -> ParseDiagnostic {
    expected_node("condition", range, p)
}

/// The variable of a `for` loop is missing or is not a name, as in `for (1 in x) y`
pub(crate) fn expected_for_variable(p: &RParser, range: TextRange) -> ParseDiagnostic {
    expected_node("loop variable", range, p)
}

/// The sequence a `for` loop iterates over is missing
pub(crate) fn expected_for_sequence(p: &RParser, range: TextRange) -> ParseDiagnostic {
    expected_node("loop sequence", range, p)
}

/// The body of a `for`, `while`, or `repeat` loop is missing
pub(crate) fn expected_loop_body(p: &RParser, range: TextRange) -> ParseDiagnostic {
    expected_node("loop body", range, p)
}

/// The body of a function definition is missing, as in `function(x)`
pub(crate) fn expected_function_body(p: &RParser, range: TextRange) -> ParseDiagnostic {
    expected_node("function body", range, p)
}

pub(crate) fn expected_if_body(p: &RParser, range: TextRange) -> ParseDiagnostic {
    expected_node("if body", range, p)
}

pub(crate) fn expected_else_body(p: &RParser, range: TextRange) -> ParseDiagnostic {
    expected_node("else body", range, p)
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

/// A `;` with no expression before it on the same line, as in `;1` or `1\n;`
pub(crate) fn unexpected_semicolon(range: TextRange) -> ParseDiagnostic {
    ParseDiagnostic::new("Unexpected `;`.", range)
        .with_hint("At top level, a `;` must directly follow an expression.")
}

/// Like [biome_parser::diagnostic::expected_node()], but without the duplicative hint
fn expected_node(name: &str, range: TextRange, p: &RParser) -> ParseDiagnostic {
    let article = article_for(name);

    let message = if p.source().text().text_len() <= range.start() {
        format!("Expected {article} {name} but instead found the end of the file.")
    } else {
        format!(
            "Expected {article} {name} but instead found '{}'.",
            p.text(range)
        )
    };

    ParseDiagnostic::new(message, range)
}

fn article_for(name: &str) -> &'static str {
    match name.bytes().next() {
        Some(b'a' | b'e' | b'i' | b'o' | b'u') => "an",
        _ => "a",
    }
}

fn text_of(operator: RSyntaxKind) -> &'static str {
    operator.to_string().unwrap_or("operator")
}
