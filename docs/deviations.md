# Deviations

Every intentional difference between aether's tree and the tree produced by
`posit-dev/air`'s tree-sitter-based parser, with an example, both outputs, and
the reason.

Three categories, kept apart on purpose:

- [Aether vs. tree-sitter-r](#aether-vs-tree-sitter-r) — choices aether made, and
  can revisit.

- [tree-sitter-r vs. R](#tree-sitter-r-vs-r) — places where tree-sitter-r already
  differs from R and aether follows tree-sitter anyway, so the oracle agrees.
  Upstream behaviour we are deliberately not fixing.

- [Inherited from air](#inherited-from-air) — air conventions aether copies
  without endorsing. A to-do list with a natural date: the swap into air, after
  which snapshots can be regenerated in one deliberate pass.

The differential oracle reads nothing from this file. Reviewers do, and so does
whoever updates air's formatter.

Corpus tests marked `[deviation]` are the ones the oracle skips because of an
entry below; each entry names its tests. Entries also record the phase that lands
them, since the ledger runs ahead of the parser.

## Aether vs. tree-sitter-r

| # | Deviation | tree-sitter-r | aether |
|---|---|---|---|
| 1 | Semicolons | trivia, invisible | `SEMICOLON` token in `RStatement`; formatter removes it and hard-breaks the line, so `1;2` → two lines |
| 1b | Statement list | `R_EXPRESSION_LIST` of expressions | `R_STATEMENT_LIST` of `R_STATEMENT` |
| 1c | Braces | `R_BRACED_EXPRESSIONS` | `R_BRACED_STATEMENTS` |
| 2 | Extract RHS | `optional()`, silent | required, diagnostic + bogus |
| 3 | Namespace RHS | `optional()`, silent | required, diagnostic + bogus |
| 4 | Error recovery | whole-file bogus node | localized bogus nodes, tree still walkable |
| 5 | `AnyRSelector` | 4 variants, no bogus | new `RBogusSelector` kind added as a fifth variant, so a malformed RHS is contained in the `right` slot instead of collapsing the whole expression |
| 6 | A character that starts no token | `ERROR` node | reported, then lexed as `IDENT` — there is no error-token kind to give it |
| 7 | `1 L` | one `integer` spanning the space | the literal `1`, then the name `L` |

### 1. Semicolons are real tokens

Landed in Phase 3.

Example:

```r
1;2
```

tree-sitter-r, as walked by air, maps `;` to `TriviaPieceKind::Whitespace`, so the
semicolon is inside a trivia piece and is not a token at all:

```
RDoubleValue { value_token: R_DOUBLE_LITERAL@0..1 "1" [] [] },
RDoubleValue { value_token: R_DOUBLE_LITERAL@1..3 "2" [Whitespace(";")] [] },
```

Aether lexes `;` as `SEMICOLON` and hangs it off the statement that precedes it:

```
RStatementList [
    RStatement {
        expression: RDoubleValue { value_token: R_DOUBLE_LITERAL@0..1 "1" [] [] },
        semicolon_token: SEMICOLON@1..2 ";" [] [],
    },
    RStatement {
        expression: RDoubleValue { value_token: R_DOUBLE_LITERAL@2..3 "2" [] [] },
        semicolon_token: missing (optional),
    },
]
```

Why: a token inside a trivia piece cannot be `format_removed()`, so air's
formatter can never drop or rewrite a semicolon. Making it a token is the whole
point; the wrapper node is what makes the token expressible.

Both of `RStatement`'s slots are optional, the same trick `RArgument` already
uses, because R allows `;;` and a leading `;`:

```r
;        # RStatement { expression: missing, semicolon: ";" }
1        # RStatement { expression: 1,       semicolon: missing }
1;       # RStatement { expression: 1,       semicolon: ";" }
```

A statement with neither slot filled is never produced.

Formatting semantics, settled with the owner: the semicolon is removed and
replaced by a hard line break, so `1;2` formats as two lines.

Corpus tests: `statements/semicolons.md`.

### 1b. `R_STATEMENT_LIST` replaces `R_EXPRESSION_LIST`

Landed in Phase 3.

`RExpressionList` is replaced outright rather than kept alongside — once each
element carries an optional semicolon it is a list of statements, not of
expressions, and the name should say so. `RRoot` and `RBracedStatements` both
point at the new list, and the field is renamed with it:

```diff
 RRoot =
 	bom: 'UNICODE_BOM'?
-	expressions: RExpressionList
+	statements: RStatementList
 	eof: 'EOF'
```

Consequences for air's formatter at swap time:

- `RStatementList` iteration yields `RStatement`, not `AnyRExpression`.

- `RRoot::statements()` and `RBracedStatements::statements()` replace
  `.expressions()`; every walk gains a `.expression()` hop to reach the payload.

- `RStatement::semicolon_token()` is the new hook for dropping or rewriting a
  semicolon.

Two alternatives were rejected. Semicolons cannot stay trivia under a dedicated
`TriviaPieceKind` — that is a Biome enum and aether does not fork it. Putting the
`;` slot on every expression node instead of a wrapper works, but touches every
expression node in the grammar.

### 1c. `RBracedExpressions` becomes `RBracedStatements`

Landed in Phase 3.

It holds statements now, so it is named for them. This renames air's
`AnyRExpression::RBracedExpressions` variant and its formatter rule file
`crates/air_r_formatter/src/r/expressions/braced_expressions.rs` →
`braced_statements.rs`. Air's formatter codegen derives those paths from the node
names, so re-running it after the swap generates the new stub; the hand-written
body moves across by hand.

### 2. Extract RHS is required

Lands in Phase 10.

Example:

```r
dplyr::x   # fine everywhere
a$
a$1
```

tree-sitter-r's `extract_operator` declares `optional(field("rhs", ...))`
deliberately — its grammar comment says this is so `dplyr::` yields a namespace
node with a known package name for completions. Air's `r.ungram` already declares
the RHS required, so `a$` walks to an `RExtractExpression` with
`right: missing (required)` and no diagnostic at all. That test lives in air's
`tests/snapshots/undefined/` precisely because nobody was happy with it.

Aether needs no grammar change — the slot is already required. The deviation is
in the parser:

- `a$b`, `a$"b"`, `a$...`, `a$..1` → `R_EXTRACT_EXPRESSION`, clean.

- `a$` → `R_EXTRACT_EXPRESSION` with a missing `right` slot and an
  `expected_selector` diagnostic pointing just past the operator, with a secondary
  label on the operator itself. The message names the operator, so `$`, `@`, `::`
  and `:::` each read differently:

  ```
  × Expected an identifier, a string, `...` or `..i` but instead found the end of the file.

    1 │ dplyr::
      │      ── `::` requires a name on the right
      │        ▲
      │        ╰── Expected an identifier, a string, `...` or `..i` here.
  ```

- `a$1`, `a$NA`, `a$NULL`, `a$TRUE`, `a$(b)`, `a${b}` → the RHS is present but is
  not a valid selector, so it is wrapped in `R_BOGUS_SELECTOR` and
  `expected_selector` labels the offending range. The bogus node consumes exactly
  one atom — no postfix, no infix — so `a$1 + 2` still parses the `+` normally.

Why: `missing (required)` with no diagnostic is a silent hole in the tree. The
completions argument that motivates tree-sitter's `optional()` is served just as
well by an `R_EXTRACT_EXPRESSION` that carries a diagnostic — the `left` slot is
still there to complete from.

Corpus tests: `Dollar No RHS`, `Slot No RHS` in `expressions/extract.md`, all
marked `[deviation]`.

### 3. Namespace RHS is required

Lands in Phase 10.

Identical to deviation 2, for `::` and `:::`. Split out because tree-sitter-r
models namespace as its own rule with its own `optional()`.

Corpus tests: `Namespace With Missing RHS` in `expressions/namespace.md`, marked
`[deviation]`.

### 4. Recovery is localized

Lands progressively; the errors corpus goes fully green in Phase 14.

Example:

```r
f(
1 + 1
```

tree-sitter-r's error recovery, as surfaced by air, collapses the file: a parse
failure anywhere yields one bogus node spanning the whole input, and the LSP shows
a single whole-file error. Nothing after the error point is walkable.

Aether places bogus nodes at the smallest node that failed and keeps parsing, so
the rest of the file still produces real nodes and each error carries its own
range. This is the most user-visible win of the project and belongs in air's
changelog at swap time.

Why: every consumer of the tree — the formatter, the LSP, lint rules — degrades
from "no information" to "information everywhere except the broken span".

This deviation is why the oracle compares trees only for tests that parse cleanly
or are marked `[deviation]`; two different recovery strategies are not expected to
agree.

### 5. `AnyRSelector` gains `RBogusSelector`

Landed in Phase 3.

`AnyRSelector` had four variants and no bogus one:

```rust
pub enum AnyRSelector {
    RDotDotI(RDotDotI),
    RDots(RDots),
    RIdentifier(RIdentifier),
    RStringValue(RStringValue),
}
```

Without a bogus variant the generated factory does not panic on a failed cast, it
does something quieter and worse — `R_EXTRACT_EXPRESSION.to_bogus()` is
`R_BOGUS_EXPRESSION`, so `a$1` collapses the entire extract expression into one
bogus node instead of containing the damage in the `right` slot. Round-trip still
passes, so nothing in the harness would catch it; it would only show up by reading
the snapshot.

`R_BOGUS_SELECTOR` is a new node kind, added as the last kind before `__LAST` so
nothing renumbers. `is_bogus()` returns true for it and `to_bogus()` is
idempotent. The four selector node kinds keep their existing `to_bogus()`
mappings: `to_bogus()` answers "this node kind failed to build, what does it
degrade to", which is about the node being built, not the slot it sits in, and
`R_IDENTIFIER` or `R_STRING_VALUE` appear in plenty of non-selector positions.

Why a new kind rather than reusing `RBogusValue`:

- Biome's convention is one bogus kind per restricted position — `js.ungram` has
  ten of them. `AnyJsDecorator` is the one place a broader bogus kind is reused in
  a restricted union, and it is the exception.

- The name does not lie. The bogus RHS wraps one whole prefix atom, so `a${b}`
  wraps `R_BRACED_STATEMENTS`, `a$(b)` wraps `R_PARENTHESIZED_EXPRESSION`, and
  `a$-1` wraps `R_UNARY_EXPRESSION`. None of those is a value, and snapshots are
  the artifact reviewers read most.

- The kind alone identifies the position. A tree walker seeing `R_BOGUS_SELECTOR`
  knows a selector was malformed without inspecting the parent.

Swap-in cost: air's `RSyntaxKind`, `nodes.rs`, and both factories are generated,
so they follow from air's own codegen. The hand-written touch points are
`is_bogus()` and `to_bogus()` in `air_r_syntax/src/lib.rs`, plus one formatter
rule file — nearly free, since air's bogus rules are empty impls its codegen stubs
out:

```rust
impl FormatBogusNodeRule<RBogusSelector> for FormatRBogusSelector {}
```

`FormatAnyRSelector` in `air_r_formatter/src/r/any/selector.rs` gains a fifth arm
in the same generated pass.

### 6. A character that starts no token becomes an `IDENT`

Landed in Phase 4.

Example:

```r
€
```

tree-sitter-r produces an `ERROR` node. Aether has nowhere to put one:
`r_kinds_src.rs` has no error-token kind, `R_BOGUS` and friends are *node* kinds,
and adding a token kind would diverge from air's kind list. Dropping the
character is not an option either — the tree has to round-trip, and folding it
into trivia would make it silently disappear exactly the way semicolons used to.

So the lexer reports it with an `Unexpected character` diagnostic and hands it on
as an `IDENT`. The file never parses cleanly, so the oracle skips it, and the
character survives into the tree where a reader can see it.

The reachable cases are narrow. Every printable ASCII character starts some R
token, so this is only control characters and non-ASCII characters that are
neither `XID_Start` nor `XID_Continue`.

### 7. `1 L` is a literal and a name

Landed in Phase 4.

Example:

```r
1 L
```

tree-sitter-r spells `integer` as `seq($._float_literal, "L")` rather than as one
regex, so the `L` is a token of its own and `extras` lets whitespace fall between
the two. `1 L` and `1 i` therefore come out as a single `integer` and `complex`
spanning the space, which is not what R does — R stops the literal at the digit
and reads `L` as a name.

Aether cannot reproduce it even in principle: `R_INTEGER_LITERAL` is one token in
air's kind list, and one token cannot span a gap. It follows R here.

The adjacent case is the one that matters in practice, and aether does match
tree-sitter on all of it: the suffix is taken only when it is not the start of a
longer name, so `1L` is an integer while `1Li` is the literal `1` followed by the
name `Li`.

## tree-sitter-r vs. R

Places where tree-sitter-r already disagrees with R's own parser. Aether follows
tree-sitter, because tree-sitter is the oracle and agreeing with it is what keeps
the differential check meaningful. These are not aether's choices; they are logged
so nobody has to rediscover them.

### `%foo` at end of file

Example:

```r
x %in
```

R accepts it. `SpecialValue` in `src/main/gram.y` exits its loop on `R_EOF`
without ever seeing the closing `%` and still returns a `SPECIAL` token.

tree-sitter-r's regex is `/%[^%\\\n]*%/`, which requires the closing `%`, so it
errors.

Aether follows tree-sitter and errors.

Note that aether does copy R for the neighbouring case, an unterminated `%…%` cut
short by a newline: R ungets the newline before reporting, so the error stays
local and the next line parses normally. Aether stops before the newline too,
keeping the partial `%foo` as a `SPECIAL` token with the diagnostic attached.
That is not a deviation from tree-sitter, just a recovery choice tree-sitter has
no equivalent of.

### Backslashes inside `%…%`

Example:

```r
a %a\b% b
```

R's `SpecialValue` does not exclude backslash, so this is a valid `SPECIAL`.
tree-sitter-r's character class `[^%\\\n]` does exclude it, so it errors. Their
own grammar comment concedes the regex is approximate: "This could probably be
fine tuned to disallow more things."

Aether follows tree-sitter and errors.

## Inherited from air

Air conventions aether copies without endorsing, findable in one place for the
day the swap is done and the whole corpus can be regenerated deliberately.

### Trivia attachment polarity

Air attaches whitespace between two same-line tokens as leading of the following
token; Biome's own parsers attach it as trailing of the preceding one. Air also
makes the same whitespace-before-a-newline trailing or leading depending on
whether any token follows later in the file (`1 + \n1` vs `1  \n `). Aether copies
both, because deviating would change every snapshot and every oracle comparison
for no user-visible gain — the formatter regenerates whitespace regardless, and
round-trip holds either way.

The JS-shaped simplification, if this is ever revisited, is `JsTokenSource`'s
whole body: `let mut trailing = !first;` and, in a single streaming pass,
`if kind.is_newline() { trailing = false; }` before pushing each piece. No gap
buffer, no one-piece lookahead, no `between_two_tokens`, and the three special
cases collapse — whitespace and comments are trailing of the preceding token until
a newline is seen, then leading. That is roughly forty lines shorter than
`RTokenSource::next_non_trivia_token()` and states in one sentence.

The cost of adopting it is a whole-corpus snapshot regeneration, which is cheap
after the swap and expensive before it, since it would mask exactly the drift the
oracle is watching for.
