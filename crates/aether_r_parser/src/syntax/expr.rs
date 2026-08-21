//! Pratt parser core
//!
//! https://matklad.github.io/2020/04/13/simple-but-powerful-pratt-parsing.html
//! https://kevinushey.github.io/blog/2016/02/12/top-down-operator-precedence-parsing-with-r/

use crate::diagnostics;
use crate::parser::RParser;
use crate::syntax::call::{parse_call_arguments, parse_subset_arguments, parse_subset2_arguments};
use crate::syntax::parenthesized_expression::parse_parenthesized_expression;
use aether_r_syntax::RSyntaxKind::{self, *};
use aether_r_syntax::{AnyRSelector, T};
use biome_parser::CompletedMarker;
use biome_parser::Parser;
use biome_parser::parsed_syntax::ParsedSyntax::{self, Absent, Present};
use biome_rowan::{AstNode, TextRange};

/// How tightly an operator binds. Higher wins!
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
struct BindingPower(u8);

impl BindingPower {
    const LOWEST: Self = Self(0);

    /// Technically would represent the binding power of postfix operations,
    /// like `f(...)`, `x[...]`, and `x[[...]]`. This mostly just falls out
    /// structurally, but we have a debug assertion that uses this just to be
    /// sure.
    const HIGHEST: Self = Self(18);
}

#[derive(Debug, Clone, Copy)]
enum Associativity {
    Left,
    Right,
}

pub(crate) fn parse_expression(p: &mut RParser) -> ParsedSyntax {
    parse_expression_with_binding_power(p, BindingPower::LOWEST)
}

/// The Pratt loop
fn parse_expression_with_binding_power(
    p: &mut RParser,
    current_binding_power: BindingPower,
) -> ParsedSyntax {
    debug_assert!(
        current_binding_power <= BindingPower::HIGHEST,
        "{current_binding_power:?} exceeds {:?}. Postfix must stay the tightest-binding construct!",
        BindingPower::HIGHEST
    );

    let Present(mut lhs) = parse_prefix_expression(p).or_else(|| parse_primary_expression(p))
    else {
        return Absent;
    };

    loop {
        if p.scope().newlines_are_significant() && p.has_preceding_line_break() {
            // Finish current expression due to line break
            break;
        }

        // No need for binding power here. Postfix operations all share the
        // maximum binding power.
        if let Present(new_lhs) = parse_postfix_expression(p, &lhs) {
            lhs = new_lhs;
            continue;
        }

        if let Present(new_lhs) = parse_infix_binary_expression(p, &lhs, current_binding_power) {
            lhs = new_lhs;
            continue;
        }

        if let Present(new_lhs) = parse_infix_extract_expression(p, &lhs, current_binding_power) {
            lhs = new_lhs;
            continue;
        }

        if let Present(new_lhs) = parse_infix_namespace_expression(p, &lhs, current_binding_power) {
            lhs = new_lhs;
            continue;
        }

        break;
    }

    Present(lhs)
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
        ERROR_TOKEN => {
            // Lexer already raised `unexpected_character()`
            let m = p.start();
            p.bump_any();
            Present(m.complete(p, R_BOGUS_EXPRESSION))
        }
        R_SINGLE_QUOTED_STRING_LITERAL | R_DOUBLE_QUOTED_STRING_LITERAL | R_RAW_STRING_LITERAL => {
            let m = p.start();
            p.bump_any();
            Present(m.complete(p, R_STRING_VALUE))
        }
        T!['('] => parse_parenthesized_expression(p),
        _ => Absent,
    }
}

