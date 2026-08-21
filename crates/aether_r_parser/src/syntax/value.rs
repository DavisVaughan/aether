use crate::parser::RParser;
use aether_r_syntax::RSyntaxKind::{
    self, R_DOUBLE_QUOTED_STRING_LITERAL, R_RAW_STRING_LITERAL, R_SINGLE_QUOTED_STRING_LITERAL,
    R_STRING_VALUE,
};
use biome_parser::parsed_syntax::ParsedSyntax::{self, Present};
use biome_parser::{Parser, TokenSet, token_set};

pub(crate) const STRING_LITERALS: TokenSet<RSyntaxKind> = token_set![
    R_SINGLE_QUOTED_STRING_LITERAL,
    R_DOUBLE_QUOTED_STRING_LITERAL,
    R_RAW_STRING_LITERAL,
];

pub(crate) fn parse_string_value(p: &mut RParser) -> ParsedSyntax {
    debug_assert!(p.at_ts(STRING_LITERALS));

    let m = p.start();
    p.bump_any();
    Present(m.complete(p, R_STRING_VALUE))
}
