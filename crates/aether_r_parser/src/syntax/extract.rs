use crate::diagnostics;
use crate::parser::RParser;
use crate::syntax::selector::parse_selector;
use aether_r_syntax::RSyntaxKind::R_EXTRACT_EXPRESSION;
use biome_parser::CompletedMarker;
use biome_parser::Parser;

pub(crate) fn parse_extract_expression(p: &mut RParser, lhs: CompletedMarker) -> CompletedMarker {
    let operator = p.cur();
    let operator_range = p.cur_range();
    let m = lhs.precede(p);
    p.bump_any();
    parse_selector(p).or_add_diagnostic(p, |_, range| {
        diagnostics::expected_rhs_selector(operator, operator_range, range)
    });
    m.complete(p, R_EXTRACT_EXPRESSION)
}
