# String simplification

A follow-up to PR #12. Nothing here is implemented — this document is the whole
change, written so it can be picked up with no other context.

## Summary

Lex a string as **one token**, delimiters included. `STRING_OPEN`,
`STRING_CONTENT`, and `STRING_CLOSE` go away, and with them the lexer's string
contexts. The text between the delimiters becomes a method on `RStringValue` in
`aether_r_syntax`. While we are here, an unterminated string and an unterminated
backtick-quoted name become `ERROR_TOKEN` rather than a well-formed token.

Three pieces, separable but landing together unless the PR gets too big:

1. One-token strings — the bulk of the work.

2. `ERROR_TOKEN` for constructs that run off the end of the file. The backtick
   half is independent of everything else and could land first as a small
   commit.

3. The content-extraction API on `RStringValue`.

## Why

### 1. The lookahead footgun, which is the real prize

`BufferedLexer::nth_non_trivia()` lexes lookahead in `RLexContext::default()`
(`biome_parser/src/lexer.rs`, `LookaheadIterator::next()`), so `p.nth(n)` is
silently wrong past a `STRING_OPEN` — the string body comes back lexed as code.

That hazard is currently *managed* rather than removed. It is documented in
five places in `plans/aether-implementation-plan.md` (§8.1, §8.2.1, §9.1.1,
§11.3, and the notes near "LookaheadIterator::next()"), pinned by a dedicated
test (`a_lookahead_over_a_string_leaves_nothing_behind` in
`crates/aether_r_parser/src/lexer/tests/checkpoint.rs`), and the whole §8.2.1
invariant — "the lexer keeps no string state" — exists only to make the wrong
lookahead harmless rather than corrupting.

One-token strings delete the hazard outright. Nothing needs to keep it safe,
because there is no non-regular context left to be wrong about.

### 2. The optional-content slot disappears

`content: 'string_content'?` in `xtask/codegen/r.ungram` is optional for one
reason: an empty body emits no token at all, so `""` is two tokens where `"a"`
is three. That subtlety is currently explained in the lexer
(`lex_string_content()`), in `parse_string_value()`, in the plan, and in two
lexer tests. It stops existing.

### 3. Internal consistency, and Biome's

A backtick-quoted name is already one `IDENT` token, scanned to its close by the
lexer, with the lexer emitting `unterminated_backtick_identifier()`. Strings
being three tokens with a parser-side `expect(STRING_CLOSE)` is the odd one out
in aether's own lexer. `biome_js_parser`'s `JS_STRING_LITERAL` is one token
including its quotes, which is what `CLAUDE.md`'s "when in doubt, do what
`biome_js_parser` does" points at.

### Not a reason: `parse_argument()`

The TODO at `crates/aether_r_parser/src/syntax/call.rs` claims one-token strings
would let `parse_argument()` go back to simple lookahead. True, but it is not an
improvement, and **the optimistic parse must stay**.

`parse_argument_name()` followed by a check for `=` and a `precede()` is
single-pass, backtracks nothing, and is what `biome_js_parser` does for the same
ambiguity. Switching to `p.nth(1) == T![=]` would double-lex the head of every
argument in every call in the file to save one match arm. **Delete the TODO
comment, keep the code.**

## Decisions

### D1. One `RStringValue` node, three token kinds

```
RStringValue =
	value: (
		'r_single_quoted_string_literal'
		| 'r_double_quoted_string_literal'
		| 'r_raw_string_literal'
	)
```

Rejected: three node types (`RSingleQuotedStringValue`, `RDoubleQuotedStringValue`,
`RRawStringValue`) under an `AnyRStringValue` union. It carries the same
information but adds a union level inside `AnyRValue`, `AnyRSelector`, and
`AnyRArgumentName`, adds three generated formatter files, and breaks air's
`crates/air_r_syntax/src/call_ext.rs` pattern match on
`AnyRValue::RStringValue(_)`. The `match` on the kind does not disappear under
that design either — codegen just writes it inside `AnyRStringValue::cast()`
instead.

What three node types would buy that this does not: exhaustiveness. Adding a
fourth string form would break every consumer's `match` at compile time, where
here it slips past a `_` arm. R is not getting a fourth string form.

