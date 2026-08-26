use crate::parser::RParser;
use aether_r_syntax::RSyntaxKind::{
    DOTDOTI, DOTS, IDENT, R_DOT_DOT_I, R_DOTS, R_DOUBLE_QUOTED_STRING_LITERAL, R_IDENTIFIER,
    R_RAW_STRING_LITERAL, R_SINGLE_QUOTED_STRING_LITERAL, R_STRING_VALUE,
};
use biome_parser::Parser;
use biome_parser::parsed_syntax::ParsedSyntax::{self, Absent, Present};

/// identifier | string | `...` | `..i`
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
        R_SINGLE_QUOTED_STRING_LITERAL | R_DOUBLE_QUOTED_STRING_LITERAL | R_RAW_STRING_LITERAL => {
            let m = p.start();
            p.bump_any();
            Present(m.complete(p, R_STRING_VALUE))
        }
        _ => Absent,
    }
}
