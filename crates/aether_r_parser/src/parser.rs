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

/// What a line break means at the current position
///
/// The polarity of [`Newlines::default()`] is deliberate. A forgotten
/// [`RParser::with_newlines`] leaves [`Newlines::Significant`] in place, which
/// breaks a statement where none should break — a loud parse error at the first
/// multi-line call in the corpus. The inverse default fails silently instead:
/// `1\n+2` would quietly become one expression rather than two statements, a
/// wrong tree with no diagnostic that round-trips perfectly.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub(crate) enum Newlines {
    /// Top level and inside `{`. A line break ends the current expression.
    #[default]
    Significant,

    /// Inside `(`, `[`, or `[[`, where newlines carry no meaning — R's parser
    /// eats them, and so does tree-sitter-r's external scanner
    /// (`consume_whitespace_and_ignored_newlines`).
    Ignored,
}

/// Whether an `else` may follow a line break
///
/// R permits `else` on a line after the consequence only within braces; at top
/// level the newline ends the `if` and the `else` is a syntax error. Named for
/// what it licenses rather than for the `{` that sets it, because it has exactly
/// one reader.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub(crate) enum ElseClause {
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
    ///
    /// One caller per construct that eats newlines: `f(…)`, `x[…]`, `x[[…]]`,
    /// `(…)`, the `if` and `while` conditions, the `for` header, and a parameter
    /// list.
    pub(crate) fn with_newlines<F, R>(&mut self, newlines: Newlines, f: F) -> R
    where
        F: FnOnce(&mut Self) -> R,
    {
        let saved = std::mem::replace(&mut self.newlines, newlines);
        let result = f(self);
        self.newlines = saved;
        result
    }

    /// Runs `f` inside `{ … }`, restoring both fields after
    ///
    /// Braces are the one construct that changes both, and they change them in
    /// opposite directions: a line break inside braces is significant again,
    /// even when the braces sit inside a paren (`f({\n  a\n  b\n})`), and an
    /// `else` may follow one. Owning the pair here rather than at the call site
    /// is what stops the two from being restated, or set inconsistently, by
    /// whatever else learns to parse a block.
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

    /// True when a newline before an `else` is tolerated because we are inside
    /// braces
    ///
    /// The only reader is the `else` clause; everything else asks
    /// [`RParser::at_statement_break`]. An accessor rather than a public field,
    /// so that "exactly one reader" is enforced by privacy rather than by hope.
    pub(crate) fn allows_newline_before_else(&self) -> bool {
        self.else_clause == ElseClause::MayFollowNewline
    }

    /// True when a newline before the current token should end the current
    /// expression. False inside `(`, `[`, and `[[`.
    ///
    /// The single predicate for newline significance. Both newline rules in the
    /// language — statement termination and `else` attachment — funnel through
    /// it; nothing else should read `newlines` directly.
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
