use crate::parser::{RParser, Scope};
use crate::syntax::expr::{BindingPower, parse_expression, parse_expression_with_lhs};
use crate::syntax::value::parse_string_value;
use aether_r_syntax::RSyntaxKind::{
    self, DOTDOTI, DOTS, IDENT, R_ARGUMENT, R_ARGUMENT_LIST, R_ARGUMENT_NAME_CLAUSE,
    R_BOGUS_EXPRESSION, R_CALL_ARGUMENTS, R_DOT_DOT_I, R_DOTS, R_IDENTIFIER, R_NULL_EXPRESSION,
    R_SUBSET_ARGUMENTS, R_SUBSET2_ARGUMENTS, STRING_OPEN,
};
use aether_r_syntax::T;
use biome_parser::parse_recovery::ParseRecoveryTokenSet;
use biome_parser::parsed_syntax::ParsedSyntax::{self, Absent, Present};
use biome_parser::{Parser, ParserProgress, TokenSet, token_set};

pub(crate) fn parse_call_arguments(p: &mut RParser) {
    let m = p.start();
    p.with_scope(Scope::Parenthesis, |p| {
        p.bump(T!['(']);
        parse_argument_list(p, T![')']);
    });
    p.expect(T![')']);
    m.complete(p, R_CALL_ARGUMENTS);
}

pub(crate) fn parse_subset_arguments(p: &mut RParser) {
    let m = p.start();
    p.with_scope(Scope::Bracket, |p| {
        p.bump(T!['[']);
        parse_argument_list(p, T![']']);
    });
    p.expect(T![']']);
    m.complete(p, R_SUBSET_ARGUMENTS);
}

pub(crate) fn parse_subset2_arguments(p: &mut RParser) {
    let m = p.start();
    p.with_scope(Scope::Bracket2, |p| {
        // Purposefully bump inside `with_scope()`. Bumping lexes the next
        // token after this one, and the context for how two back to back `]`
        // `]` are lexed depends on whether we are in a Bracket2 scope or not.
        // If we bump before `with_scope()` then an empty `x[[]]` parses
        // incorrectly!
        p.bump(T!["[["]);
        parse_argument_list(p, T!["]]"]);
    });
    // Similarly, we `expect()` (bump + assert) outside the scope so that a
    // potential future `]` can't be affected by a now closed Bracket2 scope
    p.expect(T!["]]"]);
    m.complete(p, R_SUBSET2_ARGUMENTS);
}

/// Tokens a failed argument may resynchronise on
///
/// Deliberately wider than any one list's terminator: eating a closing
/// delimiter of any kind would strip an enclosing construct of its terminator
/// and turn one error into a cascade. `EOF` is absent because `recover()` stops
/// there whether the set says so or not.
const ARGUMENT_LIST_RECOVERY_SET: TokenSet<RSyntaxKind> =
    token_set![T![,], T![')'], T![']'], T!["]]"], T!['}']];

fn parse_argument_list(p: &mut RParser, close: RSyntaxKind) {
    let argument_list = p.start();
    let mut progress = ParserProgress::default();

    // Handle empty list before looping
    if !p.at(close) && !p.at(T![EOF]) {
        loop {
            progress.assert_progressing(p);
            parse_argument(p);

            if p.at(close) || p.at(T![EOF]) {
                break;
            }

            if !p.at(T![,]) {
                // A closing delimiter that belongs to someone else
                if p.at_ts(ARGUMENT_LIST_RECOVERY_SET) {
                    break;
                }

                // `f(a b)`. Reported before recovering, because afterwards the
                // cursor sits on the terminator.
                p.expect(T![,]);

                let argument = p.start();
                let recovered =
                    ParseRecoveryTokenSet::new(R_BOGUS_EXPRESSION, ARGUMENT_LIST_RECOVERY_SET)
                        .recover(p);
                debug_assert!(
                    recovered.is_ok(),
                    "`EOF` and the recovery set are both excluded above"
                );
                argument.complete(p, R_ARGUMENT);

                if !p.at(T![,]) {
                    break;
                }
            }

            p.bump(T![,]);
        }
    }

    argument_list.complete(p, R_ARGUMENT_LIST);
}

/// One argument: `[name =] [value]`, both halves optional
///
/// An empty argument opens and immediately closes its marker, pushing no
/// children. That is the technique `r.ungram` documents, and it is what makes
/// `f(,,a,,b,,)` representable.
fn parse_argument(p: &mut RParser) {
    let m = p.start();

    // We can't tell if we have `foo` or `foo =` up front, so we parse `foo`
    // and check if we land at `=`. If so, we have a name clause, otherwise we
    // have an argument value and we loop back through `parse_expression()` but
    // with an already known LHS.
    match parse_argument_name(p) {
        Present(name) if p.at(T![=]) => {
            let clause = name.precede(p);
            p.bump(T![=]);
            clause.complete(p, R_ARGUMENT_NAME_CLAUSE);
            parse_expression(p).ok();
        }
        // Not a name clause after all, use as lhs instead
        Present(lhs) => {
            parse_expression_with_lhs(p, lhs, BindingPower::LOWEST);
        }
        Absent => {
            parse_expression(p).ok();
        }
    }

    m.complete(p, R_ARGUMENT);
}

/// identifier | string | `...` | `..i` | `NULL`
///
/// `NULL` is accepted here and nowhere else, and has been since the
/// beginning of R (r-lib/tree-sitter-r#164).
fn parse_argument_name(p: &mut RParser) -> ParsedSyntax {
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
        T![null] => {
            let m = p.start();
            p.bump_any();
            Present(m.complete(p, R_NULL_EXPRESSION))
        }
        STRING_OPEN => parse_string_value(p),
        _ => Absent,
    }
}
