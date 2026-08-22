//! Pratt parser core
//!
//! https://matklad.github.io/2020/04/13/simple-but-powerful-pratt-parsing.html
//! https://kevinushey.github.io/blog/2016/02/12/top-down-operator-precedence-parsing-with-r/

use crate::diagnostics;
use crate::parser::RParser;
use crate::syntax::call::{parse_call_arguments, parse_subset_arguments, parse_subset2_arguments};
use crate::syntax::parenthesized_expression::parse_parenthesized_expression;
use aether_r_syntax::RSyntaxKind::*;
use aether_r_syntax::{AnyRSelector, T};
use biome_parser::CompletedMarker;
use biome_parser::Parser;
use biome_parser::parsed_syntax::ParsedSyntax::{self, Absent, Present};
use biome_rowan::AstNode;

/// How tightly an operator binds. Higher wins!
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
struct BindingPower(u8);

impl BindingPower {
    const LOWEST: Self = Self(0);

    const HIGHEST: Self = Self(16);
}

#[derive(Debug, Clone, Copy)]
enum Associativity {
    Left,
    Right,
}

// Taken from R's `gram.y`
const BINARY_HELP_BINDING_POWER: BindingPower = BindingPower(1);
const UNARY_HELP_BINDING_POWER: BindingPower = BindingPower(1);
const BINARY_LEFT_ASSIGN_BINDING_POWER: BindingPower = BindingPower(2);
const BINARY_EQUALS_ASSIGN_BINDING_POWER: BindingPower = BindingPower(3);
const BINARY_RIGHT_ASSIGN_BINDING_POWER: BindingPower = BindingPower(4);
const BINARY_TILDE_BINDING_POWER: BindingPower = BindingPower(5);
const UNARY_TILDE_BINDING_POWER: BindingPower = BindingPower(5);
const BINARY_OR_BINDING_POWER: BindingPower = BindingPower(6);
const BINARY_AND_BINDING_POWER: BindingPower = BindingPower(7);
const UNARY_NOT_BINDING_POWER: BindingPower = BindingPower(8);
const BINARY_COMPARISON_BINDING_POWER: BindingPower = BindingPower(9);
const BINARY_PLUS_MINUS_BINDING_POWER: BindingPower = BindingPower(10);
const BINARY_MULTIPLY_DIVIDE_BINDING_POWER: BindingPower = BindingPower(11);
const BINARY_SPECIAL_OR_PIPE_BINDING_POWER: BindingPower = BindingPower(12);
const BINARY_COLON_BINDING_POWER: BindingPower = BindingPower(13);
const UNARY_PLUS_MINUS_BINDING_POWER: BindingPower = BindingPower(14);
const BINARY_EXPONENTIATE_BINDING_POWER: BindingPower = BindingPower(15);

