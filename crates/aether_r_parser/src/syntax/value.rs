use crate::diagnostics;
use crate::lexer::{RLexContext, RStringDelimiter};
use crate::parser::RParser;
use aether_r_syntax::RSyntaxKind::{R_STRING_VALUE, STRING_CLOSE, STRING_CONTENT, STRING_OPEN};
use biome_parser::Parser;
use biome_parser::parsed_syntax::ParsedSyntax::{self, Present};

pub(crate) fn parse_string_value(p: &mut RParser) -> ParsedSyntax {
    debug_assert!(p.at(STRING_OPEN));

    let m = p.start();

    // The same value serves both bumps: the text at `STRING_CONTENT` is the body
    let delimiter = RStringDelimiter::from_open(p.cur_text());
    let open_range = p.cur_range();

    p.bump_with_context(STRING_OPEN, RLexContext::StringContent(delimiter));
    // An empty body emits no `STRING_CONTENT` at all
    if p.at(STRING_CONTENT) {
        p.bump_with_context(STRING_CONTENT, RLexContext::StringEnd(delimiter));
    }

    // Not `expect()`: `STRING_CLOSE` has no spelling to name in its message, and
    // the body ran to EOF, so there is a better one to give
    if !p.eat(STRING_CLOSE) {
        p.error(diagnostics::unterminated_string(open_range, p.cur_range()));
    }

    Present(m.complete(p, R_STRING_VALUE))
}
