use crate::diagnostics;
use crate::parser::{RParser, Scope};
use crate::syntax::body::parse_body_expression;
use crate::syntax::expr::parse_expression;
use aether_r_syntax::RSyntaxKind::{
    self, DOTDOTI, DOTS, IDENT, R_BOGUS_PARAMETER_NAME, R_DOT_DOT_I, R_DOTS, R_FUNCTION_DEFINITION,
    R_IDENTIFIER, R_PARAMETER, R_PARAMETER_DEFAULT, R_PARAMETER_LIST, R_PARAMETERS,
};
use aether_r_syntax::T;
use biome_parser::CompletedMarker;
use biome_parser::parse_recovery::ParseRecoveryTokenSet;
use biome_parser::parsed_syntax::ParsedSyntax::{self, Absent, Present};
use biome_parser::{Parser, ParserProgress, TokenSet, token_set};

pub(crate) fn parse_function_definition(p: &mut RParser, keyword: RSyntaxKind) -> CompletedMarker {
    let m = p.start();
    p.bump(keyword);

    if p.at(T!['(']) {
        parse_parameters(p);
    } else {
        p.error(diagnostics::expected_parameters(p, p.cur_range()));
    }

    parse_body_expression(p).or_add_diagnostic(p, diagnostics::expected_function_body);

    m.complete(p, R_FUNCTION_DEFINITION)
}

fn parse_parameters(p: &mut RParser) {
    let m = p.start();
    p.with_scope(Scope::Parenthesis, |p| {
        p.bump(T!['(']);
        parse_parameter_list(p);
    });
    p.expect(T![')']);
    m.complete(p, R_PARAMETERS);
}

/// What ends a parameter list
///
/// Any closing delimiter, not just the one the caller opened, for the reason
/// [crate::syntax::call]'s `ARGUMENT_LIST_ENDS` gives.
const PARAMETER_LIST_ENDS: TokenSet<RSyntaxKind> =
    token_set![T![EOF], T![')'], T![']'], T!["]]"], T!['}']];

/// Where a parameter may stop: at the `=` of its own default, at the `,` that
/// starts the next one, or at an end
const PARAMETER_LIST_BOUNDARIES: TokenSet<RSyntaxKind> =
    PARAMETER_LIST_ENDS.union(token_set![T![,], T![=]]);

fn parse_parameter_list(p: &mut RParser) {
    let parameter_list = p.start();
    let mut progress = ParserProgress::default();
    let mut first = true;

    while !p.at_ts(PARAMETER_LIST_ENDS) {
        if first {
            first = false;
        } else {
            // Either a `,` that introduces the next parameter, or a missing one
            // that we report without consuming and let the next parameter start
            // where we already are
            p.expect(T![,]);
        }

        progress.assert_progressing(p);
        parse_parameter(p);
    }

    parameter_list.complete(p, R_PARAMETER_LIST);
}

/// One parameter: `name [= default]`
fn parse_parameter(p: &mut RParser) {
    let m = p.start();

    parse_parameter_name(p)
        .or_recover_with_token_set(
            p,
            &ParseRecoveryTokenSet::new(R_BOGUS_PARAMETER_NAME, PARAMETER_LIST_BOUNDARIES),
            diagnostics::expected_parameter_name,
        )
        .ok();

    if p.at(T![=]) {
        parse_parameter_default(p);
    }

    m.complete(p, R_PARAMETER);
}

/// identifier | `...` | `..i`
fn parse_parameter_name(p: &mut RParser) -> ParsedSyntax {
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
        _ => Absent,
    }
}

/// `= value`, where the value is required (r-lib/tree-sitter-r#161)
fn parse_parameter_default(p: &mut RParser) {
    let m = p.start();
    p.bump(T![=]);
    parse_expression(p).or_add_diagnostic(p, diagnostics::expected_expression);
    m.complete(p, R_PARAMETER_DEFAULT);
}
