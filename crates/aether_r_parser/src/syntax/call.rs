use crate::lexer::RReLexContext;
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
    p.bump(T!['(']);
    p.with_scope(Scope::Parenthesis, |p| {
        parse_argument_list(p, ArgumentListEnd::Parenthesis);
    });
    p.expect(T![')']);
    m.complete(p, R_CALL_ARGUMENTS);
}

pub(crate) fn parse_subset_arguments(p: &mut RParser) {
    let m = p.start();
    p.bump(T!['[']);
    p.with_scope(Scope::Bracket, |p| {
        parse_argument_list(p, ArgumentListEnd::Bracket);
    });
    p.expect(T![']']);
    m.complete(p, R_SUBSET_ARGUMENTS);
}

pub(crate) fn parse_subset2_arguments(p: &mut RParser) {
    let m = p.start();
    p.bump(T!["[["]);
    p.with_scope(Scope::Bracket2, |p| {
        parse_argument_list(p, ArgumentListEnd::Bracket2);
    });
    // The list joined its `]]` on the way out, so this is a plain expect
    p.expect(T!["]]"]);
    m.complete(p, R_SUBSET2_ARGUMENTS);
}

/// What closes an argument list
#[derive(Debug, Clone, Copy)]
enum ArgumentListEnd {
    Parenthesis,
    Bracket,
    Bracket2,
}

impl ArgumentListEnd {
    /// Whether the current token closes this list
    fn at(self, p: &mut RParser) -> bool {
        match self {
            Self::Parenthesis => p.at(T![')']),
            Self::Bracket => p.at(T![']']),
            Self::Bracket2 => {
                // Only if adjacent, so `x[[a] ]` and `x[[a]\n]` stay errors
                if p.at(T![']']) {
                    p.re_lex(RReLexContext::JoinBrackets);
                }
                p.at(T!["]]"])
            }
        }
    }

    /// Tokens a failed argument may resynchronise on
    ///
    /// Uniform across all three variants, and deliberately wider than this
    /// list's own terminator: eating a closing delimiter of any kind would strip
    /// an enclosing construct of its terminator and turn one error into a
    /// cascade.
    ///
    /// `]]` is absent because [ArgumentListEnd::at()] is the only thing that
    /// produces one, and only where the list is about to end.
    fn recovery_set(self) -> TokenSet<RSyntaxKind> {
        token_set![T![,], T![')'], T![']'], T!['}'], T![EOF]]
    }
}

fn parse_argument_list(p: &mut RParser, end: ArgumentListEnd) {
    let m = p.start();
    let mut progress = ParserProgress::default();

    // An empty list has no arguments at all, where `f(,)` has two: a comma
    // always has an argument on either side of it
    if !end.at(p) && !p.at(T![EOF]) {
        loop {
            progress.assert_progressing(p);
            parse_argument(p);

            if end.at(p) || p.at(T![EOF]) {
                break;
            }

            if !p.at(T![,]) {
                // A closing delimiter that belongs to someone else
                if p.at_ts(end.recovery_set()) {
                    break;
                }

                // `f(a b)`. Reported before recovering, so the range covers the
                // tokens about to be buried in the bogus node.
                p.expect(T![,]);

                let argument = p.start();
                let recovered =
                    ParseRecoveryTokenSet::new(R_BOGUS_EXPRESSION, end.recovery_set()).recover(p);

                if recovered.is_err() {
                    argument.abandon(p);
                    break;
                }
                argument.complete(p, R_ARGUMENT);

                if !p.at(T![,]) {
                    break;
                }
            }

            p.bump(T![,]);
        }
    }

    m.complete(p, R_ARGUMENT_LIST);
}

/// One argument: `[name =] [value]`, both halves optional
///
/// An empty argument opens and immediately closes its marker, pushing no
/// children. That is the technique `r.ungram` documents, and it is what makes
/// `f(,,a,,b,,)` representable.
///
/// The name and the head of a value are the same tokens, so the choice between
/// them cannot be made before parsing — but it needs no rewind either. Parse the
/// atom, then look at what follows. This is sound because R admits only a bare
/// symbol, string, `...`, `..i`, or `NULL` as a name, so
/// [parse_argument_name()] can never consume more than the value would have.
fn parse_argument(p: &mut RParser) {
    let m = p.start();

    match parse_argument_name(p) {
        Present(name) if p.at(T![=]) => {
            let clause = name.precede(p);
            p.bump(T![=]);
            clause.complete(p, R_ARGUMENT_NAME_CLAUSE);
            parse_expression(p).ok();
        }
        // Not a name clause after all, so the atom was the head of the value
        Present(atom) => {
            parse_expression_with_lhs(p, atom, BindingPower::LOWEST);
        }
        Absent => {
            parse_expression(p).ok();
        }
    }

    m.complete(p, R_ARGUMENT);
}

/// identifier | string | `...` | `..i` | `NULL`
///
/// Every one of these is also an expression, which is what lets
/// [parse_argument()] hand the atom straight to the Pratt loop when no `=`
/// follows it. `NULL` is accepted here and nowhere else, and has been since the
/// beginning of R — see r-lib/tree-sitter-r#164.
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