fn parse_prefix_expression(p: &mut RParser) -> ParsedSyntax {
    let Some(binding_power) = prefix_binding_power(p.cur()) else {
        // Not a prefix operator
        return Absent;
    };

    let m = p.start();
    p.bump_any();
    parse_expression_with_binding_power(p, binding_power)
        .or_add_diagnostic(p, diagnostics::expected_expression);
    Present(m.complete(p, R_UNARY_EXPRESSION))
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

/// Infix: `a + b`, `a <- b`, `a %in% b`
fn parse_infix_binary_expression(
    p: &mut RParser,
    lhs: &CompletedMarker,
    current_binding_power: BindingPower,
) -> ParsedSyntax {
    let Some((new_binding_power, associativity)) = infix_binary_binding_power(p.cur()) else {
        // Not an infix binary operator
        return Absent;
    };
    if new_binding_power < current_binding_power {
        // New operator binds too loosely
        return Absent;
    }
    let new_binding_power = match associativity {
        Associativity::Left => BindingPower(new_binding_power.0 + 1),
        Associativity::Right => new_binding_power,
    };

    let m = lhs.clone().precede(p);
    p.bump_any();
    parse_expression_with_binding_power(p, new_binding_power)
        .or_add_diagnostic(p, diagnostics::expected_expression);
    Present(m.complete(p, R_BINARY_EXPRESSION))
}

/// Taken from R's `gram.y`
fn infix_binary_binding_power(kind: RSyntaxKind) -> Option<(BindingPower, Associativity)> {
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

/// Infix: `a$b`, `a@b`
fn parse_infix_extract_expression(
    p: &mut RParser,
    lhs: &CompletedMarker,
    current_binding_power: BindingPower,
) -> ParsedSyntax {
    let Some(new_binding_power) = infix_extract_binding_power(p.cur()) else {
        return Absent;
    };
    if new_binding_power < current_binding_power {
        return Absent;
    }

    let operator = p.cur();
    let operator_range = p.cur_range();

    let m = lhs.clone().precede(p);
    p.bump_any();
    parse_selector(p).or_add_diagnostic(p, |p, range| {
        diagnostics::expected_selector(p, operator, operator_range, range)
    });
    Present(m.complete(p, R_EXTRACT_EXPRESSION))
}

/// No associativity: the right operand is one selector, never a nested expression
fn infix_extract_binding_power(kind: RSyntaxKind) -> Option<BindingPower> {
    match kind {
        T![$] | T![@] => Some(BindingPower(16)),
        _ => None,
    }
}

/// Infix: `a::b`, `a:::b`
fn parse_infix_namespace_expression(
    p: &mut RParser,
    lhs: &CompletedMarker,
    current_binding_power: BindingPower,
) -> ParsedSyntax {
    let Some(new_binding_power) = infix_namespace_binding_power(p.cur()) else {
        return Absent;
    };
    if new_binding_power < current_binding_power {
        return Absent;
    }

    let operator = p.cur();
    let operator_range = p.cur_range();

    let m = parse_namespace_left(p, lhs, operator, operator_range).precede(p);
    p.bump_any();

    if p.scope().newlines_are_significant() && p.has_preceding_line_break() {
        p.error(diagnostics::selector_after_line_break(
            operator,
            operator_range,
        ));
    } else {
        parse_selector(p).or_add_diagnostic(p, |p, range| {
            diagnostics::expected_selector(p, operator, operator_range, range)
        });
    }

    Present(m.complete(p, R_NAMESPACE_EXPRESSION))
}

/// No associativity: the right operand is one selector, never a nested expression
fn infix_namespace_binding_power(kind: RSyntaxKind) -> Option<BindingPower> {
    match kind {
        T![::] | T![:::] => Some(BindingPower(17)),
        _ => None,
    }
}

/// `::` and `:::` restrict their left operand as well, and it is already parsed
fn parse_namespace_left(
    p: &mut RParser,
    lhs: &CompletedMarker,
    operator: RSyntaxKind,
    operator_range: TextRange,
) -> CompletedMarker {
    if AnyRSelector::can_cast(lhs.kind(p)) {
        return lhs.clone();
    }

    p.error(diagnostics::expected_selector_on_the_left(
        p,
        operator,
        operator_range,
        lhs.range(p),
    ));

    lhs.clone().precede(p).complete(p, R_BOGUS_SELECTOR)
}

/// identifier | string | `...` | `..i`
fn parse_selector(p: &mut RParser) -> ParsedSyntax {
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
        R_SINGLE_QUOTED_STRING_LITERAL | R_DOUBLE_QUOTED_STRING_LITERAL | R_RAW_STRING_LITERAL => {
            let m = p.start();
            p.bump_any();
            Present(m.complete(p, R_STRING_VALUE))
        }
        _ => Absent,
    }
}

/// Postfix: `f(x)`, `x[i]`, `x[[i]]`
fn parse_postfix_expression(p: &mut RParser, lhs: &CompletedMarker) -> ParsedSyntax {
    match p.cur() {
        T!['('] => {
            let m = lhs.clone().precede(p);
            parse_call_arguments(p);
            Present(m.complete(p, R_CALL))
        }
        T!['['] => {
            let m = lhs.clone().precede(p);
            parse_subset_arguments(p);
            Present(m.complete(p, R_SUBSET))
        }
        T!["[["] => {
            let m = lhs.clone().precede(p);
            parse_subset2_arguments(p);
            Present(m.complete(p, R_SUBSET2))
        }
        _ => Absent,
    }
}