**Consequence to know before starting:** an alternation slot is
`TokenKind::Many` in the codegen, and `Field::method_name()` in
`xtask/codegen/src/kind_src.rs` names those accessors after the label with no
`_token` suffix. So this generates `RStringValue::value() -> SyntaxResult<SyntaxToken>`,
not `value_token()`, which is mildly asymmetric with `RDoubleValue::value_token()`
next door. That is the one decision here a reviewer might reasonably flip: a
single `R_STRING_LITERAL` kind restores `value_token()` and drops two kinds, at
the cost of turning the kind `match` in `inner_range()` (below) into a
first-byte sniff of the token text. Three kinds still wins on balance — the
lexer already knows which it lexed, `cst` cells become self-describing
(`R_RAW_STRING_LITERAL@0..8 "r\"(abc)\""`), and air's eventual quote-style
option gets a kind check rather than a text check.

Put the three kinds in the `literals:` list in `xtask/codegen/src/r_kinds_src.rs`,
next to `R_INTEGER_LITERAL` / `R_DOUBLE_LITERAL` / `R_COMPLEX_LITERAL`, not in
`tokens:`. That makes `is_literal()` true for them, which is correct and which
nothing currently reads — `is_literal()` has no call sites in aether or in air.

Watch the name collision hazard while editing: `R_DOUBLE_LITERAL` is a *number*,
`R_DOUBLE_QUOTED_STRING_LITERAL` is a string.

### D2. Content extraction returns `TokenText`, plus a `TextRange` sibling

Not `&str` — `value()` hands back an owned `SyntaxToken`, so a `&str` borrowed
from it cannot outlive the method. Not `String` — that allocates on a path a
formatter would call per string.

`TokenText` (`biome_rowan/src/token_text.rs`) holds a `GreenToken` plus a
relative range, derefs to `str`, and is `Send + Sync`. It is exactly what Biome
uses for this: `inner_string_text()` at `biome_js_syntax/src/lib.rs`, wrapped by
`JsStringLiteralExpression::inner_string_text()` in `expr_ext.rs` and
`JsDirective::inner_string_text()` in `directive_ext.rs`.

Ship the `TextRange` sibling at the same time. It is what an LSP wants (a
completion inside a string, a diagnostic span), it shares the whole
implementation, and adding it later means revisiting the same file.

### D3. `ERROR_TOKEN` for constructs that run off the end of the file

Unterminated strings and unterminated backtick names become `ERROR_TOKEN`.
Unterminated numbers (`0x`, `1e`, deviation 8) and unterminated `%...%` do
**not** change.

The line: keep the kind when the token's extent is right and only its content is
malformed; use `ERROR_TOKEN` when the lexer never found a closing delimiter and
guessed "the rest of the file".

- `0x`, `1e`, `%a\n` are bounded. `consume_special()` stops at a newline, and
  the number scans stop where the literal stops. A diagnosed literal token tells
  the truth about its own span.

- `consume_quoted_string_content()` and `consume_backtick_identifier()` run to
  EOF. `x <- "abc` followed by 500 lines produces one token covering all 500
  lines, and today the tree says it is a valid string.

Two supporting arguments:

- **It is deviation 6's own argument.** `docs/deviations.md` rejects `IDENT` for
  `€` because "the tree would assert that `€` is a name, and nothing in the
  harness would catch it — round-trip passes either way." Today the
  `Unclosed Backtick` block in `literals/identifiers.snap.md` comes back as an
  `RIdentifier` whose `name_token` is a single `IDENT` covering the whole file —
  the backtick, the escaped backtick after it, and the trailing newline. Same
  lie, same invisibility.

- **JS does this for strings.** `read_str_literal` returns `ERROR_TOKEN` when
  `consume_str_literal()` fails (`biome_js_parser/src/lexer/mod.rs`). The one
  place JS keeps the kind is an unterminated *block comment*, because trivia
  cannot become a real token — R has no block comments, so it does not come up.

**The oracle does not constrain this decision at all.**
`crates/aether_r_parser_tests/src/check.rs` gates the comparison on
`!has_errors && !markers.deviation`, so every `[error]` block is already outside
the differential and the unterminated cases have never been compared. On air's
side it would not matter anyway: per deviation 6, any tree-sitter `ERROR` makes
air discard the tree for one whole-file `R_BOGUS_EXPRESSION`, which deviation 4
already walks away from.

