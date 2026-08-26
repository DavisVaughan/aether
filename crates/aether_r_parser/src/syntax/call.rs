use crate::diagnostics;
use crate::parser::RParser;
use crate::parser::Scope;
use crate::syntax::expr::parse_expression;
use aether_r_syntax::RSyntaxKind;
use aether_r_syntax::RSyntaxKind::DOTDOTI;
use aether_r_syntax::RSyntaxKind::DOTS;
use aether_r_syntax::RSyntaxKind::IDENT;
use aether_r_syntax::RSyntaxKind::R_ARGUMENT;
use aether_r_syntax::RSyntaxKind::R_ARGUMENT_LIST;
use aether_r_syntax::RSyntaxKind::R_ARGUMENT_NAME_CLAUSE;
use aether_r_syntax::RSyntaxKind::R_BOGUS_EXPRESSION;
use aether_r_syntax::RSyntaxKind::R_CALL;
use aether_r_syntax::RSyntaxKind::R_CALL_ARGUMENTS;
use aether_r_syntax::RSyntaxKind::R_DOT_DOT_I;
use aether_r_syntax::RSyntaxKind::R_DOTS;
use aether_r_syntax::RSyntaxKind::R_DOUBLE_QUOTED_STRING_LITERAL;
use aether_r_syntax::RSyntaxKind::R_IDENTIFIER;
use aether_r_syntax::RSyntaxKind::R_NULL_EXPRESSION;
use aether_r_syntax::RSyntaxKind::R_RAW_STRING_LITERAL;
use aether_r_syntax::RSyntaxKind::R_SINGLE_QUOTED_STRING_LITERAL;
use aether_r_syntax::RSyntaxKind::R_STRING_VALUE;
use aether_r_syntax::RSyntaxKind::R_SUBSET;
use aether_r_syntax::RSyntaxKind::R_SUBSET_ARGUMENTS;
use aether_r_syntax::RSyntaxKind::R_SUBSET2;
use aether_r_syntax::RSyntaxKind::R_SUBSET2_ARGUMENTS;
use aether_r_syntax::T;
use biome_parser::CompletedMarker;
use biome_parser::Parser;
use biome_parser::ParserProgress;
use biome_parser::TokenSet;
use biome_parser::parse_recovery::ParseRecoveryTokenSet;
use biome_parser::token_set;

pub(crate) fn parse_call_expression(p: &mut RParser, lhs: CompletedMarker) -> CompletedMarker {
    let m = lhs.precede(p);

    let arguments = p.start();
    p.with_scope(Scope::Parenthesis, |p| {
        p.bump(T!['(']);
        parse_argument_list(p);
    });
    p.expect(T![')']);
    arguments.complete(p, R_CALL_ARGUMENTS);

    m.complete(p, R_CALL)
}

pub(crate) fn parse_subset_expression(p: &mut RParser, lhs: CompletedMarker) -> CompletedMarker {
    let m = lhs.precede(p);

    let arguments = p.start();
    p.with_scope(Scope::Bracket, |p| {
        p.bump(T!['[']);
        parse_argument_list(p);
    });
    p.expect(T![']']);
    arguments.complete(p, R_SUBSET_ARGUMENTS);

    m.complete(p, R_SUBSET)
}

pub(crate) fn parse_subset2_expression(p: &mut RParser, lhs: CompletedMarker) -> CompletedMarker {
    let m = lhs.precede(p);

    let arguments = p.start();
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
    arguments.complete(p, R_SUBSET2_ARGUMENTS);

    m.complete(p, R_SUBSET2)
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
    let mut first = true;

    while !p.at_ts(ARGUMENT_LIST_ENDS) {
        if first {
            first = false;
        } else {
            // Either a `,` that introduces the next argument, or a missing one
            // that we report without consuming and let the next argument start
            // where we already are
            p.expect(T![,]);
        }

        progress.assert_progressing(p);
        parse_argument(p);
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

    // Optional value
    parse_argument_value(p);

    m.complete(p, R_ARGUMENT);
}

/// Value can be missing as long as the next token is a boundary, like
/// `f(a,<here>)` where the next token is `)` or `f(a,<here>,)` where the next
/// token is `,`, but not with `f(* x)` where `* x` simply isn't an expression.
/// That should be a parse error!
fn parse_argument_value(p: &mut RParser) {
    if p.at_ts(ARGUMENT_LIST_BOUNDARIES) {
        return;
    }

    parse_expression(p)
        .or_recover_with_token_set(
            p,
            &ParseRecoveryTokenSet::new(R_BOGUS_EXPRESSION, ARGUMENT_LIST_BOUNDARIES),
            diagnostics::expected_expression,
        )
        .ok();
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
            p.bump(IDENT);
            name.complete(p, R_IDENTIFIER);
            p.bump(T![=]);
            clause.complete(p, R_ARGUMENT_NAME_CLAUSE);
        }
        DOTS if p.nth_at(1, T![=]) => {
            let clause = p.start();
            let name = p.start();
            p.bump(DOTS);
            name.complete(p, R_DOTS);
            p.bump(T![=]);
            clause.complete(p, R_ARGUMENT_NAME_CLAUSE);
        }
        DOTDOTI if p.nth_at(1, T![=]) => {
            let clause = p.start();
            let name = p.start();
            p.bump(DOTDOTI);
            name.complete(p, R_DOT_DOT_I);
            p.bump(T![=]);
            clause.complete(p, R_ARGUMENT_NAME_CLAUSE);
        }
        T![null] if p.nth_at(1, T![=]) => {
            let clause = p.start();
            let name = p.start();
            p.bump(T![null]);
            name.complete(p, R_NULL_EXPRESSION);
            p.bump(T![=]);
            clause.complete(p, R_ARGUMENT_NAME_CLAUSE);
        }
        kind @ (R_SINGLE_QUOTED_STRING_LITERAL
        | R_DOUBLE_QUOTED_STRING_LITERAL
        | R_RAW_STRING_LITERAL)
            if p.nth_at(1, T![=]) =>
        {
            let clause = p.start();
            let name = p.start();
            p.bump(kind);
            name.complete(p, R_STRING_VALUE);
            p.bump(T![=]);
            clause.complete(p, R_ARGUMENT_NAME_CLAUSE);
        }
        _ => {}
    }
}
