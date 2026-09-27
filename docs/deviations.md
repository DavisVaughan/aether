# Deviations

Aether produces the same Rowan tree as air's tree-sitter-based parser, except
where this file says otherwise. Everything below is deliberate.

It's written for whoever updates air or ark to sit on top of aether. Most
entries are diagnostics only: same tree, more errors reported. The ones that
actually change the tree, and so need code changes downstream, are semicolons
and statements, strings, `|` raw strings, the new bogus node kinds,
`ERROR_TOKEN`, the `for` variable, and whitespace attachment.

The last section is different: places where tree-sitter-r already disagrees with
R and aether follows tree-sitter anyway. Inherited quirks, logged so nobody
rediscovers them.

## Semicolons are real tokens

Air maps `;` to a whitespace trivia piece, so it isn't a token at all:

```
1;2

RDoubleValue { value_token: R_DOUBLE_LITERAL@0..1 "1" [] [] },
RDoubleValue { value_token: R_DOUBLE_LITERAL@1..3 "2" [Whitespace(";")] [] },
```

A token buried in a trivia piece can't be `format_removed()`, so air's formatter
can never drop or rewrite a semicolon. Aether lexes it as `SEMICOLON` and hangs
it off the statement before it:

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

Both of `RStatement`'s slots are optional, the same trick `RArgument` uses,
because R allows `;;` and a leading `;` inside `{}`:

```r
;        # RStatement { expression: missing, semicolon: ";" }
1        # RStatement { expression: 1,       semicolon: missing }
1;       # RStatement { expression: 1,       semicolon: ";" }
```

A statement with neither slot filled is never produced.

A `;` belongs to the expression before it only when no newline intervenes. R's
`prog` offers `expr '\n'` and `expr ';'` as alternatives, so whichever terminator
arrives first is the one that ends the expression: `1;` is one expression, `1\n;`
is `2:1: unexpected ';'`. So a `;` on a new line gets an `RStatement` of its own.
That also leaves any blank line above it in the statement's leading trivia,
rather than inside a token the formatter is about to delete.

Agreed formatting: remove the semicolon and hard-break the line, so `1;2` becomes
two lines.

### The list node changed with it

Once every element carries an optional semicolon it's a list of statements, not
of expressions, so `RExpressionList` is replaced rather than kept alongside:

```diff
 RRoot =
 	bom: 'UNICODE_BOM'?
-	expressions: RExpressionList
+	statements: RStatementList
 	eof: 'EOF'
```

`RBracedExpressions` becomes `RBracedStatements` for the same reason.

For air, that means:

- Iterating an `RStatementList` yields `RStatement`, not `AnyRExpression`. Every
  walk gains a `.expression()` hop to reach the payload.

- `RRoot::statements()` and `RBracedStatements::statements()` replace
  `.expressions()`.

- `RStatement::semicolon_token()` is the hook for dropping the `;`.

- The `AnyRExpression::RBracedExpressions` variant is renamed, and the formatter
  rule file `r/expressions/braced_expressions.rs` becomes `braced_statements.rs`.
  Air's codegen derives that path from the node name, so re-running it generates
  the new stub; move the hand-written body across by hand.

Two alternatives were rejected. Semicolons can't stay trivia under a dedicated
`TriviaPieceKind`, because that's a Biome enum and we don't fork it. Putting a
`;` slot on every expression node works, but touches every expression node in the
grammar.

## A string is one token

tree-sitter-r spells a string as a node over three children (an open delimiter,
an optional content, and a close), and air's walker carries that through:

```
RStringValue {
    open_token: STRING_OPEN@0..6 "r\"---(" [] [],
    content_token: STRING_CONTENT@6..9 "foo" [] [],
    close_token: STRING_CLOSE@9..14 ")---\"" [] [],
}
```

Aether lexes the whole thing, delimiters included, as one token:

```
RStringValue {
    value: R_RAW_STRING_LITERAL@0..14 "r\"---(foo)---\"" [] [],
}
```