What this costs, recorded honestly: the tree stops recording *which* construct
broke, because `parse_primary_expression()`'s existing `ERROR_TOKEN` arm builds
`R_BOGUS_EXPRESSION` and not `R_BOGUS_VALUE`. The diagnostic still says
"Unterminated string.", so a human loses nothing and a tree walker does. Nothing
downstream is affected today: air refuses to format any file with parse errors
(`crates/workspace/src/format.rs`), and ark reads tree-sitter directly.

What it buys, beyond honesty: the ungram's `value` slot is genuinely required,
and `inner_string_text()` becomes total by construction — no "is this string
actually terminated" check, which is the invariant Biome's version asserts in a
SAFETY comment.

Recovery does not get worse, and this is the part that makes it easy: these
tokens eat the rest of the file, so there is nothing after them to recover into.

### D4. The oracle collapses `R_STRING_VALUE`, rather than `[deviation]` on every string test

Air's tree keeps three tokens until swap-in, so without normalization every
block containing a string would mismatch. Marking them all `[deviation]` would
throw away real coverage. Collapse instead, in
`crates/aether_r_oracle/src/shape.rs`.

Coverage loss is close to zero. The collapsed comparison still pins where every
string starts and ends, which is what the raw-string delimiter logic gets wrong
when it gets anything wrong (`r"(())"`, hyphen counts, `r"-abc"` falling through
to an identifier). The internal boundaries stop existing on aether's side, so
there is nothing there to lose coverage on.

## Implementation

### Step 1 — Kinds and grammar

`xtask/codegen/src/r_kinds_src.rs`

- Remove `"STRING_OPEN"`, `"STRING_CONTENT"`, `"STRING_CLOSE"` from `tokens:`.

- Add `"R_SINGLE_QUOTED_STRING_LITERAL"`, `"R_DOUBLE_QUOTED_STRING_LITERAL"`,
  `"R_RAW_STRING_LITERAL"` to `literals:`.

`xtask/codegen/r.ungram`

- Replace the `RStringValue` rule with the alternation from D1.

Then `just codegen`. Expect churn in `crates/aether_r_syntax/src/generated/`
and `crates/aether_r_factory/src/generated/`.

`crates/aether_r_syntax/src/lib.rs` is hand-written and has two exhaustive
matches over every kind — the non-bogus arm of `is_bogus()` and `to_bogus()`.
Swap the three old kinds for the three new ones in both. The new kinds degrade
to `R_BOGUS` like every other token; `R_STRING_VALUE` keeps degrading to
`R_BOGUS_VALUE`.

Also add `TokenText` to the `pub use biome_rowan::{...}` re-export line so
consumers get the return type without depending on `biome_rowan` directly.

### Step 2 — Lexer

`crates/aether_r_parser/src/lexer.rs`

Delete `RLexContext` entirely and set `type LexContext = ()` on the `Lexer`
impl. `biome_parser` provides `impl LexContext for ()` (`lexer.rs`, near the
`LexContext` trait), and `()::is_regular()` is always true, so `BufferedLexer`
never resets its lookahead. `next_token()` loses its `match context` and calls
`lex_token()` directly.

`RStringDelimiter` mostly survives — the raw-string scan still needs quote,
hyphen count, and bracket. What goes:

- `from_open()`, which existed only so the parser could read the delimiter back
  off the `STRING_OPEN` text. Nothing reads it now.

- The split between `consume_string_open()`, `lex_string_content()`, and
  `lex_string_end()`, which collapse into one function.

Add a `kind()` method on `RStringDelimiter` returning the matching literal kind.
Keep `open_length()` and `close_length()`.

The new shape, with the existing scan bodies moved in unchanged:

```rust
fn consume_string(&mut self, delimiter: RStringDelimiter) -> RSyntaxKind {
    let open = TextRange::at(
        self.text_position(),
        TextSize::from(delimiter.open_length() as u32),
    );
    self.advance(delimiter.open_length());

    let terminated = match delimiter {
        RStringDelimiter::Single | RStringDelimiter::Double => {
            self.consume_quoted_string_content(delimiter.quote())
        }
        RStringDelimiter::Raw { quote, hyphens, bracket } => {
            self.consume_raw_string_content(quote, hyphens, bracket)
        }
    };

    if !terminated {
        let eof = TextRange::empty(self.text_position());
        self.push_diagnostic(unterminated_string(open, eof));
        return ERROR_TOKEN;
    }

    self.advance(delimiter.close_length());
    delimiter.kind()
}
```

