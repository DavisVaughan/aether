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

    newlines: Newlines,
    else_clause: ElseClause,
}

/// Strategy for interpreting line breaks
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub(crate) enum Newlines {
    /// A line break ends the current expression at top level and inside `{`
    #[default]
    Significant,

    /// Ignore line breaks inside `(`, `[`, or `[[`
    Ignored,
}

/// R permits `else` on a line after the `consequence` only within braces
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
enum ElseClause {
    #[default]
    SameLineOnly,
    MayFollowNewline,
}

impl<'src> RParser<'src> {
    pub(crate) fn new(source: &'src str, options: RParserOptions) -> Self {
        Self {
            context: ParserContext::default(),
            source: RTokenSource::from_str(source),
            options,
            newlines: Newlines::default(),
            else_clause: ElseClause::default(),
        }
    }

    pub(crate) fn options(&self) -> &RParserOptions {
        &self.options
    }

    /// Runs `f` with `newlines` in effect, restoring the previous value after
    pub(crate) fn with_newlines<F, R>(&mut self, newlines: Newlines, f: F) -> R
    where
        F: FnOnce(&mut Self) -> R,
    {
        let saved = std::mem::replace(&mut self.newlines, newlines);
        let result = f(self);
        self.newlines = saved;
        result
    }

    /// Runs `f` inside `{`
    pub(crate) fn with_braces<F, R>(&mut self, f: F) -> R
    where
        F: FnOnce(&mut Self) -> R,
    {
        let saved = (self.newlines, self.else_clause);
        self.newlines = Newlines::Significant;
        self.else_clause = ElseClause::MayFollowNewline;
        let result = f(self);
        (self.newlines, self.else_clause) = saved;
        result
    }

    pub(crate) fn else_clause_may_follow_newline(&self) -> bool {
        self.else_clause == ElseClause::MayFollowNewline
    }

    pub(crate) fn at_statement_break(&self) -> bool {
        self.newlines == Newlines::Significant && self.has_preceding_line_break()
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
