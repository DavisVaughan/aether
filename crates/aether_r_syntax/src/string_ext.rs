use crate::RStringValue;
use crate::RSyntaxKind::{
    R_DOUBLE_QUOTED_STRING_LITERAL, R_RAW_STRING_LITERAL, R_SINGLE_QUOTED_STRING_LITERAL,
};
use crate::RSyntaxToken;
use biome_rowan::{SyntaxResult, TextRange, TextSize, TokenText};

impl RStringValue {
    /// The text between the delimiters
    ///
    /// `"foo"` and `r"---(foo)---"` both give `foo`
    pub fn inner_string_text(&self) -> SyntaxResult<TokenText> {
        let token = self.value()?;
        Ok(token.token_text_trimmed().slice(inner_string_range(&token)))
    }
}

/// Relative to the start of the token
///
/// Total because the lexer gives an unterminated string an `ERROR_TOKEN`, so a
/// string literal kind always carries both of its delimiters
fn inner_string_range(token: &RSyntaxToken) -> TextRange {
    let text = token.text_trimmed();

    let (open, close) = match token.kind() {
        R_SINGLE_QUOTED_STRING_LITERAL | R_DOUBLE_QUOTED_STRING_LITERAL => (1, 1),
        // `r`, the quote, the hyphens, and the bracket
        R_RAW_STRING_LITERAL => {
            let hyphens = text.as_bytes()[2..]
                .iter()
                .take_while(|byte| **byte == b'-')
                .count();
            (3 + hyphens, 2 + hyphens)
        }
        kind => unreachable!("`{kind:?}` is not a string"),
    };

    TextRange::new(
        TextSize::from(open as u32),
        TextSize::from((text.len() - close) as u32),
    )
}
