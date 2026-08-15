use crate::parser::RParser;
use aether_r_syntax::RSyntaxKind::{R_ROOT, R_STATEMENT_LIST, UNICODE_BOM};
use biome_parser::Parser;

pub(crate) fn parse_root(p: &mut RParser) {
    let root = p.start();
    p.eat(UNICODE_BOM);

    let statements = p.start();
    statements.complete(p, R_STATEMENT_LIST);

    root.complete(p, R_ROOT);
}
