use crate::parser::RParser;
use aether_r_syntax::RSyntaxKind;
use biome_parser::CompletedMarker;
use biome_parser::Parser;

/// Bumps a single token and wraps it in `kind_complete`
///
/// The shared leaf shape behind identifiers, `...`, `..i`, NA variants,
/// keyword expressions, and literals wherever they're parsed as a single
/// token, like in [crate::syntax::expr::parse_expression_with_binding_power()]
/// and [crate::syntax::selector::parse_selector()].
pub(crate) fn parse_literal_expression(
    p: &mut RParser,
    kind_bump: RSyntaxKind,
    kind_complete: RSyntaxKind,
) -> CompletedMarker {
    let m = p.start();
    p.bump(kind_bump);
    m.complete(p, kind_complete)
}
