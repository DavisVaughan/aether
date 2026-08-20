# Deviations

Every intentional difference between aether's tree and the tree produced by
`posit-dev/air`'s tree-sitter-based parser, with an example, both outputs, and
the reason.

Two categories, kept apart on purpose:

- [Aether vs. tree-sitter-r](#aether-vs-tree-sitter-r) — choices aether made, and
  can revisit.

- [tree-sitter-r vs. R](#tree-sitter-r-vs-r) — places where tree-sitter-r already
  differs from R and aether follows tree-sitter anyway, so the oracle agrees.
  Upstream behaviour we are deliberately not fixing.

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
| 6 | A character that starts no token | `ERROR` node | new `ERROR_TOKEN` kind added, reported by the lexer and wrapped in a bogus node by the parser |
| 7 | `L` and `i` suffixes | tokens of their own, so `1 L` is one `integer` and `1Li` is `1` and `Li` | part of the literal, taken whatever follows, as R takes them |
| 8 | Unterminated numbers | `0x`, `0x1p`, `1e` re-split or accepted, all silently | one literal token carrying `Unterminated hexadecimal literal.`, `Unterminated binary exponent.`, or `Unterminated exponent.` |
| 9 | A backslash inside `%...%` | excluded by the `special` regex, so `%a\b%` errors | ordinary text, as R takes it, so `%a\b%` is one `SPECIAL` |
| 10 | Whitespace attachment | air's walker: leading of the following token, with a second rule for whitespace before a newline | `JsTokenSource`'s rule — trailing of the preceding token until a newline, leading from the newline on. Comment and newline attachment are unchanged |
| 11 | Two expressions on one line | accepted silently, since `program` needs no separator | an error, as R requires |

### 1. Semicolons are real tokens

Grammar landed in Phase 3; the parser fills the slot in Phase 13, so the trees
below are the target rather than what aether builds today.

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

- `a$1`, `a$NA`, `a$NULL`, `a$TRUE`, `a$(b)`, `a${b}`, `a$if` → identical
  treatment. The RHS is present but is not a valid selector, so the slot is left
  missing and `expected_selector` labels it. **Nothing is consumed**, so the `1`,
  the `(b)`, or the whole `if` statement goes on to be parsed as the next
  statement, exactly as under tree-sitter.

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

Corpus tests: `Dollar No RHS`, `Slot No RHS`, and `Extract With A Literal RHS` in
`expressions/extract.md`. All three carry `[deviation]` today, and the shape
matches, so expect the marker to come off once the oracle confirms it — leaving
`[error]`.

### 3. Namespace RHS is required

Lands in Phase 10.

Identical to deviation 2, for `::` and `:::`. Split out because tree-sitter-r
models namespace as its own rule with its own `optional()`.

Corpus tests: `Namespace With Missing RHS` and `Namespace With A Literal RHS` in
`expressions/namespace.md`, both carrying `[deviation]` today and both expected
to shed it for the reason given in deviation 2.

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

- The kind alone identifies the position. A tree walker seeing `R_BOGUS_SELECTOR`
  knows a selector was malformed without inspecting the parent.

**Nothing constructs one.** Phase 10 was going to, by consuming a non-selector RHS
into a bogus node; deviation 2 settles that it diagnoses and consumes nothing, so
the kind is unproduced. It stays because the argument above does not depend on
having a producer, and because removing it means renumbering nothing but churning
codegen for no gain — but do not read its presence as evidence that some path
builds one.

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

### 6. A character that starts no token becomes an `ERROR_TOKEN`

Landed in Phase 4 as a new token kind in `r_kinds_src.rs`; Phase 7 added the
`parse_primary_expression` arm that wraps it.

Example:

```r
€
```

tree-sitter-r produces an `ERROR` node. Aether reports the character with an
`Unexpected character` diagnostic and hands it on as an `ERROR_TOKEN`, which the
parser can only wrap in a bogus node. The file never parses cleanly, so the
oracle skips it, and the character survives into the tree where a reader can see
it.

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
is lost by disagreeing: the oracle only compares trees for inputs aether parses
cleanly, and it skips every one of these either way.

Why follow R rather than the oracle: R's rule cannot disagree with R about a
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
block is compared rather than marked `[deviation]`.

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
have added air's formatter snapshots on top. The oracle lost no coverage either
— see below — because the drift worth watching for is in comment attachment,
which this does not touch.

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

#### Oracle

**No test carries `[deviation]` for this**, which makes it the first entry here
handled entirely by normalization. Air's walker produces air's polarity, so
without normalization every corpus file containing a space would mismatch — which
is every file, and the oracle would be useless from Phase 5 onward.

So the oracle re-tags `Whitespace` pieces on both trees by the same rule before
comparing. **Whitespace only, never comments:** comment attachment is unchanged
here, and it is the half that reaches air's formatter, so it is exactly the half
the oracle has to keep checking. Normalizing over "trivia" rather than over
whitespace would blind it to the one attachment drift that has consequences.

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
deviation, in the same family as deviations 2 and 3. What it costs is that every
input of this shape stops being oracle-compared, because the oracle skips
anything aether does not parse cleanly.

Why follow R: the alternative is silence on a class of typo the language really
does reject, and the rule is what makes `1foo`, `0x`, and `a$1` land as two
statements with something said about them rather than two statements with
nothing said. Deviations 7 and 8 both lean on it, and their tables are written
against it.

Corpus tests: `statements/newlines.md`, and `literals/identifiers.md`'s
`Invalid Identifiers`, which is `[error]` here and clean under tree-sitter.

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

Note that aether does copy R for the neighbouring case, an unterminated `%...%` cut
short by a newline: R ungets the newline before reporting, so the error stays
local and the next line parses normally. Aether stops before the newline too,
keeping the partial `%foo` as a `SPECIAL` token with the diagnostic attached.
That is not a deviation from tree-sitter, just a recovery choice tree-sitter has
no equivalent of.