const BINARY_HELP_ASSOCIATIVITY: Associativity = Associativity::Left;
const BINARY_LEFT_ASSIGN_ASSOCIATIVITY: Associativity = Associativity::Right;
const BINARY_EQUALS_ASSIGN_ASSOCIATIVITY: Associativity = Associativity::Right;
const BINARY_RIGHT_ASSIGN_ASSOCIATIVITY: Associativity = Associativity::Left;
const BINARY_TILDE_ASSOCIATIVITY: Associativity = Associativity::Left;
const BINARY_OR_ASSOCIATIVITY: Associativity = Associativity::Left;
const BINARY_AND_ASSOCIATIVITY: Associativity = Associativity::Left;
const BINARY_COMPARISON_ASSOCIATIVITY: Associativity = Associativity::Left;
const BINARY_PLUS_MINUS_ASSOCIATIVITY: Associativity = Associativity::Left;
const BINARY_MULTIPLY_DIVIDE_ASSOCIATIVITY: Associativity = Associativity::Left;
const BINARY_SPECIAL_OR_PIPE_ASSOCIATIVITY: Associativity = Associativity::Left;
const BINARY_COLON_ASSOCIATIVITY: Associativity = Associativity::Left;
const BINARY_EXPONENTIATE_ASSOCIATIVITY: Associativity = Associativity::Right;

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
        "{current_binding_power:?} must not exceed {:?}.",
        BindingPower::HIGHEST
    );

    let mut lhs = match p.cur() {
        T![?] => parse_unary_expression(p, UNARY_HELP_BINDING_POWER),
        T![~] => parse_unary_expression(p, UNARY_TILDE_BINDING_POWER),
        T![!] => parse_unary_expression(p, UNARY_NOT_BINDING_POWER),
        T![+] | T![-] => parse_unary_expression(p, UNARY_PLUS_MINUS_BINDING_POWER),
        R_INTEGER_LITERAL => {
            let m = p.start();
            p.bump_any();
            m.complete(p, R_INTEGER_VALUE)
        }
        R_DOUBLE_LITERAL => {
            let m = p.start();
            p.bump_any();
            m.complete(p, R_DOUBLE_VALUE)
        }
        R_COMPLEX_LITERAL => {
            let m = p.start();
            p.bump_any();
            m.complete(p, R_COMPLEX_VALUE)
        }
        IDENT => {
            let m = p.start();
            p.bump_any();
            m.complete(p, R_IDENTIFIER)
        }
        DOTS => {
            let m = p.start();
            p.bump_any();
            m.complete(p, R_DOTS)
        }
        DOTDOTI => {
            let m = p.start();
            p.bump_any();
            m.complete(p, R_DOT_DOT_I)
        }
        T![next] => {
            let m = p.start();
            p.bump_any();
            m.complete(p, R_NEXT_EXPRESSION)
        }
        T![break] => {
            let m = p.start();
            p.bump_any();
            m.complete(p, R_BREAK_EXPRESSION)
        }
        T![true] => {
            let m = p.start();
            p.bump_any();
            m.complete(p, R_TRUE_EXPRESSION)
        }
        T![false] => {
            let m = p.start();
            p.bump_any();
            m.complete(p, R_FALSE_EXPRESSION)
        }
        T![null] => {
            let m = p.start();
            p.bump_any();
            m.complete(p, R_NULL_EXPRESSION)
        }
        T![inf] => {
            let m = p.start();
            p.bump_any();
            m.complete(p, R_INF_EXPRESSION)
        }
        T![nan] => {
            let m = p.start();
            p.bump_any();
            m.complete(p, R_NAN_EXPRESSION)
        }
        T![na_logical] | T![na_integer] | T![na_double] | T![na_complex] | T![na_character] => {
            let m = p.start();
            p.bump_any();
            m.complete(p, R_NA_EXPRESSION)
        }
        ERROR_TOKEN => {
            // Lexer already raised `unexpected_character()`
            let m = p.start();
            p.bump_any();
            m.complete(p, R_BOGUS_EXPRESSION)
        }
        R_SINGLE_QUOTED_STRING_LITERAL | R_DOUBLE_QUOTED_STRING_LITERAL | R_RAW_STRING_LITERAL => {
            let m = p.start();
            p.bump_any();
            m.complete(p, R_STRING_VALUE)
        }
        T!['('] => parse_parenthesized_expression(p),
        // Doesn't start an expression
        _ => return Absent,
    };

    loop {
        if p.scope().newlines_are_significant() && p.has_preceding_line_break() {
            break;
        }

        let new_lhs = match p.cur() {
            T!['('] => parse_call_expression(p, &lhs),
            T!['['] => parse_subset_expression(p, &lhs),
            T!["[["] => parse_subset2_expression(p, &lhs),
            T![$] | T![@] => parse_extract_expression(p, &lhs),
            T![::] | T![:::] => parse_namespace_expression(p, &lhs),
            T![?] => parse_binary_expression(
                p,
                &lhs,
                current_binding_power,
                BINARY_HELP_BINDING_POWER,
                BINARY_HELP_ASSOCIATIVITY,
            ),
            T![<-] | T![<<-] | T![:=] => parse_binary_expression(
                p,
                &lhs,
                current_binding_power,
                BINARY_LEFT_ASSIGN_BINDING_POWER,
                BINARY_LEFT_ASSIGN_ASSOCIATIVITY,
            ),
            T![=] => parse_binary_expression(
                p,
                &lhs,
                current_binding_power,
                BINARY_EQUALS_ASSIGN_BINDING_POWER,
                BINARY_EQUALS_ASSIGN_ASSOCIATIVITY,
            ),
            T![->] | T![->>] => parse_binary_expression(
                p,
                &lhs,
                current_binding_power,
                BINARY_RIGHT_ASSIGN_BINDING_POWER,
                BINARY_RIGHT_ASSIGN_ASSOCIATIVITY,
            ),
            T![~] => parse_binary_expression(
                p,
                &lhs,
                current_binding_power,
                BINARY_TILDE_BINDING_POWER,
                BINARY_TILDE_ASSOCIATIVITY,
            ),
            T![|] | T![||] => parse_binary_expression(
                p,
                &lhs,
                current_binding_power,
                BINARY_OR_BINDING_POWER,
                BINARY_OR_ASSOCIATIVITY,
            ),
            T![&] | T![&&] => parse_binary_expression(
                p,
                &lhs,
                current_binding_power,
                BINARY_AND_BINDING_POWER,
                BINARY_AND_ASSOCIATIVITY,
            ),
            T![<] | T![<=] | T![>] | T![>=] | T![==] | T![!=] => parse_binary_expression(
                p,
                &lhs,
                current_binding_power,
                BINARY_COMPARISON_BINDING_POWER,
                BINARY_COMPARISON_ASSOCIATIVITY,
            ),
            T![+] | T![-] => parse_binary_expression(
                p,
                &lhs,
                current_binding_power,
                BINARY_PLUS_MINUS_BINDING_POWER,
                BINARY_PLUS_MINUS_ASSOCIATIVITY,
            ),
            T![*] | T![/] => parse_binary_expression(
                p,
                &lhs,
                current_binding_power,
                BINARY_MULTIPLY_DIVIDE_BINDING_POWER,
                BINARY_MULTIPLY_DIVIDE_ASSOCIATIVITY,
            ),
            SPECIAL | T![|>] => parse_binary_expression(
                p,
                &lhs,
                current_binding_power,
                BINARY_SPECIAL_OR_PIPE_BINDING_POWER,
                BINARY_SPECIAL_OR_PIPE_ASSOCIATIVITY,
            ),
            T![:] => parse_binary_expression(
                p,
                &lhs,
                current_binding_power,
                BINARY_COLON_BINDING_POWER,
                BINARY_COLON_ASSOCIATIVITY,
            ),
            T![^] | T![**] => parse_binary_expression(
                p,
                &lhs,
                current_binding_power,
                BINARY_EXPONENTIATE_BINDING_POWER,
                BINARY_EXPONENTIATE_ASSOCIATIVITY,
            ),
            _ => Absent,
        };

        match new_lhs {
            Present(new_lhs) => {
                lhs = new_lhs;
                continue;
            }
            Absent => break,
        }
    }

    Present(lhs)
}

