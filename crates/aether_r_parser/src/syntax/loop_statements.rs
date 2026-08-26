use crate::diagnostics;
use crate::parser::{RParser, Scope};
use crate::syntax::body::parse_body_expression;
use crate::syntax::expr::parse_expression;
use aether_r_syntax::RSyntaxKind::{
    self, DOTDOTI, DOTS, IDENT, R_BOGUS_FOR_VARIABLE, R_DOT_DOT_I, R_DOTS, R_FOR_STATEMENT,
    R_IDENTIFIER, R_REPEAT_STATEMENT, R_WHILE_STATEMENT,
};
use aether_r_syntax::T;
use biome_parser::CompletedMarker;
use biome_parser::parse_recovery::ParseRecoveryTokenSet;
use biome_parser::parsed_syntax::ParsedSyntax::{self, Absent, Present};
use biome_parser::{Parser, TokenSet, token_set};

/// Where a `for` variable may stop: at the `in` that follows it, or at an end
///
/// Any closing delimiter, not just the one the caller opened, for the reason
/// [crate::syntax::call]'s `ARGUMENT_LIST_ENDS` gives.
const FOR_VARIABLE_ENDS: TokenSet<RSyntaxKind> =
    token_set![T![EOF], T![in], T![')'], T![']'], T!["]]"], T!['}']];

pub(crate) fn parse_for_statement(p: &mut RParser) -> CompletedMarker {
    let m = p.start();
    p.bump(T![for]);

    p.with_scope(Scope::Parenthesis, |p| {
        p.expect(T!['(']);
        parse_for_variable(p)
            .or_recover_with_token_set(
                p,
                &ParseRecoveryTokenSet::new(R_BOGUS_FOR_VARIABLE, FOR_VARIABLE_ENDS),
                diagnostics::expected_for_variable,
            )
            .ok();
        p.expect(T![in]);
        parse_expression(p).or_add_diagnostic(p, diagnostics::expected_for_sequence);
    });
    p.expect(T![')']);

    parse_body_expression(p).or_add_diagnostic(p, diagnostics::expected_loop_body);

    m.complete(p, R_FOR_STATEMENT)
}

/// identifier | `...` | `..i`
fn parse_for_variable(p: &mut RParser) -> ParsedSyntax {
    match p.cur() {
        IDENT => {
            let m = p.start();
            p.bump(IDENT);
            Present(m.complete(p, R_IDENTIFIER))
        }
        DOTS => {
            let m = p.start();
            p.bump(DOTS);
            Present(m.complete(p, R_DOTS))
        }
        DOTDOTI => {
            let m = p.start();
            p.bump(DOTDOTI);
            Present(m.complete(p, R_DOT_DOT_I))
        }
        _ => Absent,
    }
}

pub(crate) fn parse_while_statement(p: &mut RParser) -> CompletedMarker {
    let m = p.start();
    p.bump(T![while]);

    p.with_scope(Scope::Parenthesis, |p| {
        p.expect(T!['(']);
        parse_expression(p).or_add_diagnostic(p, diagnostics::expected_condition);
    });
    p.expect(T![')']);

    parse_body_expression(p).or_add_diagnostic(p, diagnostics::expected_loop_body);

    m.complete(p, R_WHILE_STATEMENT)
}

pub(crate) fn parse_repeat_statement(p: &mut RParser) -> CompletedMarker {
    let m = p.start();
    p.bump(T![repeat]);

    parse_body_expression(p).or_add_diagnostic(p, diagnostics::expected_loop_body);

    m.complete(p, R_REPEAT_STATEMENT)
}
