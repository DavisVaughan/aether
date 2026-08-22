use crate::diagnostics;
use crate::parser::{RParser, Scope};
use crate::syntax::expr::{BindingPower, parse_expression, parse_expression_with_binding_power};
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
    parse_body(p).or_add_diagnostic(p, diagnostics::expected_expression);

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

/// What ends a parameter list
///
/// Any closing delimiter, not just the one the caller opened, for the reason
/// [crate::syntax::call]'s `ARGUMENT_LIST_ENDS` gives.
const PARAMETER_LIST_ENDS: TokenSet<RSyntaxKind> =
    token_set![T![EOF], T![')'], T![']'], T!["]]"], T!['}']];

/// Where a parameter may stop: at the `,` that starts the next one, or at an end
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

            // Either a `,` that introduces the next parameter, or a missing one
            // that we report without consuming and let the next parameter start
            // where we already are
            p.expect(T![,]);
        }
    }

    parameter_list.complete(p, R_PARAMETER_LIST);
}

/// One parameter: `name [= default]`
///
/// An empty parameter opens and immediately closes its marker, pushing no
/// children, which is what makes `function(a,,b)` representable. Unlike an
/// argument it is never legal, so it always comes with a diagnostic.
fn parse_parameter(p: &mut RParser) {
    let m = p.start();

    parse_parameter_name(p)
        .or_recover_with_token_set(
            p,
            &ParseRecoveryTokenSet::new(R_BOGUS_PARAMETER_NAME, PARAMETER_LIST_BOUNDARIES),
            diagnostics::expected_parameter,
        )
        .ok();

    // Optional (hence the `ok()`)
    parse_parameter_default(p).ok();

    m.complete(p, R_PARAMETER);
}

/// identifier | `...` | `..i`
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

/// `= value`, where the value is required (r-lib/tree-sitter-r#161)
fn parse_parameter_default(p: &mut RParser) -> ParsedSyntax {
    if !p.at(T![=]) {
        return Absent;
    }

    let m = p.start();
    p.bump(T![=]);
    parse_expression(p).or_add_diagnostic(p, diagnostics::expected_expression);

    Present(m.complete(p, R_PARAMETER_DEFAULT))
}

/// Parse a function body
///
/// Generally treated like a normal expression, but R's precedence table puts
/// `?` at a lower binding power than a function body, so `function() a ? b`
/// needs to parse as `(function () a) ? b`.
fn parse_body(p: &mut RParser) -> ParsedSyntax {
    // Treated as left associative
    let binding_power = BindingPower::FUNCTION_OR_LOOP.increment();
    parse_expression_with_binding_power(p, binding_power)
}