fn parse_unary_expression(p: &mut RParser, binding_power: BindingPower) -> CompletedMarker {
    let m = p.start();
    p.bump_any();
    parse_expression_with_binding_power(p, binding_power)
        .or_add_diagnostic(p, diagnostics::expected_expression);
    m.complete(p, R_UNARY_EXPRESSION)
}

/// Infix: `a + b`, `a <- b`, `a %in% b`
fn parse_binary_expression(
    p: &mut RParser,
    lhs: &CompletedMarker,
    current_binding_power: BindingPower,
    binding_power: BindingPower,
    associativity: Associativity,
) -> ParsedSyntax {
    if binding_power < current_binding_power {
        // New operator binds too loosely
        return Absent;
    }
    let binding_power = match associativity {
        Associativity::Left => BindingPower(binding_power.0 + 1),
        Associativity::Right => binding_power,
    };

    let m = lhs.clone().precede(p);
    p.bump_any();
    parse_expression_with_binding_power(p, binding_power)
        .or_add_diagnostic(p, diagnostics::expected_expression);
    Present(m.complete(p, R_BINARY_EXPRESSION))
}

fn parse_call_expression(p: &mut RParser, lhs: &CompletedMarker) -> ParsedSyntax {
    let m = lhs.clone().precede(p);
    parse_call_arguments(p);
    Present(m.complete(p, R_CALL))
}

fn parse_subset_expression(p: &mut RParser, lhs: &CompletedMarker) -> ParsedSyntax {
    let m = lhs.clone().precede(p);
    parse_subset_arguments(p);
    Present(m.complete(p, R_SUBSET))
}

fn parse_subset2_expression(p: &mut RParser, lhs: &CompletedMarker) -> ParsedSyntax {
    let m = lhs.clone().precede(p);
    parse_subset2_arguments(p);
    Present(m.complete(p, R_SUBSET2))
}

/// Infix: `a$b`, `a@b`
fn parse_extract_expression(p: &mut RParser, lhs: &CompletedMarker) -> ParsedSyntax {
    let operator = p.cur();
    let operator_range = p.cur_range();

    let m = lhs.clone().precede(p);
    p.bump_any();
    parse_selector(p).or_add_diagnostic(p, |p, range| {
        diagnostics::expected_rhs_selector(p, operator, operator_range, range)
    });
    Present(m.complete(p, R_EXTRACT_EXPRESSION))
}

/// Infix: `a::b`, `a:::b`
fn parse_namespace_expression(p: &mut RParser, lhs: &CompletedMarker) -> ParsedSyntax {
    let operator = p.cur();
    let operator_range = p.cur_range();

    // `::` and `:::` restrict their left operand as well, and it is already parsed
    let mut lhs = lhs.clone();
    if !AnyRSelector::can_cast(lhs.kind(p)) {
        p.error(diagnostics::expected_lhs_selector(
            p,
            operator,
            operator_range,
            lhs.range(p),
        ));
        lhs = lhs.precede(p).complete(p, R_BOGUS_SELECTOR);
    }

    let m = lhs.precede(p);
    p.bump_any();

    if p.scope().newlines_are_significant() && p.has_preceding_line_break() {
        p.error(diagnostics::selector_after_line_break(
            operator,
            operator_range,
        ));
    } else {
        parse_selector(p).or_add_diagnostic(p, |p, range| {
            diagnostics::expected_rhs_selector(p, operator, operator_range, range)
        });
    }

    Present(m.complete(p, R_NAMESPACE_EXPRESSION))
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
