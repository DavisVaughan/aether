use crate::diagnostics;
use crate::parser::{RParser, Scope};
use crate::syntax::expr::{parse_expression, parse_function_body};
use aether_r_syntax::RSyntaxKind::{
    self, DOTDOTI, DOTS, IDENT, R_BOGUS_PARAMETER_NAME, R_DOT_DOT_I, R_DOTS, R_FUNCTION_DEFINITION,
    R_IDENTIFIER, R_PARAMETER, R_PARAMETER_DEFAULT, R_PARAMETER_LIST, R_PARAMETERS,
};
use aether_r_syntax::T;
use biome_parser::CompletedMarker;
use biome_parser::parse_recovery::ParseRecoveryTokenSet;
use biome_parser::parsed_syntax::ParsedSyntax::{self, Absent, Present};
use biome_parser::{Parser, ParserProgress, TokenSet, token_set};

pub(crate) fn parse_function_definition(p: &mut RParser) -> CompletedMarker {
    debug_assert!(p.at(T![function]) || p.at(T!["\\"]));

    let m = p.start();
    p.bump_any();

    parse_parameters(p).or_add_diagnostic(p, diagnostics::expected_parameters);
    parse_function_body(p).or_add_diagnostic(p, diagnostics::expected_expression);

    m.complete(p, R_FUNCTION_DEFINITION)
}

fn parse_parameters(p: &mut RParser) -> ParsedSyntax {
    if !p.at(T!['(']) {
        // Caller will report a diagnostic
        return Absent;
    }

    let m = p.start();
    p.with_scope(Scope::Parenthesis, |p| {
        p.bump(T!['(']);
        parse_parameter_list(p);
    });
    p.expect(T![')']);

    Present(m.complete(p, R_PARAMETERS))
}

const PARAMETER_LIST_ENDS: TokenSet<RSyntaxKind> =
    token_set![T![EOF], T![')'], T![']'], T!["]]"], T!['}']];

const PARAMETER_LIST_BOUNDARIES: TokenSet<RSyntaxKind> =
    PARAMETER_LIST_ENDS.union(token_set![T![,]]);

fn parse_parameter_list(p: &mut RParser) {
    let parameter_list = p.start();
    let mut progress = ParserProgress::default();

    // Account for empty parameter list
    if !p.at_ts(PARAMETER_LIST_ENDS) {
        loop {
            progress.assert_progressing(p);
            parse_parameter(p);

            if p.at_ts(PARAMETER_LIST_ENDS) {
                break;
            }

            p.expect(T![,]);
        }
    }

    parameter_list.complete(p, R_PARAMETER_LIST);
}

fn parse_parameter(p: &mut RParser) {
    let m = p.start();

    if parse_parameter_name(p).is_absent() {
        p.error(diagnostics::expected_parameter(p, p.cur_range()));

        if !p.at_ts(PARAMETER_LIST_BOUNDARIES) {
            let recovered =
                ParseRecoveryTokenSet::new(R_BOGUS_PARAMETER_NAME, PARAMETER_LIST_BOUNDARIES)
                    .recover(p);
            debug_assert!(
                recovered.is_ok(),
                "not at a boundary, so not at `EOF` either"
            );
        }
    }

    parse_parameter_default(p).ok();

    m.complete(p, R_PARAMETER);
}

fn parse_parameter_name(p: &mut RParser) -> ParsedSyntax {
    match p.cur() {
        IDENT => {
            let m = p.start();
            p.bump_any();
            Present(m.complete(p, R_IDENTIFIER))
        }
        DOTS => {
            let m = p.start();
            p.bump_any();
            Present(m.complete(p, R_DOTS))
        }
        DOTDOTI => {
            let m = p.start();
            p.bump_any();
            Present(m.complete(p, R_DOT_DOT_I))
        }
        _ => Absent,
    }
}

fn parse_parameter_default(p: &mut RParser) -> ParsedSyntax {
    if !p.at(T![=]) {
        return Absent;
    }

    let m = p.start();
    p.bump(T![=]);
    parse_expression(p).or_add_diagnostic(p, diagnostics::expected_expression);

    Present(m.complete(p, R_PARAMETER_DEFAULT))
}
