use crate::parser::{RParser, Scope};
use crate::syntax::expr::parse_expression;
use crate::syntax::value::parse_string_value;
use aether_r_syntax::RSyntaxKind::{
    self, DOTDOTI, DOTS, IDENT, R_ARGUMENT, R_ARGUMENT_LIST, R_ARGUMENT_NAME_CLAUSE,
    R_BOGUS_EXPRESSION, R_CALL_ARGUMENTS, R_DOT_DOT_I, R_DOTS, R_DOUBLE_QUOTED_STRING_LITERAL,
    R_IDENTIFIER, R_NULL_EXPRESSION, R_RAW_STRING_LITERAL, R_SINGLE_QUOTED_STRING_LITERAL,
    R_SUBSET_ARGUMENTS, R_SUBSET2_ARGUMENTS,
};
use aether_r_syntax::T;
use biome_parser::parse_recovery::ParseRecoveryTokenSet;
use biome_parser::{Parser, ParserProgress, TokenSet, token_set};

pub(crate) fn parse_call_arguments(p: &mut RParser) {
    let m = p.start();
    p.with_scope(Scope::Parenthesis, |p| {
        p.bump(T!['(']);
        parse_argument_list(p);
    });
    p.expect(T![')']);
    m.complete(p, R_CALL_ARGUMENTS);
}

pub(crate) fn parse_subset_arguments(p: &mut RParser) {
    let m = p.start();
    p.with_scope(Scope::Bracket, |p| {
        p.bump(T!['[']);
        parse_argument_list(p);
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
        parse_argument_list(p);
    });
    // Similarly, we `expect()` (bump + assert) outside the scope so that a
    // potential future `]` can't be affected by a now closed Bracket2 scope
    p.expect(T!["]]"]);
    m.complete(p, R_SUBSET2_ARGUMENTS);
}

/// What ends an argument list
///
/// Any closing delimiter, not just the one the caller opened. We don't consume
/// it, we return from here. The caller's `expect()` on the closing delimiter
/// reports a diagnostic, which still doesn't consume, and the delimiter
/// survives (ideally to be consumed by its actual owner).
const ARGUMENT_LIST_ENDS: TokenSet<RSyntaxKind> =
    token_set![T![EOF], T![')'], T![']'], T!["]]"], T!['}']];

/// Where an argument may stop: at the `,` that starts the next one, or at an end
const ARGUMENT_LIST_BOUNDARIES: TokenSet<RSyntaxKind> = ARGUMENT_LIST_ENDS.union(token_set![T![,]]);

fn parse_argument_list(p: &mut RParser) {
    let argument_list = p.start();
    let mut progress = ParserProgress::default();

    // Handle empty list before looping
    if !p.at_ts(ARGUMENT_LIST_ENDS) {
        loop {
            progress.assert_progressing(p);
            parse_argument(p);

            if !p.at_ts(ARGUMENT_LIST_BOUNDARIES) {
                // We know there is a missing separator, so here's where we get
                // to report that. Since the comma is missing, this doesn't
                // bump, it just reports a diagnostic and we continue sitting
                // where we are.
                p.expect(T![,]);

                let argument = p.start();
                // Opens a marker, bumps repeatedly until we hit a boundary,
                // closes as R_BOGUS_EXPRESSION
                let recovered =
                    ParseRecoveryTokenSet::new(R_BOGUS_EXPRESSION, ARGUMENT_LIST_BOUNDARIES)
                        .recover(p);
                debug_assert!(
                    recovered.is_ok(),
                    "not at a boundary, so not at `EOF` either"
                );
                argument.complete(p, R_ARGUMENT);
            }

            // We are at a boundary, either because the argument ended at one or
            // because we just recovered to one. A comma means another argument
            // follows, so consume it and loop. Anything else ends the list, and
            // we leave it where it is.
            if !p.at(T![,]) {
                break;
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

    // Optional name clause
    parse_argument_name_clause(p);

    // Optional value (hence the `ok()`)
    parse_expression(p).ok();

    m.complete(p, R_ARGUMENT);
}

/// `name =`, where the `name` is an identifier, a string, `...`, `..i`, or `NULL`
///
/// `foo` and `foo =` start the same way, so one token of lookahead is what tells
/// a name from the head of a value
///
/// `NULL` is accepted here and nowhere else, and has been since the
/// beginning of R (r-lib/tree-sitter-r#164).
fn parse_argument_name_clause(p: &mut RParser) {
    match p.cur() {
        IDENT if p.nth_at(1, T![=]) => {
            let clause = p.start();
            let name = p.start();
            p.bump_any();
            name.complete(p, R_IDENTIFIER);
            p.bump(T![=]);
            clause.complete(p, R_ARGUMENT_NAME_CLAUSE);
        }
        DOTS if p.nth_at(1, T![=]) => {
            let clause = p.start();
            let name = p.start();
            p.bump_any();
            name.complete(p, R_DOTS);
            p.bump(T![=]);
            clause.complete(p, R_ARGUMENT_NAME_CLAUSE);
        }
        DOTDOTI if p.nth_at(1, T![=]) => {
            let clause = p.start();
            let name = p.start();
            p.bump_any();
            name.complete(p, R_DOT_DOT_I);
            p.bump(T![=]);
            clause.complete(p, R_ARGUMENT_NAME_CLAUSE);
        }
        T![null] if p.nth_at(1, T![=]) => {
            let clause = p.start();
            let name = p.start();
            p.bump_any();
            name.complete(p, R_NULL_EXPRESSION);
            p.bump(T![=]);
            clause.complete(p, R_ARGUMENT_NAME_CLAUSE);
        }
        R_SINGLE_QUOTED_STRING_LITERAL | R_DOUBLE_QUOTED_STRING_LITERAL | R_RAW_STRING_LITERAL
            if p.nth_at(1, T![=]) =>
        {
            let clause = p.start();
            parse_string_value(p).ok();
            p.bump(T![=]);
            clause.complete(p, R_ARGUMENT_NAME_CLAUSE);
        }
        _ => {}
    }
}