`R_SINGLE_QUOTED_STRING_LITERAL`, `R_DOUBLE_QUOTED_STRING_LITERAL`, and
`R_RAW_STRING_LITERAL` are new; `STRING_OPEN`, `STRING_CONTENT`, and
`STRING_CLOSE` are gone. The body is reached through
`RStringValue::inner_string_text()`, which unescapes nothing: `"a\nb"` comes back
with a literal backslash and `n`.

Why:

- A string body isn't a regular token, so the three-token form needed a lexer
  context, and `BufferedLexer` lexes its lookahead in the default context.
  `p.nth(n)` past a `STRING_OPEN` came back with the body lexed as code. One
  token deletes the hazard: `RLexContext` is down to `Regular`, and
  `BumpWithContext` is gone with it.

- The optional content slot existed only because an empty body emits nothing. A
  required `value` slot says what's true.

- It's what the rest of the lexer already does. A backtick-quoted name is one
  `IDENT` scanned to its close, and `biome_js_parser` does the same with
  `JS_STRING_LITERAL`.

Three kinds rather than one because the lexer already knows which delimiter it
matched, so the kind is free, a `cst` cell reads as `R_RAW_STRING_LITERAL@0..14`
without anyone parsing the text, and air's eventual quote-style option gets a
kind check instead of a first-byte sniff. Three *node* types under an
`AnyRStringValue` union were rejected: same information, but an extra union level
inside `AnyRValue`, `AnyRSelector`, and `AnyRArgumentName`, three more generated
formatter files, and a break to air's `call_ext.rs`.

For air, this is smaller than the file count suggests. `air_r_parser`'s
`parse.rs` and `treesitter.rs` go away with the swap anyway. Their
`handle_string_value_enter()` / `handle_string_content_enter()` walker is exactly
what aether replaces. What's left is `r.ungram` and `r_kinds_src.rs` plus
codegen, the two exhaustive matches in `air_r_syntax/src/lib.rs`,
`string_value.rs` (three writes become one, and `RStringValueFields`
destructures to `{ value }`), and `string_content.rs`, which is covered below.
`call_ext.rs` and the `AnyR*::RStringValue(node) => node.format()` arms are
untouched, which was one of the reasons for a single node type.

### `string_content.rs` needs reading, not just editing

`FormatStringContentToken` asserts its token carries no trivia:

```rust
debug_assert!(
    token.text() == token.text_trimmed(),
    "String content tokens should never have trivia. ..."
);
```

That's false by construction now. There's no open token left to hold the trivia,
so the one literal token owns the string's leading and trailing pieces.

Deleting the assert isn't enough, because the code leans on it.
`normalize_string_content_token()` reads `token.text()`, while `fmt()` passes
`token.text_trimmed_range().start()` to `syntax_token_cow_slice()` as the offset
that text sits at. Those two agree only while the assert holds. With trivia
present, `syntax_token_cow_slice()` fires its own debug assert and slices the
wrong bytes in release. So `normalize_string_content_token()` must read
`text_trimmed()`, in the same commit that removes the assert.

Normalizing the whole token rather than just the content is otherwise identical
in effect, since no delimiter contains `\r\n`.

## `|` opens a raw string

R accepts `|` as a raw string delimiter alongside `(`, `[`, and `{`. `?Quotes`
doesn't mention it, but `RawStringValue()` in `src/main/gram.y` accepts it:

```
> r"-|a|b|-"
[1] "a|b"
```

tree-sitter-r only knows the three brackets, so it reads `r"|a|"` as the name `r`
followed by the string `"|a|"`:

```
(program
  (identifier)
  (string))
```

Aether follows R and lexes one `R_RAW_STRING_LITERAL`.

## Whitespace attaches to the token before it

Air attaches each space as *leading* trivia of the token that follows, so in
`1 + 2` the `1` carries nothing, `+` carries `" "`, and `2` carries `" "`. Aether
attaches each space as *trailing* trivia of the token before it, so `1` carries
`" "`, `+` carries `" "`, and `2` carries nothing.

