//! Pratt parser core
//!
//! https://matklad.github.io/2020/04/13/simple-but-powerful-pratt-parsing.html
//! https://kevinushey.github.io/blog/2016/02/12/top-down-operator-precedence-parsing-with-r/

use crate::diagnostics;
use crate::parser::RParser;
use crate::syntax::call::{
    parse_call_expression, parse_subset_expression, parse_subset2_expression,
};
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

    // Taken from R's `gram.y`
    const BINARY_HELP: Self = Self(1);
    const UNARY_HELP: Self = Self(1);
    const BINARY_LEFT_ASSIGN: Self = Self(2);
    const BINARY_EQUALS_ASSIGN: Self = Self(3);
    const BINARY_RIGHT_ASSIGN: Self = Self(4);
    const BINARY_TILDE: Self = Self(5);
    const UNARY_TILDE: Self = Self(5);
    const BINARY_OR: Self = Self(6);
    const BINARY_AND: Self = Self(7);
    const UNARY_NOT: Self = Self(8);
    const BINARY_COMPARISON: Self = Self(9);
    const BINARY_PLUS_MINUS: Self = Self(10);
    const BINARY_MULTIPLY_DIVIDE: Self = Self(11);
    const BINARY_SPECIAL_OR_PIPE: Self = Self(12);
    const BINARY_COLON: Self = Self(13);
    const UNARY_PLUS_MINUS: Self = Self(14);
    const BINARY_EXPONENTIATE: Self = Self(15);

    const HIGHEST: Self = Self(16);
}

#[derive(Debug, Clone, Copy)]
enum Associativity {
    Left,
    Right,
}

