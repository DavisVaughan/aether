use crate::parser::RParser;
use aether_r_syntax::RSyntaxKind::{R_ROOT, R_STATEMENT_LIST, UNICODE_BOM};
use biome_parser::Parser;

/// Parses a whole file
///
/// The statement list is empty until Phase 7 teaches the parser an expression.
/// The `EOF` token `RRoot` requires is not emitted here; `LosslessTreeSink`
/// appends one when the root node closes.
pub(crate) fn parse_root(p: &mut RParser) {
    let root = p.start();
    p.eat(UNICODE_BOM);

    let statements = p.start();
    statements.complete(p, R_STATEMENT_LIST);

    root.complete(p, R_ROOT);
}