Air also has a second rule: whitespace immediately before a newline is trailing,
but only if some token still follows later in the file. So the same two spaces
are trailing in `1 + \n1` and leading in `1  \n `. Aether has no such rule, and
both are trailing.

Comments and newlines attach identically under both rules, by inspection rather
than luck. Air's comment rule is "trailing iff no newline seen yet and this isn't
the first token of the file", including its deliberate choice to ignore the
end-of-file case so `1 #` ends with a trailing comment. `JsTokenSource`'s
latching bool computes the same predicate. Newlines are leading under both. So
the deviation is confined to whitespace pieces that don't end their line.

Why deviate: air's rule needs a one-piece lookahead and a decision that can't be
made until the gap's terminator is known. That's about sixty lines, a held-back
piece, and a fixup after the loop. The JS rule is a latching bool and twelve
lines. Every other Biome parser uses the JS rule; air is the outlier for no
recorded reason. And whitespace attachment has no user-visible consequence,
because a formatter regenerates whitespace from its own IR rather than reading it
off the source trivia.

Nothing in air reads attachment polarity. Audited rather than assumed:

- `has_leading_newline()` has six call sites in `air_r_formatter`
  (`if_statement.rs:200,217,223`, `function_definition.rs:154`,
  `parameters.rs:121`, `binary_expression.rs:470,927`). These are user-visible:
  they decide whether a binary expression breaks and whether `else` takes its own
  line. All unaffected, since newline pieces are leading under both rules.

- `lsp/src/handlers_format.rs:169` walks both trivia lists but explicitly skips
  whitespace and newline pieces. It's looking for comments.

- Nothing else. `air_r_syntax/src/lib.rs:322` is the kind to `TriviaPieceKind`
  mapping, and `air_r_formatter/src/comments.rs` reads no trivia lists at all.

That audit is an argument, not a proof. Air's formatter snapshot suite is the
experiment that settles it. Run it *before* changing the formatter, so a
whitespace regression can't hide inside the semicolon churn. If a non-semicolon
file moves, this deviation has failed, and the retreat is one function.

One small consequence: a node holding no tokens sits wherever the surrounding
whitespace left the cursor, so `{ }` puts air's empty statement list at offset 1
and aether's at 2. There's no text to disagree about.

## Recovery is local, not whole-file

```r
f(
1 + 1
```

A parse failure anywhere makes air produce one bogus node spanning the entire
file, and the LSP shows a single whole-file error. Nothing is walkable.

Aether puts bogus nodes at the smallest node that failed and keeps going, so the
rest of the file still produces real nodes and each error carries its own range.
This is the most user-visible win of the whole project and belongs in air's
changelog. Every consumer of the tree (formatter, LSP, lint rules) goes from "no
information" to "information everywhere except the broken span".

## Three new bogus node kinds

`AnyRSelector`, `AnyRParameterName`, and `AnyRForVariable` are restricted unions
with no bogus variant, which sounds harmless and isn't. On a failed cast the
generated factory doesn't panic. It degrades the node, and the degradation
climbs. `R_NAMESPACE_EXPRESSION.to_bogus()` is `R_BOGUS_EXPRESSION`, so `a::b::c`
collapses the whole namespace expression instead of containing the damage in the
`left` slot. Round-trip still passes, so nothing in the test harness notices; you
only see it by reading a snapshot.

Parameters are worse, because `RParameterList` is a separated list generated with
`RParameter::can_cast`. For `function(1, b) x` the chain runs: bogus name fails
the `name` slot, so `R_PARAMETER` degrades to `R_BOGUS`; that isn't an
`RParameter`, so `R_PARAMETER_LIST` degrades; that isn't an `RParameterList`, so
`R_PARAMETERS` goes too. One bad token takes the whole definition: parameters,
body, and all.

So aether adds `RBogusSelector`, `RBogusParameterName`, and `RBogusForVariable`,
and the damage stops at the slot:

```
a::b::c

RNamespaceExpression {
    left: RBogusSelector { RNamespaceExpression { a::b } },
    operator: COLON2,
    right: RIdentifier { c },
}
```

