use crate::diagnostics;
use crate::parser::RParser;
use crate::syntax::expr::parse_expression;
use aether_r_syntax::RSyntaxKind::{self, R_BOGUS_EXPRESSION, R_STATEMENT, R_STATEMENT_LIST};
use aether_r_syntax::T;
use biome_parser::parsed_syntax::ParsedSyntax::{Absent, Present};
use biome_parser::{Parser, ParserProgress};

/// Expected end of statement list
#[derive(Debug, Clone, Copy)]
pub(crate) enum StatementListEnd {
    Eof,
    #[expect(dead_code, reason = "`parse_braced_statements()` arrives in Phase 13")]
    ClosingBrace,
}

impl StatementListEnd {
    fn matches(self, kind: RSyntaxKind) -> bool {
        match self {
            Self::Eof => kind == T![EOF],
            Self::ClosingBrace => kind == T!['}'],
        }
    }
}

/// Runs to `end` and never stops short of it, which is what lets
/// [crate::syntax::parse_root()] expect an `EOF`.
pub(crate) fn parse_statement_list(p: &mut RParser, end: StatementListEnd) {
    let m = p.start();
    let mut progress = ParserProgress::default();

    while !p.at(T![EOF]) && !end.matches(p.cur()) {
        progress.assert_progressing(p);

        let statement = p.start();

        match parse_expression(p) {
            // R separates statements by a newline or a `;` and by nothing else,
            // so `1 2` is an error even though both halves parse on their own
            Present(expression) => {
                if !p.at(T![EOF]) && !end.matches(p.cur()) && !p.has_preceding_line_break() {
                    p.error(diagnostics::expected_statement_separator(
                        p,
                        expression.range(p),
                        p.cur_range(),
                    ));
                }
            }
            // Failed to find an expression. Consume one token as bogus
            // and retry.
            Absent => {
                p.error(diagnostics::expected_expression(p, p.cur_range()));
                let bogus = p.start();
                p.bump_any();
                bogus.complete(p, R_BOGUS_EXPRESSION);
            }
        }

        statement.complete(p, R_STATEMENT);
    }

    m.complete(p, R_STATEMENT_LIST);
}
