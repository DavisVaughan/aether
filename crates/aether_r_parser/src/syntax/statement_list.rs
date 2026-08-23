use crate::diagnostics;
use crate::parser::{RParser, Scope};
use crate::syntax::expr::parse_expression;
use aether_r_syntax::RSyntaxKind::{
    self, R_BOGUS_EXPRESSION, R_BRACED_STATEMENTS, R_STATEMENT, R_STATEMENT_LIST,
};
use aether_r_syntax::T;
use biome_parser::CompletedMarker;
use biome_parser::{Parser, ParserProgress};

pub(crate) fn parse_braced_statements(p: &mut RParser) -> CompletedMarker {
    debug_assert!(p.at(T!['{']));

    let m = p.start();
    p.with_scope(Scope::Brace, |p| {
        p.bump(T!['{']);
        parse_statement_list(p, T!['}']);
    });
    p.expect(T!['}']);

    m.complete(p, R_BRACED_STATEMENTS)
}

/// Runs to `close` and never stops short of it
pub(crate) fn parse_statement_list(p: &mut RParser, close: RSyntaxKind) {
    let statement_list = p.start();
    let mut progress = ParserProgress::default();

    while !p.at(T![EOF]) && !p.at(close) {
        progress.assert_progressing(p);

        let statement = p.start();

        if p.at(T![;]) {
            // R's `prog` reaches a `;` only through `expr ';'`, where its
            // `exprlist` also has `exprlist ';'` over a possibly empty list.
            if p.scope() == Scope::TopLevel {
                p.error(diagnostics::unexpected_semicolon(p.cur_range()));
            }
            p.bump(T![;]);
            statement.complete(p, R_STATEMENT);
            continue;
        }

        let expression = parse_expression(p).ok();

        if expression.is_none() {
            // Failed to find an expression. Bury the rest of the line as bogus
            // and retry.
            let range = p.cur_range();
            let bogus = p.start();
            p.bump_any();
            while !p.at(T![EOF]) && !p.at(close) && !p.at(T![;]) && !p.has_preceding_line_break() {
                p.bump_any();
            }
            bogus.complete(p, R_BOGUS_EXPRESSION);
            p.error(diagnostics::expected_expression(p, range));
        }

        let semicolon = !p.has_preceding_line_break() && p.eat(T![;]);

        // We've just accepted an expression. Now check for a statement
        // separator. If it looks like the next expression starts
        // immediately, log that as a diagnostic, i.e. `1 2`.
        if let Some(expression) = expression
            && !semicolon
            && !p.at(T![EOF])
            && !p.at(close)
            && !p.has_preceding_line_break()
        {
            p.error(diagnostics::expected_statement_separator(
                expression.range(p),
                p.cur_range(),
            ));
        }

        statement.complete(p, R_STATEMENT);
    }

    statement_list.complete(p, R_STATEMENT_LIST);
}