```
function(1, b) x

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

Three kinds rather than one shared one, even though the accepted token sets
overlap heavily: the positions accept different things (a string is a selector
but not a parameter name), Biome's convention is one bogus kind per restricted
position (`js.ungram` has ten), and a walker seeing `R_BOGUS_PARAMETER_NAME`
knows what broke without inspecting the parent.

The real node kinds keep their existing `to_bogus()` mappings. `to_bogus()`
answers "this node kind failed to build, what does it degrade to", which is about
the node, not the slot it sits in. `R_IDENTIFIER` and `R_STRING_VALUE` appear in
plenty of non-selector positions.

Each kind has exactly one construction site:

- `RBogusSelector`: the *left* of `::` and `:::`, never the right. The operand is
  already parsed by the time the operator is read, so containing it costs no
  tokens.

- `RBogusParameterName`: in `parse_parameter()`, when a name is absent and the
  list hasn't ended. Tokens up to the next boundary are recovered into it. A name
  absent *at* a boundary is the hole case, `function(a,,b)`, which is diagnosed
  the same way but consumes nothing, so the empty `RParameter` stays empty.

- `RBogusForVariable`: in `parse_for_statement()`, same shape: tokens up to the
  `in` are recovered into it, and `for (in x) y` consumes nothing and leaves the
  slot missing.

The three kinds are appended before `__LAST`, so nothing renumbers. For air,
`RSyntaxKind`, `nodes.rs`, and both factories are generated, so they follow from
air's own codegen. The hand-written work is `is_bogus()` and `to_bogus()` in
`air_r_syntax/src/lib.rs`, plus one formatter rule file per kind. That's nearly
free, since air's bogus rules are empty impls its codegen stubs out:

```rust
impl FormatBogusNodeRule<RBogusSelector> for FormatRBogusSelector {}
```

Each union's format rule gains one more arm in the same generated pass.

## The `for` variable takes `...` and `..i`

Air types the variable slot as `RIdentifier`. tree-sitter-r types the same field
as `_identifier`, which is `identifier`, `dots`, *and* `dot_dot_i`, and R agrees.
This runs and prints `1` then `2`:

```r
for (... in 1:2) print(get("..."))
```

Air's walker faithfully hands `R_DOTS` to the factory, the factory rejects it,
and the collapse above runs all the way to the top. There's no `R_ROOT` left:

```
R_BOGUS
  R_BOGUS
    R_BOGUS
      FOR_KW, L_PAREN, R_DOTS, IN_KW, R_BINARY_EXPRESSION, R_PAREN, R_CALL
  EOF
```

And no diagnostic comes with it: `air_r_parser::parse().error()` is `None`. This
is valid R that silently loses its tree.

Aether widens the slot:

```ungram
AnyRForVariable =
	RIdentifier
	| RDots
	| RDotDotI
	| RBogusForVariable
```

`r/statements/for_statement.rs` writes `variable.format()` and needs no edit.

## `ERROR_TOKEN` for text nothing can parse

```r
€
"abc
`abc
```

tree-sitter-r produces an `ERROR` node. Aether reports each of these from the
lexer (`Unexpected character`, `Unterminated string.`, and
`Unterminated backtick-quoted name.`) and hands the text on as an `ERROR_TOKEN`,
which the parser can only wrap in a bogus node. The file never parses cleanly,
and the text survives into the tree where a reader can see it.

Dropping the character isn't an option, since the tree has to round-trip, and
folding it into trivia would make it silently disappear exactly the way
semicolons used to.

Why a new kind rather than reusing `IDENT`: `IDENT` is accepted wherever a name
is, so `x <- €` would build a well-formed binary expression with an
`R_IDENTIFIER` on the right. The diagnostic would be there, but the tree would
assert that `€` is a name, and round-trip passes either way so nothing would
catch it. `ERROR_TOKEN` starts no production, so the damage is contained by
construction. Every other Biome grammar declares this kind: js, css, json,
graphql, grit, html, markdown, yaml. Air managed without it only because air has
no lexer.

