use crate::diagnostics;
use crate::parser::{RParser, Scope};
use crate::syntax::expr::parse_expression;
use aether_r_syntax::RSyntaxKind::{
    self, R_BOGUS_EXPRESSION, R_BRACED_STATEMENTS, R_ROOT, R_STATEMENT, R_STATEMENT_LIST,
};
use aether_r_syntax::T;
use biome_parser::CompletedMarker;
use biome_parser::parsed_syntax::ParsedSyntax::{Absent, Present};
use biome_parser::{Parser, ParserProgress};

pub(crate) fn parse_root(p: &mut RParser) {
    let m = p.start();
    p.eat(T![UNICODE_BOM]);
    parse_statement_list(p, T![EOF]);
    p.expect(T![EOF]);
    m.complete(p, R_ROOT);
}

pub(crate) fn parse_braced_statements(p: &mut RParser) -> CompletedMarker {
    let m = p.start();
    p.with_scope(Scope::Brace, |p| {
        p.bump(T!['{']);
        parse_statement_list(p, T!['}']);
    });
    p.expect(T!['}']);
    m.complete(p, R_BRACED_STATEMENTS)
}

/// Runs to `close` and never stops short of it
fn parse_statement_list(p: &mut RParser, close: RSyntaxKind) {
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
            _ => match parse_expression(p) {
                Present(expression) => parse_statement(p, expression, close),
                Absent => parse_recovered_statement(p, close),
            },
        }

        statement.complete(p, R_STATEMENT);
    }

    statement_list.complete(p, R_STATEMENT_LIST);
}

fn parse_statement(p: &mut RParser, expression: CompletedMarker, close: RSyntaxKind) {
    let line_break = p.has_preceding_line_break();
    let semicolon = !line_break && p.eat(T![;]);
    let terminated = semicolon || line_break || p.at(T![EOF]) || p.at(close);

    if !terminated {
        p.error(diagnostics::expected_statement_separator(
            expression.range(p),
            p.cur_range(),
        ));
    }
}

/// Failed to find an expression. Bury the rest of the line as bogus and retry.
fn parse_recovered_statement(p: &mut RParser, close: RSyntaxKind) {
    let range = p.cur_range();

    let bogus = p.start();
    p.bump_any();
    while !p.at(T![EOF]) && !p.at(close) && !p.at(T![;]) && !p.has_preceding_line_break() {
        p.bump_any();
    }
    bogus.complete(p, R_BOGUS_EXPRESSION);
    p.error(diagnostics::expected_expression(p, range));

    if !p.has_preceding_line_break() {
        p.eat(T![;]);
    }
}
