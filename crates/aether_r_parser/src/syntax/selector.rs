use crate::parser::RParser;
use aether_r_syntax::RSyntaxKind::DOTDOTI;
use aether_r_syntax::RSyntaxKind::DOTS;
use aether_r_syntax::RSyntaxKind::IDENT;
use aether_r_syntax::RSyntaxKind::R_DOT_DOT_I;
use aether_r_syntax::RSyntaxKind::R_DOTS;
use aether_r_syntax::RSyntaxKind::R_DOUBLE_QUOTED_STRING_LITERAL;
use aether_r_syntax::RSyntaxKind::R_IDENTIFIER;
use aether_r_syntax::RSyntaxKind::R_RAW_STRING_LITERAL;
use aether_r_syntax::RSyntaxKind::R_SINGLE_QUOTED_STRING_LITERAL;
use aether_r_syntax::RSyntaxKind::R_STRING_VALUE;
use biome_parser::Parser;
use biome_parser::parsed_syntax::ParsedSyntax;
use biome_parser::parsed_syntax::ParsedSyntax::Absent;
use biome_parser::parsed_syntax::ParsedSyntax::Present;

/// identifier | string | `...` | `..i`
///
/// Used by [crate::syntax::extract::parse_extract_expression()] and
/// [crate::syntax::namespace::parse_namespace_expression()]
pub(crate) fn parse_selector(p: &mut RParser) -> ParsedSyntax {
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
        kind @ (R_SINGLE_QUOTED_STRING_LITERAL
        | R_DOUBLE_QUOTED_STRING_LITERAL
        | R_RAW_STRING_LITERAL) => {
            let m = p.start();
            p.bump(kind);
            Present(m.complete(p, R_STRING_VALUE))
        }
        _ => Absent,
    }
}