The reachable cases are narrow: every printable ASCII character starts some R
token, so this is control characters and non-ASCII characters that are neither
`XID_Start` nor `XID_Continue`.

### Where the line falls

Not every diagnosed token becomes an `ERROR_TOKEN`. The test is whether the lexer
knew where the token ended.

- Bounded. `0x`, `1e`, and `%a` stop where the literal stops or at the next
  newline, so the extent is right and only the content is malformed. These keep
  their kind and carry the diagnostic. A parser that goes on to use such a token
  isn't being lied to about its span.

- Runs to EOF. `consume_string()` and `consume_backtick_identifier()` have no
  line-based stop, so `x <- "abc` followed by five hundred lines produces one
  token covering all five hundred. A well-formed kind there would assert that the
  rest of the file is a string. `biome_js_parser` draws the same line:
  `read_str_literal` returns `ERROR_TOKEN` when `consume_str_literal()` fails.

Recovery doesn't suffer, since these tokens eat the rest of the file and there's
nothing after them to recover into.

The cost, recorded honestly: the tree stops recording *which* construct broke,
because the `ERROR_TOKEN` arm builds `R_BOGUS_EXPRESSION` rather than
`R_BOGUS_VALUE`. The diagnostic still names it, so a human loses nothing and a
tree walker does. Nothing downstream is affected today, since air refuses to
format any file with parse errors and ark reads tree-sitter directly.

For air, `RSyntaxKind` is generated, so the kind follows from codegen once
`r_kinds_src.rs` carries it. The hand-written work is the `is_bogus()` /
`to_bogus()` arms. `ERROR_TOKEN` isn't bogus itself and degrades to `R_BOGUS`
like every other token. The formatter needs nothing, since `FormatBogusNodeRule`
prints verbatim and never inspects the kinds inside. Unlike the bogus node kinds
this one does renumber, since token kinds precede node kinds in the generated
enum, but nothing depends on the numbering.

## `$`, `@`, `::`, and `:::` require a right-hand side

Same tree as air, more diagnostics.

tree-sitter-r declares the RHS of `extract_operator` as `optional()` on purpose.
Its grammar comment says this is so `dplyr::` still yields a namespace node with
a known package name for completions. Air's `r.ungram` already declares the slot
required, so `a$` walks to an `RExtractExpression` with `right: missing
(required)` and no diagnostic at all. That test lives in air's
`tests/snapshots/undefined/` precisely because nobody was happy with it.

Aether needs no grammar change; the deviation is in the parser.

- `a$b`, `a$"b"`, `a$...`, `a$..1` produce a clean `R_EXTRACT_EXPRESSION`.

- `a$` produces an `R_EXTRACT_EXPRESSION` with `right` missing and an
  `expected_rhs_selector` diagnostic just past the operator, plus a secondary
  label on the operator. The message names the operator, so `$`, `@`, `::`, and
  `:::` each read differently:

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

- `a$1`, `a$NA`, `a$NULL`, `a$TRUE`, `a$(b)`, `a${b}`, `a$if` get the same
  treatment. The RHS is present but isn't a valid selector, so the slot is left
  missing and labelled. Nothing is consumed, so the `1`, the `{b}`, or the whole
  `if` statement goes on to be parsed as the next statement, exactly as under
  tree-sitter. `a$(b)` is the one that doesn't become a sibling: `(` is postfix,
  so both parsers read a call whose function is the incomplete extract
  expression.

`missing (required)` with no diagnostic is a silent hole in the tree. The
completions argument behind tree-sitter's `optional()` is served just as well by
an `R_EXTRACT_EXPRESSION` that carries a diagnostic, since the `left` slot is
still there to complete from.

Nothing is consumed because the imported corpus argues for it in its own
comments. On `foo${bar}`: "we nicely get a true node here alongside the braces …
it's useful for completions and highlighting". On `foo$if`: "this is actually
pretty decent behavior". Swallowing the RHS into a bogus node destroys exactly
that. And `$` sits in R's `EatLines` set, so `df$` at the end of a line would eat
the *next* line's first construct, while `df$` is the language's completion
trigger and its most common incomplete state.

