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

        match p.cur() {
            T![;] => parse_empty_statement(p),
            _ => match parse_expression(p) {
                Present(expression) => parse_statement(p, expression, close),
                Absent => parse_recovered_statement(p, close),
            },
        }
    }

    statement_list.complete(p, R_STATEMENT_LIST);
}

/// R's `prog` reaches a `;` only through `expr ';'`, where its `exprlist` also
/// has `exprlist ';'` over a possibly empty list
fn parse_empty_statement(p: &mut RParser) {
    let m = p.start();

    if p.scope() == Scope::TopLevel {
        p.error(diagnostics::unexpected_semicolon(p.cur_range()));
    }
    p.bump(T![;]);

    m.complete(p, R_STATEMENT);
}

fn parse_statement(p: &mut RParser, expression: CompletedMarker, close: RSyntaxKind) {
    let range = expression.range(p);
    let m = expression.precede(p);

    let line_break = p.has_preceding_line_break();
    let semicolon = !line_break && p.eat(T![;]);
    let terminated = semicolon || line_break || p.at(T![EOF]) || p.at(close);

    if !terminated {
        p.error(diagnostics::expected_statement_separator(
            range,
            p.cur_range(),
        ));
    }

    m.complete(p, R_STATEMENT);
}

/// Failed to find an expression. Bury the rest of the line as bogus and retry.
fn parse_recovered_statement(p: &mut RParser, close: RSyntaxKind) {
    let m = p.start();
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

    m.complete(p, R_STATEMENT);
}
