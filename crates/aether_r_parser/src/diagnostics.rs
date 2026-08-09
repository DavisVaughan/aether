//! Constructors for every diagnostic aether emits

use biome_parser::diagnostic::ParseDiagnostic;
use biome_rowan::TextRange;

/// A backtick-quoted name that runs to the end of the file
pub(crate) fn unterminated_backtick_identifier(open: TextRange, eof: TextRange) -> ParseDiagnostic {
    ParseDiagnostic::new("Unterminated backtick-quoted name.", open).with_detail(
        eof,
        "Reached the end of the file looking for a closing `` ` ``.",
    )
}

/// A `%…%` operator cut short by a newline, a backslash, or the end of the file
///
/// R's `SpecialValue` ungets the newline before reporting, so the error stays
/// local and the next line still parses; aether stops before the newline for the
/// same reason.
pub(crate) fn unterminated_special(open: TextRange) -> ParseDiagnostic {
    ParseDiagnostic::new("Unterminated `%` operator.", open)
        .with_hint("A `%...%` operator must be closed by a second `%` on the same line.")
}

/// A character that starts no known R token
pub(crate) fn unexpected_character(range: TextRange, character: char) -> ParseDiagnostic {
    ParseDiagnostic::new(format!("Unexpected character `{character}`."), range)
}
