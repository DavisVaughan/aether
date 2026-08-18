//! Pratt parser core
//!
//! https://matklad.github.io/2020/04/13/simple-but-powerful-pratt-parsing.html
//! https://kevinushey.github.io/blog/2016/02/12/top-down-operator-precedence-parsing-with-r/

use crate::diagnostics;
use crate::parser::RParser;
use crate::syntax::call::{parse_call_arguments, parse_subset_arguments, parse_subset2_arguments};
use crate::syntax::parenthesized_expression::parse_parenthesized_expression;
use crate::syntax::value::parse_string_value;
use aether_r_syntax::RSyntaxKind::{self, *};
use aether_r_syntax::T;
use biome_parser::CompletedMarker;
use biome_parser::Parser;
use biome_parser::parsed_syntax::ParsedSyntax::{self, Absent, Present};

/// How tightly an operator binds. Higher wins!
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub(crate) struct BindingPower(u8);

impl BindingPower {
    /// Accepts every operator
    pub(crate) const LOWEST: Self = Self(0);

    /// Rank of `f(…)`, `x[…]`, `x[[…]]`
    const HIGHEST: Self = Self(18);
}

#[derive(Debug, Clone, Copy)]
enum Associativity {
    Left,
    Right,
}

/// Taken from R's `gram.y`
///
/// `$`, `@`, `::`, and `:::` are missing until Phase 10 gives them a right-hand
/// side of their own
fn infix_binding_power(kind: RSyntaxKind) -> Option<(BindingPower, Associativity)> {
    Some(match kind {
        T![?] => (BindingPower(1), Associativity::Left),
        T![<-] | T![<<-] | T![:=] => (BindingPower(2), Associativity::Right),
        T![=] => (BindingPower(3), Associativity::Right),
        T![->] | T![->>] => (BindingPower(4), Associativity::Left),
        T![~] => (BindingPower(5), Associativity::Left),
        T![|] | T![||] => (BindingPower(6), Associativity::Left),
        T![&] | T![&&] => (BindingPower(7), Associativity::Left),
        T![<] | T![<=] | T![>] | T![>=] | T![==] | T![!=] => (BindingPower(9), Associativity::Left),
        T![+] | T![-] => (BindingPower(10), Associativity::Left),
        T![*] | T![/] => (BindingPower(11), Associativity::Left),
        SPECIAL | T![|>] => (BindingPower(12), Associativity::Left),
        T![:] => (BindingPower(13), Associativity::Left),
        T![^] | T![**] => (BindingPower(15), Associativity::Right),
        _ => return None,
    })
}

fn prefix_binding_power(kind: RSyntaxKind) -> Option<BindingPower> {
    Some(match kind {
        T![?] => BindingPower(1),
        T![~] => BindingPower(5),
        T![!] => BindingPower(8),
        T![+] | T![-] => BindingPower(14),
        _ => return None,
    })
}

pub(crate) fn parse_expression(p: &mut RParser) -> ParsedSyntax {
    parse_expression_with_binding_power(p, BindingPower::LOWEST)
}

fn parse_expression_with_binding_power(
    p: &mut RParser,
    current_binding_power: BindingPower,
) -> ParsedSyntax {
    let Present(lhs) = parse_prefix_expression(p).or_else(|| parse_primary_expression(p)) else {
        return Absent;
    };
    Present(parse_expression_with_lhs(p, lhs, current_binding_power))
}

