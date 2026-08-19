mod call;
mod expr;
mod parenthesized_expression;
mod statement_list;
mod value;

use crate::parser::RParser;
use crate::syntax::statement_list::parse_statement_list;
use aether_r_syntax::RSyntaxKind::{EOF, R_ROOT, UNICODE_BOM};
use biome_parser::Parser;

pub(crate) fn parse_root(p: &mut RParser) {
    let m = p.start();
    p.eat(UNICODE_BOM);
    parse_statement_list(p, EOF);
    p.expect(EOF);
    m.complete(p, R_ROOT);
}
