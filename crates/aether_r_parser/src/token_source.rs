#[cfg(test)]
mod tests;

use crate::lexer::{RLexContext, RLexer};
use aether_r_syntax::RSyntaxKind::{self, EOF};
use biome_parser::diagnostic::ParseDiagnostic;
use biome_parser::lexer::BufferedLexer;
use biome_parser::prelude::{BumpWithContext, TokenSource, Trivia};
use biome_parser::token_source::TokenSourceWithBufferedLexer;
use biome_rowan::{TextRange, TriviaPieceKind};

pub(crate) struct RTokenSource<'src> {
    lexer: BufferedLexer<RSyntaxKind, RLexer<'src>>,

    /// The trivia the CST is rebuilt from, in source order
    trivia: Vec<Trivia>,
}

impl<'src> RTokenSource<'src> {
    pub(crate) fn from_str(source: &'src str) -> Self {
        let lexer = BufferedLexer::new(RLexer::from_str(source));

        let mut source = Self {
            lexer,
            trivia: Vec::new(),
        };

        source.next_non_trivia_token(RLexContext::default(), true);
        source
    }

    /// Advances to the next non-trivia token, collecting the trivia it skips
    ///
    /// Trivia trails the token before it until a newline is seen, and leads the
    /// token after it from the newline onward. Before the first token of the
    /// file, everything leads. That is every Biome parser's rule, and it needs
    /// no lookahead. Air's rule is different; deviation 10 says why aether does
    /// not follow it.
    fn next_non_trivia_token(&mut self, context: RLexContext, first: bool) {
        let mut trailing = !first;

        loop {
            let kind = self.lexer.next_token(context);

            let Ok(trivia_kind) = TriviaPieceKind::try_from(kind) else {
                break;
            };

            // Before the push, so the newline itself leads the next token
            if trivia_kind.is_newline() {
                trailing = false;
            }

            self.trivia
                .push(Trivia::new(trivia_kind, self.current_range(), trailing));
        }
    }
}

impl TokenSource for RTokenSource<'_> {
    type Kind = RSyntaxKind;

    fn current(&self) -> Self::Kind {
        self.lexer.current()
    }

    fn current_range(&self) -> TextRange {
        self.lexer.current_range()
    }

    fn text(&self) -> &str {
        self.lexer.source()
    }

    /// Delegated, and deliberately not mirrored in a field here
    ///
    /// Lookahead drives the inner lexer past the current token.
    /// [`BufferedLexer`] holds a checkpoint of the token the parser is actually
    /// on and answers from that one while lookahead is live, which is what makes
    /// plain delegation correct under
    /// [`NthToken::nth`](biome_parser::prelude::NthToken::nth).
    fn has_preceding_line_break(&self) -> bool {
        self.lexer.has_preceding_line_break()
    }

    fn bump(&mut self) {
        self.bump_with_context(RLexContext::Regular);
    }

    fn skip_as_trivia(&mut self) {
        self.skip_as_trivia_with_context(RLexContext::Regular);
    }

    fn finish(self) -> (Vec<Trivia>, Vec<ParseDiagnostic>) {
        (self.trivia, self.lexer.finish())
    }
}

impl BumpWithContext for RTokenSource<'_> {
    type Context = RLexContext;

    fn bump_with_context(&mut self, context: Self::Context) {
        if self.current() != EOF {
            self.next_non_trivia_token(context, false);
        }
    }

    fn skip_as_trivia_with_context(&mut self, context: Self::Context) {
        if self.current() != EOF {
            self.trivia.push(Trivia::new(
                TriviaPieceKind::Skipped,
                self.current_range(),
                false,
            ));

            self.next_non_trivia_token(context, true);
        }
    }
}

impl<'src> TokenSourceWithBufferedLexer<RLexer<'src>> for RTokenSource<'src> {
    fn lexer(&mut self) -> &mut BufferedLexer<RSyntaxKind, RLexer<'src>> {
        &mut self.lexer
    }
}
