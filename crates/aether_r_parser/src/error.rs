use biome_diagnostics::PrintDescription;
use biome_parser::prelude::ParseDiagnostic;

/// An error that occurs during parsing
///
/// Replacement for [ParseDiagnostic], mainly so we can implement
/// [std::error::Error], which it oddly does not implement.
#[derive(Debug, Clone)]
pub struct ParseError {
    message: String,
}

impl std::error::Error for ParseError {}

impl std::fmt::Display for ParseError {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        f.write_str(&self.message)
    }
}

impl ParseError {
    /// Renders a diagnostic's description, dropping its labels and hint
    ///
    /// Lossy on purpose: [ParseError] is the narrow back-compatible channel,
    /// and [`Parse::diagnostics()`](crate::Parse::diagnostics) is the one that
    /// keeps everything.
    pub(crate) fn from_diagnostic(diagnostic: &ParseDiagnostic) -> Self {
        Self {
            message: PrintDescription(diagnostic).to_string(),
        }
    }
}
