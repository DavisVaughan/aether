# Deviations

Every intentional difference between aether's tree and the tree produced by
`posit-dev/air`'s tree-sitter-based parser, with an example, both outputs, and
the reason.

Two categories, kept apart on purpose:

- [Aether vs. tree-sitter-r](#aether-vs-tree-sitter-r) — choices aether made, and
  can revisit.

- [tree-sitter-r vs. R](#tree-sitter-r-vs-r) — places where tree-sitter-r already
  differs from R and aether follows tree-sitter anyway. Upstream behaviour we are
  deliberately not fixing.

This file is read by reviewers, and by whoever updates air's formatter. Each
entry names the corpus tests it covers and records the phase that lands it,
since the ledger runs ahead of the parser.

## Aether vs. tree-sitter-r

| # | Deviation | tree-sitter-r | aether |
|---|---|---|---|
| 1 | Semicolons | trivia, invisible | `SEMICOLON` token in `RStatement`; formatter removes it and hard-breaks the line, so `1;2` → two lines |
| 1b | Statement list | `R_EXPRESSION_LIST` of expressions | `R_STATEMENT_LIST` of `R_STATEMENT` |
| 1c | Braces | `R_BRACED_EXPRESSIONS` | `R_BRACED_STATEMENTS` |
| 2 | Extract RHS | `optional()`, silent | required, diagnostic, slot left missing |
| 3 | Namespace RHS and LHS | `optional()` RHS, silent | required, diagnostic, slot left missing; a non-name LHS is contained in an `RBogusSelector` |
| 4 | Error recovery | whole-file bogus node | localized bogus nodes, tree still walkable |
| 5 | `AnyRSelector` | 4 variants, no bogus | new `RBogusSelector` kind added as a fifth variant, so a malformed operand is contained in its slot instead of collapsing the whole expression |
| 6 | Text no production can take: a character that starts no token, an unterminated string, an unterminated backtick-quoted name | `ERROR` node | new `ERROR_TOKEN` kind added, reported by the lexer and wrapped in a bogus node by the parser |
| 7 | `L` and `i` suffixes | tokens of their own, so `1 L` is one `integer` and `1Li` is `1` and `Li` | part of the literal, taken whatever follows, as R takes them |
| 8 | Unterminated numbers | `0x`, `0x1p`, `1e` re-split or accepted, all silently | one literal token carrying `Unterminated hexadecimal literal.`, `Unterminated binary exponent.`, or `Unterminated exponent.` |
| 9 | A backslash inside `%...%` | excluded by the `special` regex, so `%a\b%` errors | ordinary text, as R takes it, so `%a\b%` is one `SPECIAL` |
| 10 | Whitespace attachment | air's walker: leading of the following token, with a second rule for whitespace before a newline | `JsTokenSource`'s rule — trailing of the preceding token until a newline, leading from the newline on. Comment and newline attachment are unchanged |
| 11 | Two expressions on one line | accepted silently, since `program` needs no separator | an error, as R requires |
| 12 | Strings | `string` node over an open, an optional content, and a close | one literal token per quote style, delimiters included, with the body reachable through `RStringValue::inner_string_text()` |
| 13 | `AnyRParameterName` | 3 variants, no bogus | new `RBogusParameterName` kind added as a fourth variant, so a parameter that is not a name is contained in its slot instead of collapsing the list around it |
| 11b | A `;` with no expression before it on the same line | accepted silently everywhere | an error at top level, as R requires; accepted inside `{}`, as R does |
| 14 | The `for` variable | `RIdentifier` only, so `for (... in x) y` collapses the whole file to one bogus node | new `AnyRForVariable` union of `RIdentifier`, `RDots`, `RDotDotI`, and a new `RBogusForVariable`, so `...` and `..i` are taken as R takes them |
| 15 | `OperatorPrecedence` and `call_ext.rs` | vendored into `air_r_syntax`, used by the formatter | not carried into `aether_r_syntax`; not a tree-shape deviation |

### 1. Semicolons are real tokens

Grammar landed in Phase 3, the parser in Phase 13.

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
uses, because R allows `;;` and a leading `;` inside `{}`:

```r
;        # RStatement { expression: missing, semicolon: ";" }
1        # RStatement { expression: 1,       semicolon: missing }
1;       # RStatement { expression: 1,       semicolon: ";" }
```

A statement with neither slot filled is never produced.

**A `;` is the semicolon of the expression before it only when no newline
intervenes.** R's `prog` offers `expr '\n'` and `expr ';'` as alternatives, so
the first terminator to arrive is the one that reduces the expression: `1;` is
one expression, `1\n;` is `2:1: unexpected ';'`. Braces accept the second form,
through `exprlist ';'` trailing a list that `1` has already left, so the `;` is
not `1`'s there either. Aether therefore gives a `;` after a line break an
`RStatement` of its own, which also leaves any blank line above it in that
statement's leading trivia rather than inside a token the formatter is about to
delete.

Whether such a statement is *allowed* is deviation 11b.

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

Landed in Phase 10.

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
  `expected_rhs_selector` diagnostic pointing just past the operator, with a secondary
  label on the operator itself. The message names the operator, so `$`, `@`, `::`
  and `:::` each read differently:

  ```
  × Expected an identifier, a string, a `...`, or a `..i` but instead found the end of the file.

    1 │ dplyr::
    2 │
      │

  i Expected an identifier, a string, a `...`, or a `..i` here.

  i `::` requires a name on the right.

  > 1 │ dplyr::
      │      ^^
  ```

- `a$1`, `a$NA`, `a$NULL`, `a$TRUE`, `a$(b)`, `a${b}`, `a$if` → identical
  treatment. The RHS is present but is not a valid selector, so the slot is left
  missing and `expected_rhs_selector` labels it. **Nothing is consumed**, so the `1`,
  the `{b}`, or the whole `if` statement goes on to be parsed as the next
  statement, exactly as under tree-sitter. `a$(b)` is the one that does not become
  a sibling: `(` is postfix, so both parsers read a call whose function is the
  incomplete extract expression.

Why: `missing (required)` with no diagnostic is a silent hole in the tree. The
completions argument that motivates tree-sitter's `optional()` is served just as
well by an `R_EXTRACT_EXPRESSION` that carries a diagnostic — the `left` slot is
still there to complete from.

Why nothing is consumed: the imported corpus argues for it in its own comments.
On `foo${bar}` — "we nicely get a true node here alongside the braces … it's
useful for completions and highlighting" — and on `foo$if` — "this is actually
pretty decent behavior". Swallowing the RHS into a bogus node destroys exactly
that structure, and `$` sits in R's `EatLines` set, so `df$` at the end of a line
would eat the *next* line's first construct — while `df$` is the language's
completion trigger and its most common incomplete state.

So this deviation is diagnostics only, never shape. Aether's tree for `a$1` is
what air already builds: `RExtractExpression` with `right: missing (required)`,
then `RDoubleValue` as a sibling.

Corpus tests: `Dollar No RHS`, `Slot No RHS`, `Extract With A Literal RHS`, and
`Dollar, At, Namespace, Namespace Internal With Expression RHS` in
`expressions/extract.md`, all `[error]`.

### 3. Namespace RHS is required, and so is the LHS

Landed in Phase 10.

The RHS half is deviation 2, for `::` and `:::`. Split out because tree-sitter-r
models namespace as its own rule with its own `optional()`. A newline counts as a
missing RHS here and nowhere else — `::` and `:::` are the only operators absent
from R's `EatLines` switch, so `foo::\nbar` is two expressions rather than one,
and aether says so where tree-sitter-r stays quiet. The restriction lifts inside
`(`, `[`, and `[[`, where R discards newlines before `EatLines` ever runs, so
`f(stats::\nmedian)` is one clean namespace expression in both parsers.

The LHS half has no counterpart in deviation 2, because `RExtractExpression`
takes any expression on the left while `RNamespaceExpression` takes an
`AnyRSelector`. `a::b::c` and `a()::b` are errors under R and under tree-sitter-r
too, so the deviation is only in the recovery: air collapses the file to one bogus
node (deviation 4), and aether wraps the offending operand in an `RBogusSelector`
and keeps the namespace node, its operator, and its right-hand name.

Corpus tests: `Namespace get`, `Namespace get internal`, `Namespace With Missing
RHS`, `Namespace With A Literal RHS`, `Chained ::`, `Chained :::`, `Call On The
Left Of ::`, and `Call On The Left Of :::` in `expressions/namespace.md`, all
`[error]`. `Namespace Newline Is Significant Only Outside Parens` is the clean
half and carries no marker.

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
does something quieter and worse — `R_NAMESPACE_EXPRESSION.to_bogus()` is
`R_BOGUS_EXPRESSION`, so `a::b::c` collapses the entire namespace expression into
one bogus node instead of containing the damage in the `left` slot. Round-trip
still passes, so nothing in the harness would catch it; it would only show up by
reading the snapshot.

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

- The kind alone identifies the position. A tree walker seeing `R_BOGUS_SELECTOR`
  knows a selector was malformed without inspecting the parent.

**One path constructs one, and it is the `left` slot, not the `right`.** Phase 10
was going to use the `right` slot, by consuming a non-selector RHS into a bogus
node; deviation 2 settles that it diagnoses and consumes nothing. The LHS of `::`
and `:::` is an `AnyRSelector` too, and there the operand is already parsed by the
time the operator is read, so containing it costs no tokens:

```
a::b::c

RNamespaceExpression {
    left: RBogusSelector { RNamespaceExpression { a::b } },
    operator: COLON2,
    right: RIdentifier { c },
}
```

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

### 6. Text no production can take becomes an `ERROR_TOKEN`

Landed in Phase 4 as a new token kind in `r_kinds_src.rs`; Phase 7 added the
`ERROR_TOKEN` arm of the lhs match that wraps it. Unterminated strings and
unterminated backtick-quoted names joined it in the string simplification pass.

Example:

```r
€
"abc
`abc
```

tree-sitter-r produces an `ERROR` node. Aether reports each of these from the
lexer — `Unexpected character`, `Unterminated string.`,
`Unterminated backtick-quoted name.` — and hands the text on as an `ERROR_TOKEN`,
which the parser can only wrap in a bogus node. The file never parses cleanly,
and the text survives into the tree where a reader can see it.

Dropping the character is not an option — the tree has to round-trip, and folding
it into trivia would make it silently disappear exactly the way semicolons used
to.

Why a new kind rather than reusing `IDENT`:

- `IDENT` is accepted wherever a name is, so `x <- €` would build a well-formed
  `R_BINARY_EXPRESSION` with an `R_IDENTIFIER` right-hand side. The diagnostic
  would be there, but the tree would assert that `€` is a name, and nothing in
  the harness would catch it — round-trip passes either way. `ERROR_TOKEN` starts
  no production, so the damage is contained in a bogus node by construction.

- Every other Biome grammar has this kind: js, css, json, graphql, grit, html,
  markdown, and yaml all declare `ERROR_TOKEN` in their own `tokens` list. It is
  a convention rather than a `SyntaxKind` requirement, which is how air managed
  without it — air has no lexer, and any tree-sitter `ERROR` makes it discard the
  tree for one whole-file `R_BOGUS_EXPRESSION`, so no producer ever existed.
  Aether lexes, so it is in the same position as the other eight.

The reachable cases are narrow. Every printable ASCII character starts some R
token, so this is only control characters and non-ASCII characters that are
neither `XID_Start` nor `XID_Continue`.

Swap-in cost: `RSyntaxKind` is generated, so the kind follows from air's own
codegen once `r_kinds_src.rs` carries it. The hand-written touch points are the
`is_bogus()` and `to_bogus()` arms in `air_r_syntax/src/lib.rs` — `ERROR_TOKEN` is
not bogus itself and degrades to `R_BOGUS` like every other token. The formatter
needs nothing: `FormatBogusNodeRule` prints verbatim and never inspects the kinds
of the tokens inside a bogus node.

Unlike `R_BOGUS_SELECTOR` this one does renumber, since token kinds precede node
kinds in the generated enum. Nothing depends on the numbering — no explicit
discriminants are emitted, and `to_raw`/`from_raw` round-trip within a single
build — so the generated diff is the one added line.

#### Where the line falls

Not every diagnosed token becomes an `ERROR_TOKEN`. The test is whether the
lexer knew where the token ended:

- **Bounded.** `0x`, `1e`, and `%a` stop where the literal stops or at the next
  newline, so the token's extent is right and only its content is malformed. It
  keeps its kind and carries the diagnostic — deviation 8 for the numbers, the
  `%foo` entry below for the operator. A parser that goes on to use such a token
  is not being lied to about its span.

- **Runs to EOF.** `consume_string()` and `consume_backtick_identifier()` have no
  line-based stop, so `x <- "abc` followed by five hundred lines produces one
  token covering all five hundred. A well-formed kind there would assert that the
  rest of the file is a string, which is deviation 6's own argument for `€`: the
  diagnostic would be present, the tree would still lie, and round-trip passes
  either way. `biome_js_parser` draws the same line — `read_str_literal` returns
  `ERROR_TOKEN` when `consume_str_literal()` fails.

Recovery does not suffer. These tokens eat the rest of the file, so there is
nothing after them to recover into.

What it costs, recorded honestly: the tree stops recording *which* construct
broke, because the lhs match's `ERROR_TOKEN` arm builds
`R_BOGUS_EXPRESSION` and not `R_BOGUS_VALUE`. The diagnostic still names it, so a
human loses nothing and a tree walker does. Nothing downstream is affected today
— air refuses to format any file with parse errors, and ark reads tree-sitter
directly.

Corpus tests: `Unclosed Double Quote`, `Unclosed Single Quote`, and
`Unclosed Raw String` in `literals/strings.md`, and `Unclosed Backtick` in
`literals/identifiers.md`, all `[error]`.

### 7. `L` and `i` belong to the literal, not to the token stream

Landed in Phase 4.

Example:

```r
1 L
1Li
```

tree-sitter-r spells `integer` as `seq($._float_literal, "L")` rather than as one
regex, so the `L` is a token of its own. Two things follow from that, and aether
disagrees with both.

`extras` may fall between the two tokens, so `1 L` and `1 i` come out as a single
`integer` and `complex` spanning the space. Aether cannot reproduce that even in
principle — `R_INTEGER_LITERAL` is one token in air's kind list, and one token
cannot span a gap.

And the `L` competes with `identifier` on length, so a name that merely starts
with a suffix wins outright: `1Li` is the float `1` followed by the name `Li`.

R does neither. `NumericValue` in `src/main/gram.y` stops the literal at the digit
when a space follows, and otherwise takes the suffix whatever comes next: the `L`
enters the scan loop and breaks it, the `i` is picked up after it, and neither is
ever pushed back. Aether follows R on both counts.

| source | tree-sitter-r | R and aether |
|---|---|---|
| `1 L` | one `integer` spanning the space | `1`, then the name `L` |
| `1Li` | `float 1`, `identifier Li` | `1L`, then the name `i` |
| `1iL` | `float 1`, `identifier iL` | `1i`, then the name `L` |
| `1LL` | `float 1`, `identifier LL` | `1L`, then the name `L` |
| `1L.` | `float 1`, `identifier L.` | `1L`, then the name `.` |
| `1e5iL` | `float 1e5`, `identifier iL` | `1e5i`, then the name `L` |

Every row but the first is a syntax error in R and in aether, arriving as two
expressions on one line (deviation 11). tree-sitter-r parses all of them cleanly,
since its `program` does not require a separator between expressions. So nothing
is lost by disagreeing: every one of these is a syntax error in aether, so there
is no clean tree to compare against tree-sitter's in the first place.

Why follow R rather than tree-sitter: R's rule cannot disagree with R about a
program R accepts, because it never looks past the suffix. The lookahead rule can
only differ on programs R rejects, and there it puts the error one token earlier
than R does. It also cost a byte of lookahead that nothing else in the lexer
needed.

### 8. An unfinished number literal is diagnosed, not re-split

Landed in Phase 4.

Example:

```r
0x
0x1p
1e
```

A `0x` commits to a hexadecimal and an `e` or `p` commits to an exponent. When
what the grammar requires next is not there, tree-sitter-r un-matches the longer
token and re-splits the bytes into something shorter that happens to be valid:
`0x` becomes the number `0` and the name `x`, `0x1p` becomes `0x1` and `p`. Where
even that is unnecessary it simply accepts — its `_number_literal` regex ends
`[eE][+-]?\d*`, so `1e` and `1e+` are whole floats.

R rejects all of them, from the lexer, at the start of the literal:

```
> 1e
Error: unexpected input in "1e"
```

Aether follows R on the verdict and keeps the token: the literal spans everything
that was consumed and carries a diagnostic.

| source | tree-sitter-r | aether |
|---|---|---|
| `0x` | `float 0`, `identifier x` | `R_DOUBLE_LITERAL@0..2`, `Unterminated hexadecimal literal.` |
| `0xg` | `float 0`, `identifier xg` | `R_DOUBLE_LITERAL@0..2`, then `identifier g` |
| `0xp3` | `float 0`, `identifier xp3` | `R_DOUBLE_LITERAL@0..2`, then `identifier p3` |
| `0x1p` | `float 0x1`, `identifier p` | `R_DOUBLE_LITERAL@0..4`, `Unterminated binary exponent.` |
| `0x1p+` | `float 0x1`, `identifier p`, `+` | `R_DOUBLE_LITERAL@0..5` |
| `1e` | `float 1e` | `R_DOUBLE_LITERAL@0..2`, `Unterminated exponent.` |
| `1e+` | `float 1e+` | `R_DOUBLE_LITERAL@0..3` |
| `1east` | `float 1e`, `identifier ast` | `R_DOUBLE_LITERAL@0..2`, then `identifier ast` |

Why: the token boundaries barely change — `1east` splits the same way either
way — so this buys almost nothing in tree shape and everything in the message.
Without it, `0x` is not silently accepted either: it lexes as two expressions on
one line, and the separator rule of deviation 11 reports "Expected a newline or a
`;` between two expressions.", which is a bewildering thing to read while
half-typing `0xFF`. The lexer is the only place that still knows a hexadecimal
was started, so it is the only place that can say so.

Keeping the token rather than emitting an `ERROR_TOKEN` is deviation 6's line
held from the other side: the text is a well-formed literal that was cut short,
so the parser can still use it, and `ERROR_TOKEN` is reserved for characters that
start nothing at all. An unterminated `%...%` keeps its `SPECIAL` token for the
same reason, which the `%foo` entry below picks up.

The line is drawn at what the grammar *requires*. A suffix is optional, so a
literal that ends without one is finished, not unterminated — `1L` and `1Li` are
deviation 7's business and carry no diagnostic.

Corpus tests: `literals/numbers.md`, the unterminated cases marked `[error]`.

### 9. A backslash inside `%...%` is ordinary text

Landed in Phase 4.

Example:

```r
a %a\b% b
```

tree-sitter-r's `special` is `/%[^%\\\n]*%/`, a character class that excludes
backslash, so this errors. Their own grammar comment concedes the regex is
approximate: "This could probably be fine tuned to disallow more things."

R's `SpecialValue` in `src/main/gram.y` excludes only the newline, so `%a\b%` is
an ordinary `SPECIAL`. Aether follows R and takes the whole operator.

| source | tree-sitter-r | R and aether |
|---|---|---|
| `a %a\b% b` | `%a`, `\`, `b`, `%` — an `ERROR` node | one `SPECIAL` spanning `%a\b%` |

Why: nothing is gained by rejecting a program R accepts, and a backslash carries
no meaning inside `%...%` for the lexer to act on — there are no escapes there,
so it is one more character in the operator's name. The newline is different,
since R stops at it too.

Lexer test: `a_backslash_is_ordinary_text` in
`crates/aether_r_parser/src/lexer/tests/specials.rs`. No corpus test — every
special in `expressions/specials.md` matches tree-sitter-r's regex too, so the
block needs no marker.

### 10. Whitespace attaches trailing, not leading

Landed in Phase 5.

The one entry in this section whose counterpart is air's walker rather than
tree-sitter-r's grammar — tree-sitter-r has no notion of trivia attachment at
all. It belongs here anyway, because what this file compares is aether's tree
against the tree air's parser produces, and attachment is part of that tree.

Example:

```r
1 + 2
```

Air attaches each space as **leading** trivia of the token that follows it, so
`1` carries nothing, `+` carries `" "`, and `2` carries `" "`. Aether attaches
each space as **trailing** trivia of the token before it, so `1` carries `" "`,
`+` carries `" "`, and `2` carries nothing.

Air has a second rule on top of that one: whitespace immediately before a newline
is trailing of the preceding token, but only if some token still follows later in
the file. So the same two spaces are trailing in `1 + \n1` and leading in
`1  \n `. Aether has no such rule; both are trailing.

Round-trip holds either way — no bytes move, only which token owns them.

#### What does not change

Attachment for the other two trivia kinds is bit-identical under the two rules,
and by inspection rather than by luck:

- **Comments.** Air's rule is trailing iff no newline has been seen yet and this
  is not the first token of the file, including its deliberate choice to ignore
  the end-of-file case there so that `1 #` ends with a trailing comment.
  `JsTokenSource`'s latching bool computes the same predicate.

- **Newlines.** Leading under both. Air says so explicitly; JS latches the flag
  before pushing the newline piece rather than after.

So the deviation is confined to `Whitespace` pieces, and within those, to the
ones that do not end their line.

#### Why deviate at all

Air's rule needs a one-piece lookahead — whether a space is trailing depends on
what follows it — plus a decision that cannot be made until the gap's terminator
is known. That is about sixty lines, a held-back piece, and a fixup after the
loop. JS's rule is a latching bool and twelve lines, with nothing held back and
nothing to allocate.

Every other Biome parser uses the JS rule, so `CLAUDE.md`'s "when in doubt, do
what `biome_js_parser` does" applies directly. Air is the outlier, and it is the
outlier for no recorded reason: whitespace attachment has no user-visible
consequence, because a formatter regenerates whitespace from its own IR rather
than reading it off the source trivia.

The uniformity is worth something on its own. Under air's rule the same
whitespace-before-a-newline is trailing in `1 + \n1` and leading in `1  \n `,
which is a distinction a reader of a `cst` cell has to reconstruct from the rest
of the file. Under this one it is trailing in both.

#### Why Phase 5 and not after the swap

The cost of the change grows monotonically: it landed before the corpus existed,
where regenerating every snapshot was free, and deferring past the swap would
have added air's formatter snapshots on top.

#### Swap-in cost, and the audit behind that claim

Nothing in air reads whitespace attachment polarity. Audited at the time this
landed, rather than assumed:

- `has_leading_newline()` has six call sites in `air_r_formatter`
  (`if_statement.rs:200,217,223`, `function_definition.rs:154`,
  `parameters.rs:121`, `binary_expression.rs:470,927`). These are user-visible —
  they decide whether a binary expression breaks and whether `else` takes its own
  line. All unaffected, because the newline piece is leading under both rules.

- `lsp/src/handlers_format.rs:169` (`text_non_whitespace_range`, itself copied
  from `biome_formatter`) walks both trivia lists but skips whitespace and
  newline pieces explicitly. It is looking for comments.

- Nothing else. `air_r_syntax/src/lib.rs:322` is the `RSyntaxKind` →
  `TriviaPieceKind` mapping, and `air_r_formatter/src/comments.rs` reads no
  trivia lists at all.

The audit is an argument, not a proof, and air's formatter snapshot suite is the
experiment that settles it at swap-in. Run it before the formatter changes, so a
whitespace regression cannot hide inside the semicolon churn. If a non-semicolon
file moves, this deviation has failed its premise — and the retreat is cheap,
because restoring air's rule means rewriting one function.

One more consequence, found in Phase 13. A node holding no tokens sits wherever
the whitespace around it left the cursor, so `{ }` puts air's empty statement
list at 1 and aether's at 2. There is no text to disagree about.

### 11. Two expressions on one line is an error

Landed in Phase 7.

Example:

```r
1 2
```

R separates statements by a newline or a `;` and by nothing else, so this is a
syntax error:

```
> 1 2
Error: unexpected numeric constant in "1 2"
```

tree-sitter-r's `program` is a bare repetition of expressions with no separator
between them, so it accepts this, and so does air. Aether follows R and reports

```
Expected a newline or a `;` between two expressions.
```

at the second expression, with the first labelled as the one that already ended.

The tree is the same either way — two statements — so this is a diagnostic-only
deviation, in the same family as deviations 2 and 3.

Why follow R: the alternative is silence on a class of typo the language really
does reject, and the rule is what makes `1foo`, `0x`, and `a$1` land as two
statements with something said about them rather than two statements with
nothing said. Deviations 7 and 8 both lean on it, and their tables are written
against it.

Corpus tests: `statements/newlines.md`, and `literals/identifiers.md`'s
`Invalid Identifiers`, which is `[error]` here and clean under tree-sitter. So is
`control/if.md`'s `` Identifiers starting with `else` that are not the `else`
keyword - 2 (#200) ``, where `} else.x` puts an `if` and a name on one line.

### 11b. A `;` with no expression is an error at top level

Landed in Phase 13.

Example:

```r
;1
```

R rejects it, and rejects every other top-level `;` that has no expression
before it on the same line:

```
> ;1
Error: unexpected ';' in ";"
```

The grammar says why. `prog` reaches a `;` through one production only:

```
prog	:	END_OF_INPUT
	|	'\n'
	|	error
	|	expr_or_assign_or_help '\n'
	|	expr_or_assign_or_help ';'
	;
```

There is no bare-`;` alternative, and `expr_or_assign_or_help '\n'` is a peer of
`expr_or_assign_or_help ';'`, so whichever terminator arrives first reduces the
expression. Inside `{}` the picture is different, because `exprlist` has a
production for exactly this:

```
exprlist:	/* empty */
	|	expr_or_assign_or_help
	|	exprlist ';' expr_or_assign_or_help
	|	exprlist ';'
	|	exprlist '\n' expr_or_assign_or_help
	|	exprlist '\n'
	;
```

`exprlist ';'` over a possibly empty list admits a leading `;`, a repeated `;`,
and a `;` after a newline. Checked against R 4.6.0:

| Source | top level | inside `{}` |
|---|---|---|
| `1;` | valid | valid |
| `1;2` | valid | valid |
| `;` | `1:1: unexpected ';'` | valid |
| `;;` | `1:1: unexpected ';'` | valid |
| `;1` | `1:1: unexpected ';'` | valid |
| `1;;` | `1:3: unexpected ';'` | valid |
| `1\n;` | `2:1: unexpected ';'` | valid |

tree-sitter-r's `program` is a bare repetition and its `;` is trivia, so it
accepts every row of both columns. Aether reports `Unexpected `;`.` on the
top-level half and accepts the braced half, which is R's rule exactly.

The condition is one line to state: an `RStatement` with no expression is an
error at top level. That set is precisely the set R rejects, which is why the
rule needs no case analysis beyond the scope it is in.

Tree-only, no shape change — the `;` is still a token on a statement of its own,
so this is a diagnostic deviation in the same family as 11.

Why follow R rather than stay silent like air: the same argument as 11. A file
holding a stray `;` does not run, and saying so beats formatting it as if it
did.

Corpus tests: `statements/semicolons.md`, where four imported air snapshots are
`[error]` here and clean under tree-sitter, and the two aether-authored headings
`A Semicolon With No Expression Is An Error At Top Level` and
`A Semicolon With No Expression Is Allowed Inside Braces` pair off the two
columns of the table above.

### 12. A string is one token

Landed in the string simplification pass, after Phase 9.

Example:

```r
r"---(foo)---"
```

tree-sitter-r spells a string as a node over three children, and air's walker
carries that through: an open delimiter, an optional content, and a close. The
content is optional because an empty body has nothing to emit, so `""` has two
children where `"a"` has three:

```
RStringValue {
    open_token: STRING_OPEN@0..6 "r\"---(" [] [],
    content_token: STRING_CONTENT@6..9 "foo" [] [],
    close_token: STRING_CLOSE@9..14 ")---\"" [] [],
}
```

Aether lexes the whole thing, delimiters included, as one literal token, with a
kind per quote style:

```
RStringValue {
    value: R_RAW_STRING_LITERAL@0..14 "r\"---(foo)---\"" [] [],
}
```

`R_SINGLE_QUOTED_STRING_LITERAL`, `R_DOUBLE_QUOTED_STRING_LITERAL`, and
`R_RAW_STRING_LITERAL` are three new literal kinds; `STRING_OPEN`,
`STRING_CONTENT`, and `STRING_CLOSE` are gone. The body is reached through
`RStringValue::inner_string_text()`, with `inner_string_range()` alongside it for
consumers that want the span rather than the text. Neither unescapes anything:
`"a\nb"` comes back with a literal backslash and `n`.

Why deviate:

- **It removes a lookahead hazard rather than managing one.** A string body is
  not a regular token, so it needed a lexer context, and `BufferedLexer` lexes
  its lookahead in the default context — `p.nth(n)` past a `STRING_OPEN` came
  back with the string body lexed as code. That was survivable only because the
  lexer deliberately kept no string state, an invariant that existed for no other
  reason. One-token strings delete the hazard outright: `RLexContext` is down to
  its `Regular` variant, so there is no non-regular context left to be wrong
  about, and `BumpWithContext` and `do_bump_with_context` go with it.

- **The optional slot stops existing.** `content: 'string_content'?` was optional
  only because an empty body emits nothing. A required `value` slot says what is
  true.

- **It is what the rest of the lexer already does.** A backtick-quoted name is
  one `IDENT` scanned to its close. Strings were the odd one out, and
  `biome_js_parser`'s `JS_STRING_LITERAL` is one token including its quotes.

Three kinds rather than one: the lexer already knows which delimiter it matched,
so the kind is free; a `cst` cell reads as `R_RAW_STRING_LITERAL@0..14` without
the reader parsing the text; and air's eventual quote-style option gets a kind
check rather than a first-byte sniff. Three *node* types under an
`AnyRStringValue` union were rejected — same information, but a union level
inside `AnyRValue`, `AnyRSelector`, and `AnyRArgumentName`, three more generated
formatter files, and a break to air's `call_ext.rs` match on
`AnyRValue::RStringValue(_)`.

Swap-in cost, smaller than the file count suggests. `air_r_parser`'s `parse.rs`
and `treesitter.rs` are deleted by the swap anyway — their
`handle_string_value_enter()` / `handle_string_content_enter()` walker is
precisely what aether replaces. What is left:

- `xtask/codegen/r.ungram` and `xtask/codegen/src/r_kinds_src.rs`, then air's
  codegen.

- `air_r_syntax/src/lib.rs` — the two exhaustive `is_bogus()` / `to_bogus()`
  matches. Every token kind is listed there, so this is three lines out and
  three in rather than anything about strings.

- `air_r_formatter/src/r/auxiliary/string_value.rs` — three writes become one.
  Field names, not kinds: `RStringValueFields` destructures to `{ value }`.

- `air_r_formatter/src/string_content.rs` — **the one that needs reading rather
  than editing.** See below.

- `air_r_syntax/src/call_ext.rs` — unchanged, which is one of the reasons for one
  node type. So are the three `AnyR*::RStringValue(node) => node.format()` arms
  in the formatter's `r/any/`. Everything in air that is semantic about a string
  goes through the node, which is the whole point.

Air's formatter specs are R-in / R-out, so no snapshot churn there.

#### `string_content.rs` inherits an invariant this breaks

Normalizing the whole token rather than the content token is identical in
effect, since no delimiter contains `\r\n`. The hazard is trivia, not
normalization.

`FormatStringContentToken` asserts that its token has none:

```rust
debug_assert!(
    token.text() == token.text_trimmed(),
    "String content tokens should never have trivia. Trivia should be on string open or string close tokens instead."
);
```

That is false by construction here. There is no open token left to hold the
trivia, so the one literal token owns the string's leading and trailing pieces.

Deleting the assert is not enough, because the code depends on it.
`normalize_string_content_token()` reads `token.text()`, while `fmt()` passes
`token.text_trimmed_range().start()` to `syntax_token_cow_slice()` as the offset
that text sits at. The two agree only while the assert holds. With trivia
present, `syntax_token_cow_slice()`'s borrowed-case check

```rust
let range = TextRange::at(self.start, text.text_len());
debug_assert_eq!(*text, &self.token.text()[range - self.token.text_range().start()], …);
```

fires in debug and slices the wrong bytes in release. **`normalize_string_content_token()`
must read `text_trimmed()`.** Do that in the same commit as the assert, not
after it.

Unterminated strings are deviation 6's business, not this entry's.

### 13. `AnyRParameterName` gains `RBogusParameterName`

Landed in Phase 11.

`AnyRParameterName` had three variants and no bogus one:

```rust
pub enum AnyRParameterName {
    RDotDotI(RDotDotI),
    RDots(RDots),
    RIdentifier(RIdentifier),
}
```

Example:

```r
function(1, b) x
```

This is deviation 5 one level deeper, and the collapse it prevents is worse.
`RParameterList` is a separated list generated with `RParameter::can_cast`, so a
child that is not an `RParameter` makes the whole list invalid. Without a bogus
name the chain runs: the bogus node fails the `name` slot, so `R_PARAMETER`
degrades to `R_BOGUS`; `R_BOGUS` is not an `RParameter`, so `R_PARAMETER_LIST`
degrades to `R_BOGUS`; that is not an `RParameterList`, so `R_PARAMETERS` goes
too, and one bad token takes the entire definition with it — parameters, body,
and all.

With the kind, the damage stops at the name:

```
RFunctionDefinition {
    name: FUNCTION_KW,
    parameters: RParameters {
        items: RParameterList [
            RParameter { name: RBogusParameterName { 1 } },
            RParameter { name: RIdentifier { b } },
        ],
    },
    body: RIdentifier { x },
}
```

`R_BOGUS_PARAMETER_NAME` is a new node kind, added after `R_BOGUS_SELECTOR` so
nothing renumbers. `is_bogus()` returns true for it and `to_bogus()` is
idempotent; the three real name kinds keep their existing mappings, for the reason
deviation 5 gives.

Why a new kind rather than reusing `RBogusSelector`, which `AnyRSelector` already
has and which is nearly the same set of tokens: the two positions accept
different things — a string is a selector and is not a parameter name — and
Biome's convention is one bogus kind per restricted position. A tree walker that
sees `R_BOGUS_PARAMETER_NAME` knows a parameter was malformed without looking at
its parent.

**One path constructs one**, in `parse_parameter`: a name that is absent where the
list has not ended is diagnosed with `expected_parameter_name` and the tokens up to the
next boundary are recovered into it. A name that is absent *at* a boundary is the
hole case, `function(a,,b)`, which is diagnosed the same way and consumes nothing,
so the empty `RParameter` stays empty.

Swap-in cost is deviation 5's, one entry down: `RSyntaxKind`, `nodes.rs`, and both
factories are generated, leaving `is_bogus()`, `to_bogus()`, one empty formatter
rule, and a fourth arm in `FormatAnyRParameterName`.

Corpus tests: `functions/parameters.md`'s `A Parameter That Is Not A Name` and
`A Missing Comma Between Parameters`, both `[error]`.

### 14. The `for` variable takes any name, and gains `RBogusForVariable`

Landed in Phase 12.

`RForStatement` typed its variable slot as `RIdentifier`:

```ungram
RForStatement =
	'for'
	'('
	variable: RIdentifier
	'in'
	sequence: AnyRExpression
	')'
	body: AnyRExpression
```

tree-sitter-r types the same field as `_identifier`, which is `identifier`,
`dots`, *and* `dot_dot_i`, and R agrees — this runs, and prints `1` then `2`:

```r
for (... in 1:2) print(get("..."))
```

So air's walker faithfully hands `R_DOTS` to the factory, the factory rejects it,
and deviation 5's collapse runs all the way to the top. There is no `R_ROOT` left:

```
R_BOGUS
  R_BOGUS
    R_BOGUS
      FOR_KW, L_PAREN, R_DOTS, IN_KW, R_BINARY_EXPRESSION, R_PAREN, R_CALL
  EOF
```

No diagnostic comes with it. `air_r_parser::parse().error()` is `None`, so this is
not a parse air knows went wrong — it is valid R that silently loses its tree, and
every consumer downstream of the formatter sees one bogus node where a file used
to be.

Aether widens the slot instead:

```ungram
AnyRForVariable =
	RIdentifier
	| RDots
	| RDotDotI
	| RBogusForVariable
```

The bogus variant is deviations 5 and 13 one position further along, and it is
what `for (1 in x) y` recovers into. Without it the slot has no kind that fits, so
a variable that is not a name would take `R_FOR_STATEMENT` with it, and with it the
damage stops at the variable:

```
RForStatement {
    variable: RBogusForVariable { 1 },
    sequence: RIdentifier { x },
    body: RIdentifier { y },
}
```

`R_BOGUS_FOR_VARIABLE` is a new node kind, added after `R_BOGUS_PARAMETER_NAME` so
nothing renumbers. `is_bogus()` returns true for it and `to_bogus()` is idempotent.

Why not reuse `RBogusParameterName`, whose three real variants are the same three:
the positions are still distinct — deviation 13's reasoning applies unchanged, and
a walker that sees `R_BOGUS_FOR_VARIABLE` knows a loop variable was malformed
without looking at its parent.

**One path constructs one**, in `parse_for_statement`: a variable that is absent is
diagnosed with `expected_for_variable` and the tokens up to the `in` are recovered
into it. A variable that is absent *at* the `in`, as in `for (in x) y`, is
diagnosed the same way and consumes nothing, so the slot is simply left missing.

Swap-in cost is deviation 13's, one position down: `RSyntaxKind`, `nodes.rs`, and
both factories are generated, leaving `is_bogus()`, `to_bogus()`, one empty
formatter rule, and a `FormatAnyRForVariable` union rule beside the
`AnyRParameterName` one. `r/statements/for_statement.rs` writes `variable.format()`
and needs no edit.

Corpus tests: `control/for.md`'s `` `...` And `..i` As The `for` Variable ``,
whose tree cleanly disagrees with the bogus root above — air collapses the whole
file where aether does not. `A `for` Variable That Is Not A Name` and `A `for`
With No Variable` cover the bogus kind and need no marker, being `[error]`. The
imported `For` test exercises `...` and `..i` too, and disagrees with
tree-sitter-r's tree for the same reason.

### 15. `OperatorPrecedence` and `call_ext.rs` are not carried over

Phase 2 vendored `air_r_syntax`'s hand-written files wholesale, on the reasoning
that "air's formatter uses them, so they must be here for the swap-in to work."
This entry narrows that: two of them are formatter-only and gain nothing from
living in the parser's own crate ahead of time.

`OperatorPrecedence` duplicated `crates/aether_r_parser/src/syntax/binding_power.rs`'s
`BindingPower` under different names for the same token set, unused by anything
in `aether_r_parser` — the Pratt loop has its own binding power table and never
called `try_from_binary_operator()`. `call_ext.rs`'s `RCall::is_test_call()` is
formatter policy (recognizing `test_that()`-shaped calls to format their brace
argument specially), with no caller anywhere in the parser or its tests.

Both are deleted from `aether_r_syntax`, not deprioritized: `argument_ext.rs` and
`string_ext.rs` stay, since the parser or its tests reach through them today.

Why: keeping formatter-only code in the parser crate ahead of the formatter's own
existence means maintaining it against a moving target with no test coverage to
catch drift. `BindingPower`'s reordering under deviation work would have to be
mirrored into `OperatorPrecedence` by hand, unverified, for a table that would go
untouched until air's formatter port begins.

Swap-in cost: whoever ports air's formatter recreates both from air's original
`air_r_syntax` source — this is a straight vendor, not a rewrite, since aether's
`RSyntaxKind` and node shapes are otherwise unchanged from air's for every token
and node either file touches.

## tree-sitter-r vs. R

Places where tree-sitter-r already disagrees with R's own parser. Aether follows
tree-sitter regardless, since these are inherited quirks rather than aether's own
choices; they are logged here so nobody has to rediscover them.

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

Note that aether does copy R for the neighbouring case, an unterminated `%...%` cut
short by a newline: R ungets the newline before reporting, so the error stays
local and the next line parses normally. Aether stops before the newline too,
keeping the partial `%foo` as a `SPECIAL` token with the diagnostic attached.
That is not a deviation from tree-sitter, just a recovery choice tree-sitter has
no equivalent of.