`consume_quoted_string_content()` and `consume_raw_string_content()` currently
return `()` and just stop. Give them a `bool`: the quoted one returns
`self.current_byte() == Some(quote)`, the raw one returns
`self.at_raw_string_close(quote, hyphens, bracket)`. Do not otherwise touch
their bodies — the raw-string resume-after-a-failed-bracket behaviour
(r-lib/tree-sitter-r#162) and the trailing-escape guard are both load bearing
and both already tested.

The two call sites in `lex_token()` (the `b'"'` / `b'\''` arm and the raw-string
check inside the identifier arm) now call `consume_string()`.

`consume_backtick_identifier()`: return `ERROR_TOKEN` instead of `IDENT` on the
EOF path, right after `push_diagnostic(unterminated_backtick_identifier(...))`.
That is a one-line change and it is what D3 buys for backticks.

`crates/aether_r_parser/src/diagnostics.rs` — `unterminated_string()` keeps its
signature and both spans; it just moves from a parser call site to a lexer one.

### Step 3 — Token source and parser

`crates/aether_r_parser/src/token_source.rs`

- Drop the `BumpWithContext` impl. Fold `bump_with_context()` into `bump()` and
  `skip_as_trivia_with_context()` into `skip_as_trivia()`.

- `next_non_trivia_token()` loses its `context` parameter and calls
  `self.lexer.next_token(())`.

`crates/aether_r_parser/src/parser.rs`

- Delete the `do_bump_with_context()` override and move the `]]` join into a
  `do_bump()` override. The body is `biome_parser`'s default plus the existing
  join:

```rust
fn do_bump(&mut self, kind: Self::Kind) {
    let end = self.cur_range().end();
    self.context_mut().push_token(kind, end);

    if self.context().is_skipping() {
        self.source_mut().skip_as_trivia();
    } else {
        self.source_mut().bump();
    }

    // Joins an adjacent `] ]` into the `]]` the lexer never emits
    if self.scope == Scope::Bracket2 && self.cur() == T![']'] {
        self.re_lex(RReLexContext::JoinBrackets);
    }
}
```

The join is still load bearing for `x[[]]`; `parse_subset2_arguments()`'s
comment about bumping inside `with_scope()` stays true.

### Step 4 — Parser string sites

`crates/aether_r_parser/src/syntax/value.rs` shrinks to a bump:

```rust
pub(crate) fn parse_string_value(p: &mut RParser) -> ParsedSyntax {
    debug_assert!(p.at_ts(STRING_LITERALS));

    let m = p.start();
    p.bump_any();
    Present(m.complete(p, R_STRING_VALUE))
}
```

Define `STRING_LITERALS` as a `token_set!` of the three kinds somewhere shared —
`value.rs` is the natural owner, exported `pub(crate)`.

Call sites, both of which currently match `STRING_OPEN`:

- `crates/aether_r_parser/src/syntax/expr.rs`, the arm in
  `parse_primary_expression()`.

- `crates/aether_r_parser/src/syntax/call.rs`, the arm in
  `parse_argument_name()`, plus deleting the TODO comment in `parse_argument()`.

Spell the three kinds out in the `match p.cur()` arms rather than reaching for a
guard; it reads better and mirrors the `T![na_logical] | ...` arm above it.

`AnyRSelector` also admits `RStringValue`, but extract and namespace expressions
have not landed yet — when that phase arrives its selector arm needs the same
three kinds.

### Step 5 — The content API

New file `crates/aether_r_syntax/src/string_value_ext.rs`, registered in
`lib.rs` next to `argument_ext` and `call_ext`. No public enum: the token kinds
are public already, so an enum would be sugar, and nothing needs it yet.

