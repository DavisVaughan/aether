use crate::diagnostics;
use crate::parser::{RParser, Scope};
use crate::syntax::expr::parse_expression;
use aether_r_syntax::RSyntaxKind::{self, R_BOGUS_EXPRESSION, R_STATEMENT, R_STATEMENT_LIST};
use aether_r_syntax::T;
use biome_parser::parsed_syntax::ParsedSyntax::{Absent, Present};
use biome_parser::{Parser, ParserProgress};

/// Runs to `close` and never stops short of it
pub(crate) fn parse_statement_list(p: &mut RParser, close: RSyntaxKind) {
    let statement_list = p.start();
    let mut progress = ParserProgress::default();

    while !p.at(T![EOF]) && !p.at(close) {
        progress.assert_progressing(p);

        let statement = p.start();

        match p.cur() {
            T![;] => {
                if p.scope() == Scope::TopLevel {
                    // At top level, a semicolon must be preceded by an
                    // expression that ends on the same line, i.e. `;;` and
                    // `1\n;` aren't legal. Inside braces, semicolons may
                    // appear even without an expression, so `{ ;; }` and
                    // `{ 1\n; }` are fine. In those cases, the statement is
                    // missing its optional expression.
                    p.error(diagnostics::unexpected_semicolon(p.cur_range()));
                }
                p.bump(T![;]);
            }
            _ => {
                match parse_expression(p) {
                    Present(expression) => {
                        if !p.at(T![EOF])
                            && !p.at(close)
                            && !p.at(T![;])
                            && !p.has_preceding_line_break()
                        {
                            p.error(diagnostics::expected_statement_separator(
                                expression.range(p),
                                p.cur_range(),
                            ));
                        }
                    }
                    Absent => {
                        // Consume everything until next statement separator as
                        // bogus, and always at least one token so that the
                        // statement list keeps progressing
                        let bogus = p.start();
                        p.bump_any();
                        while !p.at(T![EOF])
                            && !p.at(close)
                            && !p.at(T![;])
                            && !p.has_preceding_line_break()
                        {
                            p.bump_any();
                        }
                        let bogus = bogus.complete(p, R_BOGUS_EXPRESSION);
                        p.error(diagnostics::expected_expression(p, bogus.range(p)));
                    }
                }

                if !p.has_preceding_line_break() {
                    p.eat(T![;]);
                }
            }
        }

        statement.complete(p, R_STATEMENT);
    }

    statement_list.complete(p, R_STATEMENT_LIST);
}
