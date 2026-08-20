use crate::diagnostics;
use crate::parser::{RParser, Scope};
use crate::syntax::expr::parse_expression;
use aether_r_syntax::RSyntaxKind::R_PARENTHESIZED_EXPRESSION;
use aether_r_syntax::T;
use biome_parser::Parser;
use biome_parser::parsed_syntax::ParsedSyntax::{self, Present};

pub(crate) fn parse_parenthesized_expression(p: &mut RParser) -> ParsedSyntax {
    debug_assert!(p.at(T!['(']));

    let m = p.start();
    p.with_scope(Scope::Parenthesis, |p| {
        p.bump(T!['(']);
        parse_expression(p).or_add_diagnostic(p, diagnostics::expected_expression);
    });
    p.expect(T![')']);

    Present(m.complete(p, R_PARENTHESIZED_EXPRESSION))
}