`::` and `:::` add two wrinkles of their own.

A newline counts as a missing RHS here and nowhere else. They're the only
operators absent from R's `EatLines` switch, so `foo::\nbar` is two expressions
rather than one, and aether says so where tree-sitter-r stays quiet. The
restriction lifts inside `(`, `[`, and `[[`, where R discards newlines before
`EatLines` ever runs, so `f(stats::\nmedian)` is one clean namespace expression
in both parsers.

Their *left* side is restricted too, since `RNamespaceExpression` takes an
`AnyRSelector` where `RExtractExpression` takes any expression. `a::b::c` and
`a()::b` are errors under R and tree-sitter-r as well, so the deviation is only
in the recovery: air collapses the file to one bogus node, aether wraps the
operand in an `RBogusSelector` and keeps the namespace node, its operator, and
its right-hand name.

## Two expressions on one line is an error

R separates statements by a newline or a `;` and nothing else:

```
> 1 2
Error: unexpected numeric constant in "1 2"
```

tree-sitter-r's `program` is a bare repetition with no separator, so it accepts
this, and so does air. Aether reports

```
Expected a newline or a `;` between two expressions.
```

at the second expression, with the first labelled as the one that already ended.
The tree is the same either way, two statements, so this is diagnostics only.

The alternative is silence on a class of typo the language really does reject.
This rule is also what makes `1foo`, `0x`, and `a$1` land as two statements with
something said about them, so the two number entries below lean on it.

## A stray `;` is an error at top level, fine inside `{}`

```
> ;1
Error: unexpected ';' in ";"
```

R's grammar says why. `prog` reaches a `;` through one production only:

```
prog	:	END_OF_INPUT
	|	'\n'
	|	error
	|	expr_or_assign_or_help '\n'
	|	expr_or_assign_or_help ';'
	;
```

There's no bare-`;` alternative, and the two terminator productions are peers, so
whichever arrives first reduces the expression. Inside `{}` the picture differs,
because `exprlist` has a production for exactly this:

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
accepts every row of both columns. Aether reports ``Unexpected `;`.`` on the
top-level half and accepts the braced half, which is R's rule exactly.

The condition is one line to state: an `RStatement` with no expression is an
error at top level. That's precisely the set R rejects, so the rule needs no case
analysis beyond the scope it's in. No shape change: the `;` is still a token on a
statement of its own.

## `L` and `i` belong to the number

tree-sitter-r spells `integer` as `seq($._float_literal, "L")` rather than as one
regex, so the `L` is a token of its own. Two things follow, and aether disagrees
with both.

`extras` may fall between the two tokens, so `1 L` comes out as a single
`integer` spanning the space. Aether can't reproduce that even in principle,
because `R_INTEGER_LITERAL` is one token in air's kind list, and one token can't
span a gap.

And the `L` competes with `identifier` on length, so a name that merely starts
with a suffix wins outright: `1Li` is the float `1` followed by the name `Li`.

R does neither. `NumericValue` in `src/main/gram.y` stops the literal at the
digit when a space follows, and otherwise takes the suffix whatever comes next.
Aether follows R:

| source | tree-sitter-r | R and aether |
|---|---|---|
| `1 L` | one `integer` spanning the space | `1`, then the name `L` |
| `1Li` | `float 1`, `identifier Li` | `1L`, then the name `i` |
| `1iL` | `float 1`, `identifier iL` | `1i`, then the name `L` |
| `1LL` | `float 1`, `identifier LL` | `1L`, then the name `L` |
| `1L.` | `float 1`, `identifier L.` | `1L`, then the name `.` |
| `1e5iL` | `float 1e5`, `identifier iL` | `1e5i`, then the name `L` |

Every row but the first is a syntax error in R and in aether, arriving as two
expressions on one line. tree-sitter-r parses all of them cleanly, since its
`program` needs no separator. So nothing is lost by disagreeing: there's no clean
tree to compare against in the first place.

