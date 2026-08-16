#![allow(
    dead_code,
    reason = "the parse context helpers are driven by the syntax modules from Phase 7 on"
)]

use crate::options::RParserOptions;
use crate::token_source::RTokenSource;
use aether_r_syntax::RSyntaxKind;
use biome_parser::ParserContext;
use biome_parser::diagnostic::{ParseDiagnostic, merge_diagnostics};
use biome_parser::event::Event;
use biome_parser::prelude::*;

pub(crate) struct RParser<'src> {
    context: ParserContext<RSyntaxKind>,
    source: RTokenSource<'src>,
    options: RParserOptions,
    scope: Scope,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub(crate) enum Scope {
    #[default]
    TopLevel,
    Brace,
    Parenthesis,
    Bracket,
    Bracket2,
}

impl Scope {
    /// A line break ends a statement at top level and inside `{`, and means
    /// nothing inside `(`, `[`, or `[[`
    pub(crate) fn newlines_are_significant(self) -> bool {
        match self {
            Self::TopLevel | Self::Brace => true,
            Self::Parenthesis | Self::Bracket | Self::Bracket2 => false,
        }
    }
}

impl<'src> RParser<'src> {
    pub(crate) fn new(source: &'src str, options: RParserOptions) -> Self {
        Self {
            context: ParserContext::default(),
            source: RTokenSource::from_str(source),
            options,
            scope: Scope::default(),
        }
    }

    pub(crate) fn options(&self) -> &RParserOptions {
        &self.options
    }

    /// Runs `f` with `scope` in effect, restoring the previous value after
    pub(crate) fn with_scope<F, R>(&mut self, scope: Scope, f: F) -> R
    where
        F: FnOnce(&mut Self) -> R,
    {
        let saved = std::mem::replace(&mut self.scope, scope);
        let result = f(self);
        self.scope = saved;
        result
    }

    /// Ask about line breaks through [`Scope::newlines_are_significant()`]
    pub(crate) fn scope(&self) -> Scope {
        self.scope
    }

    pub(crate) fn finish(self) -> (Vec<Event<RSyntaxKind>>, Vec<Trivia>, Vec<ParseDiagnostic>) {
        let (trivia, lexer_diagnostics) = self.source.finish();
        let (events, parse_diagnostics) = self.context.finish();
        let diagnostics = merge_diagnostics(lexer_diagnostics, parse_diagnostics);
        (events, trivia, diagnostics)
    }
}

impl<'src> Parser for RParser<'src> {
    type Kind = RSyntaxKind;
    type Source = RTokenSource<'src>;

    fn context(&self) -> &ParserContext<Self::Kind> {
        &self.context
    }

    fn context_mut(&mut self) -> &mut ParserContext<Self::Kind> {
        &mut self.context
    }

    fn source(&self) -> &Self::Source {
        &self.source
    }

    fn source_mut(&mut self) -> &mut Self::Source {
        &mut self.source
    }
}