/// The Pratt loop, entered with an already-parsed left operand
///
/// Split out for [crate::syntax::call::parse_argument()], which speculatively
/// parses an argument name and hands the atom over when no `=` follows it.
pub(crate) fn parse_expression_with_lhs(
    p: &mut RParser,
    mut lhs: CompletedMarker,
    current_binding_power: BindingPower,
) -> CompletedMarker {
    // Asserted rather than branched on, so that adding an infix operator at rank
    // 18 or above fails loudly instead of silently reaching dead code
    debug_assert!(
        current_binding_power <= BindingPower::HIGHEST,
        "{current_binding_power:?} exceeds {:?}; postfix must stay the tightest-binding construct",
        BindingPower::HIGHEST
    );

    loop {
        // `1\n+2` is two statements, `1 +\n2` is one
        if p.scope().newlines_are_significant() && p.has_preceding_line_break() {
            break;
        }

        // Rank 18, so no binding power guard: no caller wants a postfix refused
        if let Some(call) = parse_postfix_expression(p, &lhs) {
            lhs = call;
            continue;
        }

        let Some((new_binding_power, associativity)) = infix_binding_power(p.cur()) else {
            break;
        };
        if new_binding_power < current_binding_power {
            break;
        }
        let new_binding_power = match associativity {
            Associativity::Left => BindingPower(new_binding_power.0 + 1),
            Associativity::Right => new_binding_power,
        };

        let m = lhs.precede(p);
        p.bump_any();
        parse_expression_with_binding_power(p, new_binding_power)
            .or_add_diagnostic(p, diagnostics::expected_expression);

        lhs = m.complete(p, R_BINARY_EXPRESSION);
    }

    lhs
}

/// Postfix: `f(x)`, `x[i]`, `x[[i]]`
fn parse_postfix_expression(p: &mut RParser, lhs: &CompletedMarker) -> Option<CompletedMarker> {
    match p.cur() {
        T!['('] => {
            let m = lhs.clone().precede(p);
            parse_call_arguments(p);
            Some(m.complete(p, R_CALL))
        }
        T!['['] => {
            let m = lhs.clone().precede(p);
            parse_subset_arguments(p);
            Some(m.complete(p, R_SUBSET))
        }
        T!["[["] => {
            let m = lhs.clone().precede(p);
            parse_subset2_arguments(p);
            Some(m.complete(p, R_SUBSET2))
        }
        _ => None,
    }
}

fn parse_prefix_expression(p: &mut RParser) -> ParsedSyntax {
    let Some(bp) = prefix_binding_power(p.cur()) else {
        return Absent;
    };

    let m = p.start();
    p.bump_any();
    parse_expression_with_binding_power(p, bp)
        .or_add_diagnostic(p, diagnostics::expected_expression);
    Present(m.complete(p, R_UNARY_EXPRESSION))
}

fn parse_primary_expression(p: &mut RParser) -> ParsedSyntax {
    match p.cur() {
        R_INTEGER_LITERAL => {
            let m = p.start();
            p.bump_any();
            Present(m.complete(p, R_INTEGER_VALUE))
        }
        R_DOUBLE_LITERAL => {
            let m = p.start();
            p.bump_any();
            Present(m.complete(p, R_DOUBLE_VALUE))
        }
        R_COMPLEX_LITERAL => {
            let m = p.start();
            p.bump_any();
            Present(m.complete(p, R_COMPLEX_VALUE))
        }
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
        T![next] => {
            let m = p.start();
            p.bump_any();
            Present(m.complete(p, R_NEXT_EXPRESSION))
        }
        T![break] => {
            let m = p.start();
            p.bump_any();
            Present(m.complete(p, R_BREAK_EXPRESSION))
        }
        T![true] => {
            let m = p.start();
            p.bump_any();
            Present(m.complete(p, R_TRUE_EXPRESSION))
        }
        T![false] => {
            let m = p.start();
            p.bump_any();
            Present(m.complete(p, R_FALSE_EXPRESSION))
        }
        T![null] => {
            let m = p.start();
            p.bump_any();
            Present(m.complete(p, R_NULL_EXPRESSION))
        }
        T![inf] => {
            let m = p.start();
            p.bump_any();
            Present(m.complete(p, R_INF_EXPRESSION))
        }
        T![nan] => {
            let m = p.start();
            p.bump_any();
            Present(m.complete(p, R_NAN_EXPRESSION))
        }
        T![na_logical] | T![na_integer] | T![na_double] | T![na_complex] | T![na_character] => {
            let m = p.start();
            p.bump_any();
            Present(m.complete(p, R_NA_EXPRESSION))
        }
        // Lexer already raised `unexpected_character()`
        ERROR_TOKEN => {
            let m = p.start();
            p.bump_any();
            Present(m.complete(p, R_BOGUS_EXPRESSION))
        }

        STRING_OPEN => parse_string_value(p),
        T!['('] => parse_parenthesized_expression(p),

        _ => Absent,
    }
}
