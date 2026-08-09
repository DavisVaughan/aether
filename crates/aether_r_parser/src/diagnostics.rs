//! Constructors for every diagnostic aether emits.
//!
//! Every diagnostic is a named function here rather than an inline
//! `ParseDiagnostic::new` at the site that raises it. That keeps the wording
//! consistent and makes the whole diagnostic surface reviewable in one file.

use biome_parser::diagnostic::ParseDiagnostic;
use biome_rowan::TextRange;

/// A backtick-quoted name that runs to the end of the file.
///
/// Backticked names are identifiers rather than strings, and R lets them span
/// newlines, so this is worded apart from the unclosed-quote diagnostics and
/// there is no line to resynchronise on. What keeps it useful is `open`: the
/// opening delimiter is the actionable location, and highlighting the rest of
/// the file instead would tell the reader nothing.
pub(crate) fn unterminated_backtick_identifier(open: TextRange, eof: TextRange) -> ParseDiagnostic {
    ParseDiagnostic::new("Unterminated backtick-quoted name.", open).with_detail(
        eof,
        "Reached the end of the file looking for a closing `` ` ``.",
    )
}

/// A `%…%` operator cut short by a newline, a backslash, or the end of the file.
///
/// R's `SpecialValue` ungets the newline before reporting, so the error stays
/// local and the next line still parses; aether stops before the newline for the
/// same reason. Backslashes are excluded because tree-sitter-r's character class
/// excludes them — see `docs/deviations.md`.
pub(crate) fn unterminated_special(open: TextRange) -> ParseDiagnostic {
    ParseDiagnostic::new("Unterminated `%` operator.", open)
        .with_hint("A `%...%` operator must be closed by a second `%` on the same line.")
}

/// A character that starts no R token at all.
pub(crate) fn unexpected_character(range: TextRange, character: char) -> ParseDiagnostic {
    ParseDiagnostic::new(format!("Unexpected character `{character}`."), range)
}