```rust
impl RStringValue {
    /// The text between the delimiters
    ///
    /// `"foo"` and `r"---(foo)---"` both give `foo`
    pub fn inner_string_text(&self) -> SyntaxResult<TokenText> {
        let token = self.value()?;
        Ok(token.token_text_trimmed().slice(inner_range(&token)))
    }

    /// Source range of [RStringValue::inner_string_text()]
    pub fn inner_string_range(&self) -> SyntaxResult<TextRange> {
        let token = self.value()?;
        Ok(inner_range(&token) + token.text_trimmed_range().start())
    }
}

/// Relative to the start of the token
fn inner_range(token: &RSyntaxToken) -> TextRange {
    let text = token.text_trimmed();

    let (open, close) = match token.kind() {
        R_SINGLE_QUOTED_STRING_LITERAL | R_DOUBLE_QUOTED_STRING_LITERAL => (1, 1),
        // `r`, the quote, the hyphens, and the bracket; the close drops the `r`
        R_RAW_STRING_LITERAL => {
            let hyphens = text.as_bytes()[2..]
                .iter()
                .take_while(|byte| **byte == b'-')
                .count();
            (3 + hyphens, 2 + hyphens)
        }
        kind => unreachable!("`{kind:?}` is not a string"),
    };

    TextRange::new(
        TextSize::from(open as u32),
        TextSize::from((text.len() - close) as u32),
    )
}
```

Under D3 this is total: the three literal kinds are only ever emitted for a
string that found its closing delimiter, so `open + close <= text.len()` always
holds and no termination check is needed. Say so in a comment on `inner_range()`
— one sentence — because it is the invariant that would silently break if
someone later decides an unterminated string should keep its kind.

Use `text_trimmed()` and `text_trimmed_range()` throughout, never the untrimmed
variants, or leading trivia lands inside the content.

### Step 6 — Oracle

`crates/aether_r_oracle/src/shape.rs`. In `children_of()` / `shape_of()`, when a
node's canonical kind is `R_STRING_VALUE`, emit a single `Shape::Token` spanning
the node's `text_trimmed_range()` instead of walking its children, with a
canonical kind name shared by both sides (`"STRING"`). Comments cannot appear
inside a string, so `leading_comments` / `trailing_comments` come from the
node's first and last token and cost nothing to get right.

Both trees go through this, so air's three tokens and aether's one collapse to
the same shape. Add a unit test in `crates/aether_r_oracle/src/lib.rs` alongside
the existing `R_STATEMENT` / `SEMICOLON` normalization tests.

### Step 7 — Tests

Lexer tests under `crates/aether_r_parser/src/lexer/tests/`:

- `tests.rs` — the `lex()` helper drives `RLexContext` and tracks the open
  delimiter. All of that goes; it becomes a plain `next_token(())` loop. Its
  doc-comment about "the lexer cannot be driven by `next_token(Regular)` alone"
  is now false and should be deleted rather than reworded.

- `strings.rs`, `raw_strings.rs` — every `assert_lex!` collapses to one kind
  with the full byte length. `empty_strings_have_no_content_token` and
  `an_empty_body_has_no_content_token` no longer describe anything; replace them
  with a single test that `""` and `r"()"` are one token.

- The unterminated tests move to `assert_lex_err!` with `ERROR_TOKEN` and the
  "Unterminated string" substring. There is a matching set in `backticks.rs`.

- `checkpoint.rs` — delete `a_lookahead_over_a_string_leaves_nothing_behind` and
  `a_string_open_reads_back_as_its_delimiter`. Keep
  `a_name_is_not_a_raw_string_open`, which still guards
  `try_from_raw_string_open()`. The other rewind tests are untouched.

- `brackets.rs`, `line_breaks.rs`, `coverage.rs` — mechanical kind renames.

New integration test `crates/aether_r_parser_tests/tests/string_value_ext.rs`,
following the shape of the existing `tests/manifest.rs`: parse a source, cast
the node, assert on `inner_string_text()` and `inner_string_range()`. Cover at
minimum `"foo"`, `''`, `r"(a)"`, `r"---(a)---"`, a string containing its own
bracket (`r"(())"`), and a multiline string.

Corpus, all regenerated by `just accept`, none hand-edited:

- `literals/strings.snap.md` — every tree, plus the three `[error]` blocks which
  become a bogus node around an `ERROR_TOKEN`.

- `literals/identifiers.snap.md` — `Unclosed Backtick` becomes the same.

- `literals/comments.snap.md`, `expressions/call.snap.md`,
  `expressions/subset2.snap.md` — incidental strings.