R's rule can't disagree with R about a program R accepts, because it never looks
past the suffix. The lookahead rule can only differ on programs R rejects, and
there it puts the error one token earlier than R does, for a byte of lookahead
nothing else in the lexer needed.

## An unfinished number keeps its token and gets a diagnostic

A `0x` commits to a hexadecimal and an `e` or `p` commits to an exponent. When
what comes next isn't there, tree-sitter-r un-matches the longer token and
re-splits the bytes into something shorter that happens to be valid: `0x` becomes
the number `0` and the name `x`. Where even that's unnecessary it just accepts:
its `_number_literal` regex ends `[eE][+-]?\d*`, so `1e` and `1e+` are whole
floats.

R rejects all of them, from the lexer, at the start of the literal:

```
> 1e
Error: unexpected input in "1e"
```

Aether follows R on the verdict and keeps the token. The literal spans everything
consumed and carries a diagnostic.

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

The token boundaries barely change (`1east` splits the same way either way), so
this buys almost nothing in tree shape and everything in the message. Without it,
`0x` isn't silently accepted either: it lexes as two expressions on one line and
reports "Expected a newline or a `;` between two expressions.", which is a
bewildering thing to read while half-typing `0xFF`. The lexer is the only place
that still knows a hexadecimal was started, so it's the only place that can say
so.

The token stays rather than becoming an `ERROR_TOKEN` because the text is a
well-formed literal that was cut short, so the parser can still use it. And the
line is drawn at what the grammar *requires*: a suffix is optional, so a literal
that ends without one is finished, not unterminated. `1L` and `1Li` carry no
diagnostic.

## A backslash inside `%...%` is ordinary text

tree-sitter-r's `special` is `/%[^%\\\n]*%/`, a character class that excludes
backslash, so `a %a\b% b` errors. Their own grammar comment concedes the regex is
approximate: "This could probably be fine tuned to disallow more things."

R's `SpecialValue` excludes only the newline, so `%a\b%` is an ordinary
`SPECIAL`, and aether takes the whole operator as one token.

Nothing is gained by rejecting a program R accepts, and a backslash carries no
meaning inside `%...%` for the lexer to act on. There are no escapes there, so
it's one more character in the operator's name. The newline is different, since R
stops at it too.

## `OperatorPrecedence` and `call_ext.rs` aren't here

Not a tree-shape deviation, just a note so nobody goes looking.

Both are formatter-only, so they aren't in `aether_r_syntax`.
`OperatorPrecedence` duplicated `binding_power.rs`'s `BindingPower` under
different names for the same token set, and the Pratt loop never called it.
`RCall::is_test_call()` is formatter policy: it recognizes `test_that()`-shaped
calls so their brace argument formats specially. It has no caller in the parser
or its tests.

Keeping formatter-only code here would mean maintaining it against a moving
target with no test coverage to catch drift. Reordering `BindingPower` would have
to be mirrored into `OperatorPrecedence` by hand, unverified, for a table nobody
touches until air's formatter port begins.

`argument_ext.rs` and `string_ext.rs` stay, since the parser or its tests reach
through them today.

Whoever ports air's formatter recreates both from air's original `air_r_syntax`
source. That's a straight vendor, not a rewrite, since aether's `RSyntaxKind` and
node shapes are unchanged from air's for every token and node either file
touches.

## tree-sitter-r vs. R

One place where tree-sitter-r already disagrees with R and aether follows
tree-sitter anyway.

### `%foo` at end of file

```r
x %in
```

R accepts it. `SpecialValue` in `src/main/gram.y` exits its loop on `R_EOF`
without ever seeing the closing `%` and still returns a `SPECIAL` token.
tree-sitter-r's regex requires the closing `%`, so it errors, and aether errors
too.

Aether does copy R for the neighbouring case, an unterminated `%...%` cut short
by a newline. R ungets the newline before reporting, so the error stays local and
the next line parses normally. Aether stops before the newline too, keeping the
partial `%foo` as a `SPECIAL` token with the diagnostic attached. That isn't a
deviation from tree-sitter, just a recovery choice tree-sitter has no equivalent
of.