impl Associativity {
    const BINARY_HELP: Self = Self::Left;
    const BINARY_LEFT_ASSIGN: Self = Self::Right;
    const BINARY_EQUALS_ASSIGN: Self = Self::Right;
    const BINARY_RIGHT_ASSIGN: Self = Self::Left;
    const BINARY_TILDE: Self = Self::Left;
    const BINARY_OR: Self = Self::Left;
    const BINARY_AND: Self = Self::Left;
    const BINARY_COMPARISON: Self = Self::Left;
    const BINARY_PLUS_MINUS: Self = Self::Left;
    const BINARY_MULTIPLY_DIVIDE: Self = Self::Left;
    const BINARY_SPECIAL_OR_PIPE: Self = Self::Left;
    const BINARY_COLON: Self = Self::Left;
    const BINARY_EXPONENTIATE: Self = Self::Right;
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
        "{current_binding_power:?} must not exceed {:?}.",
        BindingPower::HIGHEST
    );

    let mut lhs = match p.cur() {
        T![?] => parse_unary_expression(p, BindingPower::UNARY_HELP),
        T![~] => parse_unary_expression(p, BindingPower::UNARY_TILDE),
        T![!] => parse_unary_expression(p, BindingPower::UNARY_NOT),
        T![+] | T![-] => parse_unary_expression(p, BindingPower::UNARY_PLUS_MINUS),
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

        lhs = match p.cur() {
            T!['('] => parse_call_expression(p, lhs),
            T!['['] => parse_subset_expression(p, lhs),
            T!["[["] => parse_subset2_expression(p, lhs),
            T![$] | T![@] => parse_extract_expression(p, lhs),
            T![::] | T![:::] => parse_namespace_expression(p, lhs),
            T![?] if BindingPower::BINARY_HELP >= current_binding_power => parse_binary_expression(
                p,
                lhs,
                BindingPower::BINARY_HELP,
                Associativity::BINARY_HELP,
            ),
            T![<-] | T![<<-] | T![:=]
                if BindingPower::BINARY_LEFT_ASSIGN >= current_binding_power =>
            {
                parse_binary_expression(
                    p,
                    lhs,
                    BindingPower::BINARY_LEFT_ASSIGN,
                    Associativity::BINARY_LEFT_ASSIGN,
                )
            }
            T![=] if BindingPower::BINARY_EQUALS_ASSIGN >= current_binding_power => {
                parse_binary_expression(
                    p,
                    lhs,
                    BindingPower::BINARY_EQUALS_ASSIGN,
                    Associativity::BINARY_EQUALS_ASSIGN,
                )
            }
            T![->] | T![->>] if BindingPower::BINARY_RIGHT_ASSIGN >= current_binding_power => {
                parse_binary_expression(
                    p,
                    lhs,
                    BindingPower::BINARY_RIGHT_ASSIGN,
                    Associativity::BINARY_RIGHT_ASSIGN,
                )
            }
            T![~] if BindingPower::BINARY_TILDE >= current_binding_power => {
                parse_binary_expression(
                    p,
                    lhs,
                    BindingPower::BINARY_TILDE,
                    Associativity::BINARY_TILDE,
                )
            }
            T![|] | T![||] if BindingPower::BINARY_OR >= current_binding_power => {
                parse_binary_expression(p, lhs, BindingPower::BINARY_OR, Associativity::BINARY_OR)
            }
            T![&] | T![&&] if BindingPower::BINARY_AND >= current_binding_power => {
                parse_binary_expression(p, lhs, BindingPower::BINARY_AND, Associativity::BINARY_AND)
            }
            T![<] | T![<=] | T![>] | T![>=] | T![==] | T![!=]
                if BindingPower::BINARY_COMPARISON >= current_binding_power =>
            {
                parse_binary_expression(
                    p,
                    lhs,
                    BindingPower::BINARY_COMPARISON,
                    Associativity::BINARY_COMPARISON,
                )
            }
            T![+] | T![-] if BindingPower::BINARY_PLUS_MINUS >= current_binding_power => {
                parse_binary_expression(
                    p,
                    lhs,
                    BindingPower::BINARY_PLUS_MINUS,
                    Associativity::BINARY_PLUS_MINUS,
                )
            }
            T![*] | T![/] if BindingPower::BINARY_MULTIPLY_DIVIDE >= current_binding_power => {
                parse_binary_expression(
                    p,
                    lhs,
                    BindingPower::BINARY_MULTIPLY_DIVIDE,
                    Associativity::BINARY_MULTIPLY_DIVIDE,
                )
            }
            SPECIAL | T![|>] if BindingPower::BINARY_SPECIAL_OR_PIPE >= current_binding_power => {
                parse_binary_expression(
                    p,
                    lhs,
                    BindingPower::BINARY_SPECIAL_OR_PIPE,
                    Associativity::BINARY_SPECIAL_OR_PIPE,
                )
            }
            T![:] if BindingPower::BINARY_COLON >= current_binding_power => {
                parse_binary_expression(
                    p,
                    lhs,
                    BindingPower::BINARY_COLON,
                    Associativity::BINARY_COLON,
                )
            }
            T![^] | T![**] if BindingPower::BINARY_EXPONENTIATE >= current_binding_power => {
                parse_binary_expression(
                    p,
                    lhs,
                    BindingPower::BINARY_EXPONENTIATE,
                    Associativity::BINARY_EXPONENTIATE,
                )
            }
            _ => break,
        };
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
    lhs: CompletedMarker,
    binding_power: BindingPower,
    associativity: Associativity,
) -> CompletedMarker {
    let binding_power = match associativity {
        Associativity::Left => BindingPower(binding_power.0 + 1),
        Associativity::Right => binding_power,
    };

    let m = lhs.precede(p);
    p.bump_any();
    parse_expression_with_binding_power(p, binding_power)
        .or_add_diagnostic(p, diagnostics::expected_expression);
    m.complete(p, R_BINARY_EXPRESSION)
}

/// Infix: `a$b`, `a@b`
fn parse_extract_expression(p: &mut RParser, lhs: CompletedMarker) -> CompletedMarker {
    let operator = p.cur();
    let operator_range = p.cur_range();

    let m = lhs.precede(p);
    p.bump_any();
    parse_selector(p).or_add_diagnostic(p, |_, range| {
        diagnostics::expected_rhs_selector(operator, operator_range, range)
    });
    m.complete(p, R_EXTRACT_EXPRESSION)
}

/// Infix: `a::b`, `a:::b`
fn parse_namespace_expression(p: &mut RParser, mut lhs: CompletedMarker) -> CompletedMarker {
    let operator = p.cur();
    let operator_range = p.cur_range();

    // `::` and `:::` restrict their left operand as well, and it is already parsed
    if !AnyRSelector::can_cast(lhs.kind(p)) {
        p.error(diagnostics::expected_lhs_selector(
            operator,
            operator_range,
            lhs.range(p),
        ));
        lhs = lhs.precede(p).complete(p, R_BOGUS_SELECTOR);
    }

    let m = lhs.precede(p);
    p.bump_any();

    if p.scope().newlines_are_significant() && p.has_preceding_line_break() {
        p.error(diagnostics::namespace_rhs_selector_after_line_break(
            operator,
            operator_range,
        ));
    } else {
        parse_selector(p).or_add_diagnostic(p, |_, range| {
            diagnostics::expected_rhs_selector(operator, operator_range, range)
        });
    }

    m.complete(p, R_NAMESPACE_EXPRESSION)
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