No `.md` input file needs editing and no test moves, so `MANIFEST.toml` is
untouched. Do not add corpus tests for this; the 89 imported tests stay as they
are.

Worth parsing by hand once each, since they are the interesting recovery paths:
`f("abc`, `x <- "abc`, and a string opened on line 1 of a long file.

### Step 8 — Docs

`docs/deviations.md`

- Extend deviation 6 rather than opening a new entry for `ERROR_TOKEN`. It is
  the same argument applied to two more producers, and 6 already carries the
  `is_bogus()` / `to_bogus()` swap-in cost for the kind. State the
  bounded-versus-runs-to-EOF line explicitly, and say why deviation 8's numbers
  and `%...%` stay as they are.

- Add deviation 12 for one-token strings, following the entry-10 template:
  example, both trees, why deviate, swap-in cost, and an `Oracle` subsection
  recording that this is handled by normalization rather than by `[deviation]`
  markers.

- Add both to the summary table at the top of the file.

`plans/aether-implementation-plan.md` — this is the largest prose job and it is
not optional; several sections currently argue *for* the three-token design.

- §8.1 — the "lookahead lexes in `RLexContext::default()`" paragraph and its
  forward reference to §11.3. The constraint is gone.

- §8.2.1 "The lexer keeps no string state" — delete. The invariant it protects
  no longer has anything to protect.

- The lexer-contract section that says "Strings are three tokens" and explains
  the optional `STRING_CONTENT`, and the neighbouring paragraph contrasting
  backticks with strings, which becomes a statement that they now work alike.

- §9.1.1 "The string delimiter comes from the token text" — delete.

- §11.1's `parse_string_value()` sketch.

- §11.3's `parse_argument()` doc-comment: keep the optimistic-parse rationale,
  drop the sentence blaming the lookahead constraint for it, since the reason is
  now that it is better rather than that it is forced.

- §12.2's performance note, which cites the same constraint.

Grep the plan for `STRING_OPEN`, `STRING_CONTENT`, `RLexContext`, and
`lookahead` and work the list; the sections above are the ones that need
rewriting rather than a token rename.

`CLAUDE.md` needs nothing.

## Verification

- `just test` — the workspace, including the differential check on every block
  without a diagnostic.

- `just accept` then `just test` — snapshots must be stable on the second run.

- Round-trip is asserted by the harness on every corpus block; the `[error]`
  blocks are where a one-token lexer is most likely to drop bytes, so read those
  three snapshots by eye rather than trusting the assertion alone.

- Confirm no `bump_with_context` / `RLexContext` references survive:
  `rg 'LexContext|STRING_OPEN|STRING_CONTENT|STRING_CLOSE' crates xtask`.

## Swap-in impact on air

Smaller than the file count suggests. Of the files in air that touch these three
tokens, `crates/air_r_parser/src/parse.rs` and
`crates/air_r_parser/src/treesitter.rs` are deleted by the swap anyway — the
`handle_string_value_enter()` / `handle_string_content_enter()` walker is
precisely what aether replaces.

What is left:

- `xtask/codegen/r.ungram` and `xtask/codegen/src/r_kinds_src.rs`, the same edit
  as Step 1, then air's codegen.

- `crates/air_r_syntax/src/lib.rs` — the same two exhaustive matches.

- `crates/air_r_formatter/src/r/auxiliary/string_value.rs` — three writes become
  one.

- `crates/air_r_formatter/src/string_content.rs` — normalize the whole token
  rather than the content token. Identical in effect, since no delimiter
  contains `\r\n`, and `format_replaced` + `syntax_token_cow_slice` work the
  same on the wider token.

- `crates/air_r_syntax/src/call_ext.rs` — unchanged under D1, which is one of
  the reasons for D1.

Air's formatter specs are R-in / R-out, so no snapshot churn there.

## Non-goals

- Do not change `parse_argument()` to use lookahead. See "Not a reason" above.

- Do not touch deviation 8's number literals or `%...%`.

- Do not add an `RStringQuote` enum, an `is_raw()`, or a quote-style helper
  until a caller needs one. The token kinds are public and matchable.

- Do not unescape anything. `inner_string_text()` strips delimiters and nothing
  else; `"a\nb"` comes back with a literal backslash and `n`.

- Do not add corpus tests.
