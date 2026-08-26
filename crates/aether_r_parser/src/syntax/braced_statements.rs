use crate::parser::{RParser, Scope};
use crate::syntax::statement_list::parse_statement_list;
use aether_r_syntax::RSyntaxKind::R_BRACED_STATEMENTS;
use aether_r_syntax::T;
use biome_parser::CompletedMarker;
use biome_parser::Parser;

pub(crate) fn parse_braced_statements(p: &mut RParser) -> CompletedMarker {
    let m = p.start();
    p.with_scope(Scope::Brace, |p| {
        p.bump(T!['{']);
        parse_statement_list(p, T!['}']);
    });
    p.expect(T!['}']);
    m.complete(p, R_BRACED_STATEMENTS)
}
