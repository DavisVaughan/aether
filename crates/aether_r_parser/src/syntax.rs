use crate::diagnostics::not_yet_implemented;
use crate::parser::RParser;
use aether_r_syntax::RSyntaxKind::{
    EOF, R_BOGUS_EXPRESSION, R_ROOT, R_STATEMENT, R_STATEMENT_LIST, UNICODE_BOM,
};
use biome_parser::Parser;

pub(crate) fn parse_root(p: &mut RParser) {
    let root = p.start();
    p.eat(UNICODE_BOM);

    let statements = p.start();
    parse_placeholder_statement(p);
    statements.complete(p, R_STATEMENT_LIST);

    p.expect(EOF);
    root.complete(p, R_ROOT);
}

/// Swallows the file into one bogus statement
///
/// Phase 7 replaces this with `parse_statement_list()`. Until then the corpus
/// needs every non-empty input to round-trip and to *not* parse cleanly, which
/// is what makes a `[pending]` marker mean something.
fn parse_placeholder_statement(p: &mut RParser) {
    if p.at(EOF) {
        return;
    }

    let statement = p.start();
    let bogus = p.start();

    p.error(not_yet_implemented(p.cur_range()));
    while !p.at(EOF) {
        p.bump_any();
    }

    bogus.complete(p, R_BOGUS_EXPRESSION);
    statement.complete(p, R_STATEMENT);
}
