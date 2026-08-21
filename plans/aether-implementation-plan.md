# Aether: a hand-written Rust parser for R

**A staged implementation plan.**

This document is written for a Claude session picking this up cold. It assumes no
prior context beyond this file. Read it top to bottom once before writing code.

---

## 0. What we are building and why

`posit-dev/air` currently parses R with `tree-sitter-r`, then walks the tree-sitter
tree and replays it as Biome `Event`s into a `LosslessTreeSink`, producing a Rowan
green tree (`crates/air_r_parser/src/parse.rs`, ~1600 lines). This works, but:

- **No error resilience.** `parse_from_treesitter` bails the instant
  `root_node().has_error()` is true and returns a single `R_BOGUS_EXPRESSION`
  spanning the whole file. There is a `TODO` in that function saying exactly this:
  a resilient parser "would probably only be able to happen if we swap out tree
  sitter for a hand written recursive descent pratt parser using the Biome infra."
- **No real diagnostics.** One flat `ParseError` string for the entire file.
- **Semicolons are a lie.** They are mapped to `TriviaPieceKind::Whitespace` so the
  formatter silently drops them. There are `TODO(semicolon)` comments about this in
  four places in `parse.rs`.
- **Grammar quirks leak in.** `extract_operator` has an `optional()` RHS in
  tree-sitter-r purely to help completions, so `a$1` yields
  `right: missing (required)` with *no error at all*
  (see `tests/snapshots/undefined/extract_expression_error.R`).

**Aether** is a new standalone repository containing a hand-written recursive
descent + Pratt parser for R, built on the same Biome infrastructure air already
uses, producing the same Rowan tree. At the end of the project, air swaps
`air_r_parser` over to aether's implementation and drops tree-sitter entirely.

### The one-line summary of the architecture

Air's current pipeline is:

```
source ──tree-sitter──► TS tree ──RWalk──► Vec<Event> + Vec<Trivia> ──LosslessTreeSink──► Rowan
```

Aether's pipeline is:

```
source ──RLexer──► tokens ──RTokenSource──► RParser ──► Vec<Event> + Vec<Trivia> ──LosslessTreeSink──► Rowan
```

**The right half of that pipeline is identical and already works.** We are only
replacing the token production and event production. This is a good thing: it
means `Event`, `Trivia`, `RSyntaxFactory`, and `LosslessTreeSink` are all fixed
points we do not have to redesign, and the existing snapshot output format is
directly comparable.

---

## 1. Decisions already made

These were settled with the repo owner. Do not relitigate them; if one turns out
to be wrong, raise it explicitly rather than quietly deviating.

| # | Decision |
|---|---|
| 1 | New standalone repo, `aether`. Swapped into air at the very end. All tests live in aether. |
| 2 | Vendor `r.ungram` + the `xtask codegen` machinery into aether. Aether owns its syntax layer. |
| 3 | Depend on air's tree-sitter-r-backed parser as a differential oracle. A development aid that runs with the rest of the tests, and is deleted outright at swap-in. |
| 4 | Grammar deviations land **first**, so the parser targets the final tree shape from day one. |
| 5 | Full Biome-style error recovery from the start: bogus nodes, `ParseRecoveryTokenSet`, rich `ParseDiagnostic`s. |
| 6 | **Markdown snapshot harness for everything.** No Biome-style `tests/specs/**` + `.rast`. |
| 7 | Imported-but-unsupported tests stay inert via a per-test `[pending]` marker; an unexpected pass is a hard failure. |
| 8 | Lossless round-trip (`source == tree.to_string()`) asserted on every test. No fuzzing, no CRAN corpus run for now. |
| 9 | Feature order runs lexer-first, then one construct per phase, ending with recovery polish and integration. The phase table in §4 is authoritative; this row records only that the order is settled, not what it is. |
| 10 | Plan (and PRs) should be dense with real code, not prose sketches. |

---

## 2. Ground truth: what the current system actually produces

Read these files in `posit-dev/air` before starting. Paths are exact as of the
current `main`.

| Path | Why it matters |
|---|---|
| `xtask/codegen/r.ungram` | The grammar. ~330 lines. This is the spec for the tree shape. |
| `xtask/codegen/src/r_kinds_src.rs` | The `RSyntaxKind` source of truth: puncts, keywords, literals, tokens, nodes. |
| `xtask/codegen/src/*.rs` | The codegen: `generate_nodes.rs`, `generate_syntax_kinds.rs`, `generate_syntax_factory.rs`, `generate_node_factory.rs`, `generate_macros.rs`, `generate_nodes_mut.rs`, `ast.rs`, `kind_src.rs`, `language_kind.rs`. |
| `crates/air_r_syntax/` | Generated output: `src/generated/{kind,nodes,nodes_mut,macros}.rs`, plus hand-written `syntax_node.rs`, `call_ext.rs`, `argument_ext.rs`. |
| `crates/air_r_factory/` | Generated `RSyntaxFactory`. Consumed by `LosslessTreeSink`. |
| `crates/air_r_parser/src/parse.rs` | The tree-sitter walker. This is what we are replacing, and what the oracle depends on as its reference (§10.4). |
| `crates/air_r_parser/tests/spec_test.rs` | The current snapshot harness. Our markdown harness borrows its assertions. |
| `crates/biome_ungrammar/` | Vendored ungrammar parser, path dep. |
| `Cargo.toml` | Biome deps come from a **fork at a pinned rev**, not crates.io. |

### The Biome dependency pin — critical

Air's Biome deps come from a fork at a pinned rev, not crates.io, and aether's
`Cargo.toml` pins the same one. Aether adds `biome_console` and
`biome_diagnostics` from that rev, which air does not depend on — they are what
render the `diagnostics` cell in the markdown harness (§10.2). See §5 for the
consequences of the two ever disagreeing.

### Crate naming

Aether's crates are **`aether_r_syntax`, `aether_r_factory`, `aether_r_parser`**.
Aether owns them; air becomes a consumer.

This is cheaper to do than it looks. Air's codegen derives both the crate names
*and the output directories* from three functions in
`xtask/codegen/src/language_kind.rs`:

```rust
pub fn formatter_crate_name(&self) -> String { format!("air_{self}_formatter") }
pub fn syntax_crate_name(&self)    -> String { format!("air_{self}_syntax") }
pub fn factory_crate_name(&self)   -> String { format!("air_{self}_factory") }
```

and `ast.rs` builds its write paths from them:

```rust
let syntax_generated_path  = project_root().join("crates")
    .join(language_kind.syntax_crate_name()).join("src/generated");
let factory_generated_path = project_root().join("crates")
    .join(language_kind.factory_crate_name()).join("src/generated");
```

So changing `air_` to `aether_` in those three functions renames the crates,
redirects codegen output to `crates/aether_r_syntax/` and
`crates/aether_r_factory/`, and rewrites the `use aether_r_syntax::…` lines that
`aether_r_factory`'s generated code emits — all at once. That is the *entire*
rename, on the codegen side.

Drop `formatter_crate_name` and `xtask/codegen/src/formatter.rs` from aether's
copy; aether has no formatter crate and air keeps its own codegen for that.

**What this costs at swap-in: nothing, by decision.** Air currently has
~hundreds of `use air_r_syntax::…` lines across `air_r_formatter`, `workspace`,
`lsp`, and `comments`. **Settled with the owner:** none of them change. Air keeps
writing `air_r_syntax` everywhere and Cargo maps the name to aether's crate:

```toml
air_r_syntax = { package = "aether_r_syntax", git = "https://github.com/posit-dev/aether", rev = "..." }
air_r_factory = { package = "aether_r_factory", git = "...", rev = "..." }
air_r_parser = { package = "aether_r_parser", git = "...", rev = "..." }
```

The swap PR is then a handful of `Cargo.toml` lines plus the formatter changes
the semicolon deviation forces, and nothing else — which is the point: the
substantive diff stays reviewable instead of being buried under a thousand import
changes.

This also means **aether's crate names are not a decision air has to live with**.
If the owner later wants air's imports to say `aether_r_syntax` outright, that is
a mechanical find-and-replace across air, landed separately once the swap is in
(§12.3). Deferred, not rejected.

---

## 3. Repository layout

```
aether/
├── Cargo.toml                     # workspace, pinned biome rev
├── Cargo.lock                     # committed
├── rust-toolchain.toml            # copy from air
├── clippy.toml                    # copy from air
├── justfile                       # `just codegen`, `just test`, `just accept`
├── README.md
├── CLAUDE.md                      # agent guidance; see §12
├── .gitattributes                 # *.snap.md linguist-generated=true
├── .config/
│   └── nextest.toml               # nextest profiles; see §5.1
├── xtask/
│   ├── Cargo.toml
│   ├── src/
│   │   ├── main.rs                # `xtask accept`, `xtask import-corpus`
│   │   └── accept.rs              # single-process snapshot regeneration
│   └── codegen/
│       ├── Cargo.toml
│       ├── r.ungram               # vendored from air/xtask/codegen/r.ungram
│       └── src/                   # vendored codegen, unchanged
└── crates/
    ├── biome_ungrammar/           # vendored from air, path dep
    ├── aether_r_syntax/           # generated + hand-written ext traits
    ├── aether_r_factory/          # generated
    ├── aether_r_parser/           # THE NEW PARSER
    │   └── src/
    │       ├── lib.rs
    │       ├── diagnostics.rs     # every diagnostic constructor, in one file
    │       ├── lexer.rs           # RLexer; ends with `#[cfg(test)] mod tests;`
    │       ├── lexer/
    │       │   ├── tests.rs       # the assert_lex! family + `mod` declarations
    │       │   └── tests/         # one file per dispatch arm — see §8.6
    │       ├── parse.rs           # public `parse()` entry point, `RParse`
    │       ├── options.rs
    │       ├── token_source.rs    # RTokenSource; `#[cfg(test)] mod tests;`
    │       ├── parser.rs          # RParser + the two context enums
    │       ├── syntax.rs          # `parse_root`
    │       └── syntax/
    │           ├── expr.rs        # Pratt loop, primary exprs, literals
    │           ├── call.rs        # call / subset / subset2 / arg lists
    │           ├── parenthesized_expression.rs
    │           ├── function.rs    # ← Phase 11. defs, lambdas, parameters
    │           ├── control.rs     # ← Phase 12. if/else, for, while, repeat
    │           └── statement_list.rs # statement lists; braces and `;` in 13
    ├── aether_r_test_macros/      # proc-macro: `gen_corpus_tests!`
    │   └── src/lib.rs             #   modelled on air's `tests_macros`
    ├── aether_r_parser_tests/     # Every corpus test, plus the harness that
    │   ├── src/                   #   runs them. Depends on the parser;
    │   │   ├── document.rs        #   nothing depends on it but `xtask`.
    │   │   ├── check.rs           # run one block, produce its cells (§10.2)
    │   │   ├── corpus.rs          # locating input and snapshot files
    │   │   ├── manifest.rs        # `MANIFEST.toml` (§10.3.3)
    │   │   └── runner.rs          # `run_file`: read-only compare; panics on
    │   │                          #   mismatch, for use from a `#[test]`
    │   └── tests/
    │       ├── main.rs            # globs corpus/**/*.md → crate::run_file
    │       ├── manifest.rs        # the two anti-drop guards
    │       ├── quick_test.rs      # scratchpad, #[ignore]d
    │       └── corpus/            # THE TESTS — MANIFEST.toml maps every one
    │           ├── MANIFEST.toml  # every imported test name → its home file
    │           ├── literals/      # each X.md has a sibling X.snap.md
    │           ├── expressions/
    │           ├── functions/
    │           ├── control/
    │           ├── statements/
    │           └── recovery/
    └── aether_r_oracle/           # Dev-only differential oracle (§10.4)
        ├── Cargo.toml             # depends on air_r_parser at a pinned rev
        └── src/
            ├── lib.rs             # `compare()` and `OracleResult`
            └── shape.rs           # normalizes either tree for comparison
```

Everything without a `← Phase N` marker exists. `syntax.rs` holds only
`parse_root` and an empty statement list, and the modules under it arrive with
the features they parse.

---

## 4. Phase plan

Fourteen phases. Each is one reviewable PR. Phases 1–6 are infrastructure and
should land quickly; the owner wants to review the *parser* in small pieces, not
the scaffolding.

**Every phase from 7 onward ends with the same two steps:**
1. Flip the `[pending]` markers off the corpus tests the feature enables.
2. Regenerate snapshots, eyeball every changed cell.

The oracle needs no step of its own: it runs with the rest of the tests (§10.4),
so a newly enabled test that disagrees with tree-sitter-r fails immediately. What
the phase owes it is a judgement — either the disagreement is a bug, or it is
intentional and earns a `[deviation]` marker and a line in `docs/deviations.md`.

**The oracle landed as its own PR on top of Phase 6, and is not a numbered
phase.** Nothing else in the project depends on it, so it shipped separately
rather than doubling the size of the phase everything else was waiting on.

Phases 1–10 have landed. Their sections below are compressed to the decisions
later phases depend on; the repo, `docs/deviations.md`, and the code's own
comments are the source of truth for everything else.

| Phase | Title | Deliverable | |
|---|---|---|---|
| 1 | Repo bootstrap | Workspace builds, CI green, nothing else | ✅ |
| 2 | Vendor + rename syntax layer | `just codegen` reproduces `aether_r_syntax` + `aether_r_factory`, identical to air's modulo the rename | ✅ |
| 3 | Grammar deviations | `.ungram` + kinds updated, regenerated, documented | ✅ |
| 4 | Lexer | Standalone `RLexer`, plus seventeen `#[cfg(test)]` token-stream test files under `src/lexer/tests/` (§8.6) | ✅ |
| 5 | Parser scaffolding | `RParser`, `RTokenSource`, diagnostics, `parse()` returns an empty `R_ROOT` | ✅ |
| 6 | Test harness + corpus import | The markdown harness, `xtask accept`, and all 89 tree-sitter tests plus air's snapshots imported as `[pending]` | ✅ |
| 7 | Literals, identifiers, program | `1`, `"a"`, `foo`, `TRUE`, `NA_integer_`, `..1`, `...` |✅ |
| 8 | Binary & unary operators | The Pratt core, precedence table, newline sensitivity |✅ |
| 9 | Calls, subset, subset2 | `f(a, b=1)`, `x[i]`, `x[[i]]`, hole arguments |✅ |
| 10 | Extract & namespace | `$`, `@`, `::`, `:::` and the deviation from tree-sitter |✅ |
| 11 | Functions | `function(x) y`, `\(x) y`, parameters, defaults | |
| 12 | Control flow | `if`/`else` (incl. the newline rule), `for`, `while`, `repeat` | |
| 13 | Blocks, statements, semicolons | `{}`, `R_STATEMENT_LIST`, real semicolon tokens | |
| 14 | Recovery polish + air integration | Errors corpus fully green; swap into air | |

---

## 5. Phase 1 — Repo bootstrap — LANDED

The workspace, CI, and the tooling every later phase runs. The repo is the source
of truth for all of it; what follows is only what a later phase needs to know.

**The Biome pin.** `Cargo.toml` pins the same fork and rev as air
(`lionel-/biome` @ `a1296ea6`), plus `biome_console` and `biome_diagnostics`,
which air does not depend on and the markdown harness needs for diagnostic
rendering. Aether and air must not disagree on this rev: `RSyntaxNode` types are
nominally different across revs, and the swap-in (§12.3) would become an ordeal.
Treat a rev bump as its own PR with a full snapshot regeneration.

`[workspace.dependencies]` already names crates that do not exist yet
(`aether_r_oracle`, `aether_r_parser_tests`, `aether_r_test_macros`). Cargo does
not resolve a workspace dependency until something depends on it, so the entries
are inert until the phase that creates the crate.

**The commands** are in `justfile` and documented in `CLAUDE.md`. Two omissions
are deliberate:

- `just test` does not run `cargo test --doc`. Nextest does not run doctests, and
  aether has none worth running; add the line back if that changes rather than
  letting it silently do nothing.

- There is no `just oracle` recipe. The oracle is not feature-gated, so it runs
  with everything else and a dedicated recipe would only duplicate `just test`
  (§10.4.1).

### 5.1 Why nextest

Three of its properties matter directly here, and they are the reason `CLAUDE.md`
says tests run under nextest rather than `cargo test`.

**Process-per-test isolation.** A panic in one corpus file cannot take down the
others, and a stack overflow from runaway recursion — very possible while
developing a recursive descent parser — is reported as a clean failure rather
than killing the run.

**Timeouts.** `cargo test` hangs forever on an infinite loop; nextest terminates
and reports it. `.config/nextest.toml` sets
`slow-timeout = { period = "10s", terminate-after = 3 }`, which turns "the parser
never loops forever" (invariant 3) from a six-hour CI hang into a normal red
test.

**Filter expressions.** `-E 'test(/Dollar/)'` beats substring matching once there
are ~90 named corpus tests.

`retries = 0` in the CI profile is deliberate: parser tests are deterministic,
and a retry that turns a real failure green is strictly harmful. `#[ignore]`d
tests need `--run-ignored all`, which is what the `quick` recipe passes.

### 5.2 CI

`.github/workflows/ci.yml` runs fmt, clippy with `-D warnings`, the nextest
suite under the `ci` profile, and a codegen freshness gate
(`just codegen && git diff --exit-code`).

One more freshness gate joins it in Phase 6, and the workflow carries a `TODO!`
comment marking the spot: `cargo run -p xtask -- accept && git diff --exit-code`.
That gate is what makes it impossible to land a PR with stale snapshot cells,
because regenerating them on CI must produce no diff.

---

## 6. Phase 2 — Vendor the syntax layer — LANDED

`xtask/codegen/`, `crates/biome_ungrammar/`, `crates/aether_r_syntax/`, and
`crates/aether_r_factory/` are vendored from air. `just codegen` regenerates the
last two from `xtask/codegen/r.ungram`.

**The rename was three functions** in `xtask/codegen/src/language_kind.rs`:

```rust
pub fn syntax_crate_name(&self)  -> String { format!("aether_{self}_syntax") }
pub fn factory_crate_name(&self) -> String { format!("aether_{self}_factory") }
```

`ast.rs` derives its output paths from them, so renaming them also redirected
codegen output and rewrote the `use aether_r_syntax::…` lines the generated
factory emits. `formatter_crate_name()` and `xtask/codegen/src/formatter.rs` were
dropped — aether has no formatter crate, and air keeps its own codegen for that.

The hand-written files in the syntax crate (`lib.rs`, `syntax_node.rs`,
`call_ext.rs`, `argument_ext.rs`) came along too. They are not generated, and
air's formatter uses them, so they must be here for the swap-in to work.

Phase 2's acceptance check — that aether's generated output is byte-identical to
air's after `s/air_r_/aether_r_/g` — passed at the time and is not re-run. What
guards it going forward is the codegen freshness gate in CI, plus the fact that
Phase 3's deviations are now the only intended delta.

---

## 7. Phase 3 — Grammar deviations — LANDED

Aether's tree diverges from air's here, deliberately and before any parser code,
so every later phase targets the final shape.

**`docs/deviations.md` is the source of truth.** It carries the full argument for
each deviation, with examples and both trees, and it has grown past what this
plan seeded — deviations 6 through 9 landed with the lexer in Phase 4. Do not
restate it here; add to it.

What later phases code against:

**`RStatementList` of `RStatement`** replaces `RExpressionList` outright. Both of
`RStatement`'s slots are optional, the same trick `RArgument` already uses:

```ungram
RStatementList = RStatement*

RStatement =
	expression: AnyRExpression?
	semicolon: ';'?
```

`1` fills the first slot, `1;` fills both, and a bare `;` fills only the second.
A statement with neither slot filled is never produced by the parser.

The generated accessors, which §11.7 and §12.3 both write against:

```rust
impl RRoot {
    pub fn bom_token(&self) -> Option<SyntaxToken> { ... }
    pub fn statements(&self) -> RStatementList { ... }   // list slots are never Option
    pub fn eof_token(&self) -> SyntaxResult<SyntaxToken> { ... }
}

impl RStatement {
    pub fn expression(&self) -> Option<AnyRExpression> { ... }
    pub fn semicolon_token(&self) -> Option<SyntaxToken> { ... }
}
```

Three codegen rules produce that shape, and they are worth internalising because
they determine what air's formatter code will look like: an optional node slot
becomes `Option<T>`, an optional token slot becomes `Option<SyntaxToken>` and
gains a `_token` suffix, and a list slot is returned bare because it is always
present even when empty. So `root.statements()` needs no unwrapping.

**`RBracedExpressions` is now `RBracedStatements`**, since it holds statements.

**`R_BOGUS_SELECTOR` is a fifth `AnyRSelector` variant.** Without it the
generated factory does not panic on a failed cast of the `right` slot — it maps
`R_EXTRACT_EXPRESSION` through `to_bogus()` and collapses the whole expression
into one bogus node. Round-trip still passes, so nothing in the harness would
catch it.

**One path constructs one, and it is not the `right` slot.** Phase 10 was to be
its first user by consuming a non-selector RHS into a bogus node; §11.4 settles
that it diagnoses and consumes nothing instead. What builds one is the `left` slot
of `::` and `:::`, which is an `AnyRSelector` too — there the operand is already
parsed, so wrapping it costs no tokens and keeps the namespace node whole.

`R_STATEMENT_LIST` is recognised as a list kind, so `SyntaxKind::is_list()`
returns true for it. This was decided so that air's `has_bogus_nodes_or_empty_slots`
would cover it; the harness as landed does not use that helper, and §10.2 records
the gap.

---

## 8. Phase 4 — The lexer — LANDED

`crates/aether_r_parser/src/lexer.rs`, one module, with its tests underneath it
in `src/lexer/tests/`. The tests are descendants of the module they test, which
is why they sit there rather than in a test crate: they read private fields
(`after_line_break`, `diagnostics`) and call private helpers directly, and
`#[cfg(test)]` keeps every byte of them out of the parser air links against.

The code is the source of truth for how it lexes. What follows is the contract
the parser phases are written against, plus the landmines.

### 8.1 The traits we implement

`RLexer` implements `biome_parser::lexer::Lexer`, `LexerWithCheckpoint`, and
`ReLexer`. `LexerWithCheckpoint` is what gives `NthToken` lookahead for free via
`BufferedLexer`.

**`RLexContext` has one variant, `Regular`**, so `is_regular()` is always true,
`BufferedLexer` never resets its buffered lookahead, and `p.nth(n)` means the
same thing everywhere in the file. Every R token is context-free to lex, which
was only true once strings became one token each (deviation 12). The enum stays
rather than collapsing to `()` so the contract is named where a reader of the
lexer will look for it.

### 8.2 Lexer state and the sticky line-break flag

Every field of `RLexer` is one `LexerCheckpoint` restores, and `checkpoint()` /
`rewind()` carry all of them. Two are worth knowing about:

**`after_line_break`** is sticky across `next_token` calls. It is set by the
overridden `consume_newline_or_whitespaces()` and cleared only once a non-trivia
token has been stamped with `PRECEDING_LINE_BREAK`. That is what lets the lexer
report a preceding line break even though `NEWLINE` is a token in its own right,
and it is exactly `biome_js_parser`'s mechanism for ASI. It survives `WHITESPACE`
and `COMMENT`, so `1\n  # c\n  + 2` reports the break on `+` and not on `2`.

`LexerCheckpoint` carries `after_line_break` alongside `current_flags`, and the
two are not redundant: `current_flags` records what was stamped on the
checkpointed token, while `after_line_break` is the sticky state lexing resumes
from. A rewind that restored only one would silently lose the break — invisible
until Phase 12's `else` handling, and silent even then. `checkpoint.rs` has a
test for exactly this.

**`unicode_bom_length`** is set from `consume_potential_bom()` even though
nothing in R reads it. `LexerCheckpoint` has the field, so `checkpoint()` must
construct it and `rewind()` must destructure it; a hardcoded `0` there would be a
lie the next reader has to re-derive.

### 8.3 Dispatch

`lex_token()` is one `match` on the leading byte. Read it in the code; the
ordering decisions carry comments explaining themselves, several of them lifted
from tree-sitter-r's `grammar.js` where they record filed bugs.

The one structural note worth repeating, because it is invisible from the code
and easy to undo: **`r`/`R` deliberately does not get an arm of its own.** Raw
strings share the identifier arm and fall through when the prefix does not match.
A `b'r' | b'R'` arm ahead of `b'a'..=b'z'` splits the letter range, the jump table
shrinks to `\t..=r`, and every byte from `s` up falls out to a comparison chain
— measured at about 14% on 2.3 MB of R source.

### 8.4 The token inventory

The facts the parser phases need, and the ones a later grammar edit can break.

**`[` and `[[` are lexed greedily; `]` and `]]` are not.** `[[` is always a single
token. `]]` is always two `]`, and the parser joins them with
`p.re_lex(RReLexContext::JoinBrackets)` from `RParser::do_bump()`,
using the scope it already tracks. There is no scope stack in the lexer. This is
the single most important design call in the lexer and the one most likely to be
undone by accident.

The asymmetry is the point, and every reference implementation shares it. Openers
stack left to right, so two adjacent ones never straddle a boundary and greedy is
unambiguous; R's `gram.y` lexes `[[` as one `LBB` token. But closers stack right
to left, so a run of them belongs to several constructs at once and only the
parser knows where the boundaries fall — `gram.y`'s rule is literally
`expr LBB sublist ']' ']'`, two tokens, never joined.

`biome_js_parser` draws the line in the same place, and the halves are easy to
swap: `<` is lexed greedily into `<<` and `JsReLexContext::TypeArgumentLessThan`
splits it back, while `>` is never lexed greedily at all and
`JsReLexContext::BinaryOperator` joins `> >` at the one site that wants a shift.
Nested generics need no re-lex whatsoever, which is exactly the property we want
for nested subsets.

**A stack in the lexer is not on the table, whatever its merits.** Three
mechanisms rule it out: `BufferedLexer` rewinds the inner lexer on its own
initiative, `LookaheadIterator` runs it ahead speculatively, and
`LexerCheckpoint` has no slot to restore a stack from. A speculative lookahead
would advance the stack and nothing would roll it back, which yields a wrong tree
that still round-trips. `parse_argument` calls `nth()` inside `[[` (§11.3), so
that is no longer a hazard waiting on a future call site — a lexer stack would be
wrong the day it was written. tree-sitter-r pays these
costs in the open: its `Scopes` stack has to fit in
`TREE_SITTER_SERIALIZATION_BUFFER_SIZE`, so `MAX_SCOPES_COUNT` caps nesting and a
failed `scopes_push()` makes the scanner decline the token outright. And it still
consults `valid_symbols[CLOSE_BRACKET2]` — parser state — to break the tie at `]`.
Even there, the lexer is not deciding alone.

It does not need to. The decision turns on one fact — whether the innermost open
construct is a `[[` — and the parser already holds it as `Scope` (§9.2), so the
join reads it from there on every bump, at one call site.

The cost is `]]` in the recovery sets that run inside a `[[`, and that is not the
hazard a greedy lexer creates. A greedy `]]` can straddle two constructs, so
swallowing one costs two closers and strips an *enclosing* construct of its
terminator. A scope-driven `]]` only ever exists where the innermost open
construct owns both brackets, so it is an ordinary closer, named alongside `)`,
`]`, and `}` and no more dangerous than any of them. §11.3 has the join site and
the `x[[a[b]]]` worked example.

**Keywords are lexed as `IDENT` first, then mapped by text.** That is what gets
tree-sitter-r issue #200 right for free (`elseμ <- 1` must not see an `else`
keyword). Two spellings do not line up one-for-one, and `NA_real_` is the one an
implementer will get wrong by pattern-matching on the name:

| R source | `RSyntaxKind` |
|---|---|
| `TRUE` / `FALSE` / `NULL` | `TRUE_KW` / `FALSE_KW` / `NULL_KW` |
| `Inf` / `NaN` | `INF_KW` / `NAN_KW` |
| `NA` | `NA_LOGICAL_KW` |
| `NA_integer_` | `NA_INTEGER_KW` |
| `NA_real_` | **`NA_DOUBLE_KW`** — the one name that does not follow the text |
| `NA_complex_` | `NA_COMPLEX_KW` |
| `NA_character_` | `NA_CHARACTER_KW` |
| `function` … `break` | `FUNCTION_KW` … `BREAK_KW`, lowercase and identical |

**Two generated-code landmines.** `RSyntaxKind::from_keyword()` is useless here —
codegen derives it from the lowercase placeholder spellings in `r_kinds_src.rs`,
so `from_keyword("TRUE")` is `None`. And the `T!` macro has a `[#] => HASH` arm
while `RSyntaxKind` has no `HASH` variant, so `T![#]` fails to compile. Comments
are `COMMENT`; there is no `#` token.

**Backticks produce an identifier, not a string.** `` `my var` `` is one `IDENT`
covering the delimiters, because tree-sitter-r puts the quoted form inside its
`identifier` rule and air's walker maps it straight to `R_IDENTIFIER`. So there
is no `Backtick` variant in `RStringDelimiter`.

**A string is one token too**, delimiters included, with a kind per quote style:
`R_SINGLE_QUOTED_STRING_LITERAL`, `R_DOUBLE_QUOTED_STRING_LITERAL`, or
`R_RAW_STRING_LITERAL`. Escapes are not modelled separately — neither air's
walker nor `r.ungram` has an `escape_sequence` — so the body is text and nothing
else, reached through `RStringValue::inner_string_text()` in `aether_r_syntax`.
Deviation 12 in `docs/deviations.md` carries the argument. So backticked names
and strings work alike, which is the point.

Both may span newlines: R's `token()` dispatches them to the same
`StringValue()` function, which turns a newline into an escaped newline rather
than an error. An unterminated one genuinely runs to EOF, which is what
tree-sitter does too. The way to keep the error useful is where the diagnostic
points, not where the scan stops: at the opening delimiter, with EOF as a
secondary label. `enable_recovery_on_line_break()` will not help the parser
resynchronise after one, and it should not be expected to.

**`ERROR_TOKEN`** is text no production can take. Three producers, all in the
lexer: a character that starts no token (`unexpected_character`, one character
consumed), an unterminated string (`unterminated_string`), and an unterminated
backtick-quoted name (`unterminated_backtick_identifier`). Deviation 6 in
`docs/deviations.md` carries the argument for why none of them keeps a
well-formed kind, and where the line falls against deviation 8's bounded
literals. The stray-character case is narrow — every printable ASCII character
starts some R token, so it is only control characters and non-ASCII characters
that are neither `XID_Start` nor `XID_Continue`. §11.1 is the parser's side.

**Four number and operator behaviours are deviations**, all landed and all
documented in `docs/deviations.md` rather than here: the greedy `L`/`i` suffix
(7), unterminated hexadecimals and exponents carrying a diagnostic instead of
being re-split (8), and a backslash inside `%…%` being ordinary text (9). Note
deviation 9 followed R rather than tree-sitter, so `%a\b%` is one `SPECIAL` and
the implemented pattern is `/%[^%\n]*%/` — the `Specials` corpus test is
`[deviation]` because of it.

A `%…%` may not span a newline. R ungets the newline before reporting so the
error stays local, and aether does the same: stop before the newline, keep the
partial `%foo` as a `SPECIAL`, attach `unterminated_special`. `%foo` at EOF
errors, following tree-sitter rather than R — logged under "tree-sitter-r vs. R"
in the ledger.

### 8.5 Newlines: the crucial decision

**Newlines are lexed as `NEWLINE` tokens, then converted to trivia by the token
source — but the parser can still ask whether one occurred.** This is the
contract §9 and §11 are written against, and the division of labour is JS's: the
lexer answers "was there a line break here", which is a lexical fact, and the
parser decides whether that fact means anything, which in R is statement
termination and `else` attachment.

- `RLexer` emits `NEWLINE` as its own kind and owns the preceding-line-break flag
  (§8.2).

- `RTokenSource` reports `NEWLINE`, `WHITESPACE`, and `COMMENT` as trivia. It
  keeps no line-break state of its own and delegates the question to the lexer,
  exactly as `JsTokenSource` does (§9.1).

- `RParser` reads the flag only in conjunction with the current `Scope` (§9.2).
  Three sites do so: the Pratt loop's statement break (§11.2), the `::` arm it
  gained in Phase 10 (§11.4), and `parse_else_clause()` (§11.6). Nothing else
  reads the raw flag.

Do not replicate tree-sitter's scope-stack approach of conditionally swallowing
newlines in the lexer. In a recursive-descent parser the equivalent information
is structural — the parser knows whether it is inside `(`/`[`/`[[` — so a lexer
scope stack would duplicate that knowledge and is the classic source of desync
bugs.

`Trivia` kinds map as follows, matching air's current `parse.rs`. The *kinds*
match air; which token each piece attaches to does not, and deviation 10 in
`docs/deviations.md` is why (§9.1):

| Lexer kind | `TriviaPieceKind` |
|---|---|
| `WHITESPACE` | `Whitespace` |
| `NEWLINE` | `Newline` |
| `COMMENT` | `SingleLineComment` |

**A `\r\n` is one `Newline` trivia piece, not two.** Air has CRLF tests
(`tests/snapshots/ok/crlf/*`); port them in Phase 6.

### 8.6 Testing

Seventeen `#[cfg(test)]` files under `src/lexer/tests/`, one per dispatch arm of
`lex_token()` plus one per piece of cross-cutting machinery. `tests.rs` holds the
driver and three macros: `assert_lex!` (exact token stream, and no diagnostics),
`assert_lex_err!` (the same plus expected diagnostic substrings), and
`assert_line_breaks!` (the sticky flag, on non-trivia tokens only).

The markdown corpus of §10.2 is the parser's harness and the lexer does not use
it. A token stream is small enough to assert exactly; snapshots earn their keep
when the expected value is too large to write by hand.

Two assertions in the shared `check_lex()` do more work than they look like, and
both are free on every test because they live there rather than in each test:

- **The ranges must tile the source** — start at 0, each adjacent to the last, the
  final one ending at `source.len()`. Strictly stronger than summing lengths: it
  also rejects a lexer that overlaps or rewinds, and it makes reconstructing the
  input to compare it redundant.

- **Silence is half the assertion.** `assert_lex!` requires zero diagnostics. A
  lexer that produces the right tokens and also complains about them is broken in
  a way a kinds-and-lengths table alone will not catch, and `[error]` corpus tests
  will not catch it either, because they only require some diagnostic.

**`coverage.rs` is the anti-gap guard, and it keeps running.** "Every token kind
has lexer coverage" is unfalsifiable as prose; this enumerates `RSyntaxKind` via
`from_raw` up to `__LAST` and asserts that every lexable kind is produced by some
source. `MANIFEST.toml` does the same job for the corpus (§10.3.3).

The failure mode it is built for is not "someone forgot a test today" — it is a
Phase 3-style grammar edit landing a new token kind in `r_kinds_src.rs` months
from now with nothing lexing it. Adding a *node* kind does not fail it; adding a
*token* kind does, until something lexes it. That failure is otherwise discovered
by the parser, as a confusing missing-slot bug several phases downstream.

`R_BRACK2` is reachable only through a re-lex, and `coverage.rs` drives the join
rather than excusing the kind, which is what keeps the guard total.

---

## 9. Phase 5 — Parser scaffolding — LANDED

`RTokenSource`, `RParser`, the public `RParse` / `parse()`, and a `parse_root`
that eats an optional `UNICODE_BOM`, opens and closes an empty
`R_STATEMENT_LIST`, and completes `R_ROOT`. No R grammar yet — Phase 7 fills the
list. The code is the source of truth; what follows is the contract the parser
phases are written against.

### 9.1 `RTokenSource`

**The trivia rule, in one sentence:** trivia trails the token before it until a
newline is seen, and leads the token after it from the newline onward; before the
first token of the file, everything leads. `\r\n` is one piece. That is
`JsTokenSource`'s rule — one latching bool, no lookahead, nothing buffered — and
it is where aether's tree diverges from air's, which attaches whitespace leading.
**Deviation 10 carries the whole argument**: the exact scope (whitespace only,
never comments or newlines), the audit of air's consumers, and the oracle
normalization that keeps coverage despite it. Do not restate it here.

Air's `test_parse_trivia_*` tests are ported into `token_source/tests.rs`, with
the three expectations the deviation moves flipped (`1 + \n1`, `1  \n `, and the
CRLF twin). Note what the flip costs: they no longer assert agreement with an
independent implementation, only that aether does not change its mind. The
external check is the oracle (§10.4).

**`has_preceding_line_break()` delegates to the lexer and keeps no copy.**
`BufferedLexer` stores a `LexerCheckpoint` per cached lookahead token and reads
the flag off *that* one, so delegation stays correct under `nth()`. A field
maintained here in `next_non_trivia_token()` would not — it would describe
whichever token was last pulled through that function rather than the current one.
A lookahead-then-bump test pins it.

`skip_as_trivia()` resumes as if at the start of the file, so trivia after a
skipped token leads the next token instead of trailing what was skipped. Phase
14's recovery is what produces those pieces.

`TokenSourceWithBufferedLexer` supplies `NthToken` lookahead for free. There is
no `BumpWithContext` impl: nothing in R needs a lex context (§8.1), so `bump()`
and `skip_as_trivia()` are the whole surface.

### 9.2 `RParser`

One `Copy` context field — `scope: Scope` — plus the helper that scopes it.

```rust
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub(crate) enum Scope {
    #[default]
    TopLevel,
    Brace,
    Parenthesis,
    Bracket,
    Bracket2,
}
```

**One field, five variants, and both of R's newline rules read off it.** Line
breaks are significant at top level and inside `{`, and meaningless inside `(`,
`[`, `[[`; an `else` may follow a line break everywhere except top level. Those
are not two independent axes — the second is a refinement of the first, since a
line break only reaches `else` where line breaks mean anything at all. tree-sitter-r
models this the same way, as one scope stack (`SCOPE_TOP_LEVEL`, `SCOPE_BRACE`,
`SCOPE_PAREN`, `SCOPE_BRACKET`, `SCOPE_BRACKET2` in `scanner.c`), and R's `gram.y`
as one `contextp`.

The three delimiter variants behave identically and could be one. They are kept
apart so a `with_scope` call names the token it just bumped rather than a category
the reader has to expand, and so the set lines up with the scanner's for anyone
cross-reading it.

**The polarity of the `#[default]` is deliberate, and the code does not say why.**
A forgotten `with_scope` leaves `TopLevel`, which produces a spurious statement
break: a loud parse error at the first multi-line call in the corpus. A default
that ignored line breaks fails silently instead — `1\n+2` would quietly become one
expression rather than two statements, a wrong tree with no diagnostic, which the
round-trip assertion does not catch either. `Scope::newlines_are_significant()`
answers exhaustively for the same reason: a scope added later has to say which
side it is on rather than defaulting onto the silent one.

- **`with_scope(scope, f)`** saves and restores the field. Nine callers, one per
  construct that opens a scope: `f(…)`, `x[…]`, `x[[…]]`, `(…)`, the `if` and
  `while` conditions, the `for` header, a parameter list, and `{…}`. `parse_root`
  is not among them — the default *is* top-level behaviour.

- **The opener is consumed inside the scope and the closer outside it.** A bump
  lexes the token *after* the one it consumes, so the scope in effect at a bump
  governs the token that lands. The opener's bump produces the first token of the
  body, so it belongs inside; the closer's bump produces the first token after the
  construct, so it belongs outside. Only `x[[…]]` can tell the difference, because
  only it re-lexes on the strength of the scope (§8.4) — `x[[]]` needs the join on
  the bump of the `[[` itself, and `x[y[z[[a]]]]` breaks if a `[[` scope is still
  in effect when the two `]` that follow its `]]` arrive. Every construct follows
  the rule anyway, since a reader will copy whichever one they are looking at.

- **`scope()`** is a plain read. Newline significance goes through
  `Scope::newlines_are_significant()` and nothing tests the delimiter variants
  individually; `parse_else_clause` (§11.6) is the one site that compares against
  a variant, and it wants `TopLevel` by name.

#### 9.2.1 Why an enum and not a bitflags set

Checked against every Biome parser at the pinned rev, because the obvious reach
here is `bitflags` and it is the wrong size of tool. `biome_js_parser` is the only
parser that uses flags for context, and the reasons it documents on
`ParsingContextFlags` are both about scale: masks that name several flags at once
(`FUNCTION_RESET_MASK`), and a whole context snapshotting into one word.
`biome_css_parser` is the parser at *aether's* scale, with two context fields, and
it uses plain fields with `std::mem::replace`. Nothing exists in between — graphql,
html, yaml, json, and grit carry no parse context at all. At one field neither of
JS's reasons survives, and flags could not express this context anyway: the five
scopes are mutually exclusive, which an enum states and a flag set only promises.

**A closure and not an RAII guard.** JS has both and uses the closure seventeen
times against the guard's one; its single guard site is `parse_class`, where the
state applies to the remaining ninety lines of the function. Aether has no site of
that shape — all nine are one call bracketed by a `bump` and an `expect`.

**Stored state and not a threaded parameter.** JS splits deliberately between
`ExpressionContext`, passed down, and `ParsingContextFlags`, stored, and the split
is the lesson: fine-grained context that changes within one expression is a
parameter, while coarse context that changes at construct boundaries and is read
at arbitrary depth lives on the state. `Scope` is unambiguously the second kind,
so threading it would touch every parse function in the crate.

That the helper earns its place at all, where CSS ships without one, comes down to
the call-site count and a failure mode: the scope is consulted several frames below
whatever set it, so a leaked value does not crash — it silently changes how
newlines are read for the rest of the parse and yields a wrong tree that still
round-trips. That is the one class of bug neither invariant 1 nor the harness
catches.

### 9.3 `RParse` — Biome's shape, not air's

The name matches `CssParse`, `JsonParse`, and the rest of the family;
`biome_js_parser`'s bare `Parse<T>` is Biome's only exception, and only because it
is generic over its root node, which `RRoot` makes moot. The method set —
`syntax`, `tree`, `diagnostics`, `into_diagnostics`, `has_errors` — is `CssParse`
and `AnyParse` verbatim, which is also the argument for `has_errors` over air's
singular `has_error`.

**Air's error API is gone entirely.** `ParseError` collapsed a whole file to one
formatted string because a tree-sitter parse either succeeded or it failed and
there was nothing finer to report. Aether has real `ParseDiagnostic`s, and
`process()` now genuinely carries them through the tree sink, where air's code
discards them with a comment saying they "go into the tree-sink but come right
back out". **The cost is six mechanical call sites in air and one real one**,
itemised in §12.3 step 3. This is the only deliberate API break in the swap, and
the real site, `workspace/src/format.rs`, is where per-diagnostic reporting starts
paying — so the break is the point rather than a side effect.

Two members of the family are deliberately absent until something wants them:
`ok(self) -> Result<RRoot, Vec<ParseDiagnostic>>`, which `biome_js_parser` has and
no other Biome parser does and air has no caller for; and anything that collapses
to a single error, since a consumer that genuinely needs one string can render a
`ParseDiagnostic` through `biome_diagnostics::PrintDescription` at its own call
site.

`parse_with_cache` stays private. Air has no `NodeCache` caller of its own and its
LSP re-parses whole documents, and the cache buys less than the name suggests in
any case: `TreeBuilder` owns one whether or not one is passed in, so interning
within a single parse already happens and an external cache adds only sharing
*between* parses. Make it `pub` when something reuses one across files — a
one-word change, and a swap-in concern.

`RParse` stores a `SendNode` rather than an `RSyntaxNode`, which is the one place
it departs from every other Biome parser. `parse.rs`'s doc-comment carries the
reason and the `parse_is_send` test pins it.

### 9.4 Diagnostics

`diagnostics.rs` holds every diagnostic constructor in the crate, and still holds
only the six the lexer raises — Phase 5 parses nothing, so it added none of its
own. What it did drop is the blanket `#[allow(dead_code)]` that `lib.rs` carried
while the lexer had no consumer; `parser.rs` carries a scoped one with a reason
until the syntax modules drive the context helpers in Phase 7.

**Each `expected_*` constructor lands in the phase that first calls it**, and is
specified in that phase's section rather than here:

| Constructor | Lands in | First caller |
|---|---|---|
| `expected_expression` | §11.1, Phase 7 | `parse_statement_list`, then the Pratt loop's two sites in Phase 8 |
| `expected_statement_separator` | §11.1, Phase 7 | `parse_statement_list` |
| `expected_selector`, `expected_selector_on_the_left`, `selector_after_line_break` | §11.4, Phase 10 | the `$`, `@`, `::`, `:::` arms of the Pratt loop |
| `expected_parameters`, `expected_parameter` | §11.5, Phase 11 | `parse_function_definition` |
| `expected_condition` | §11.6, Phase 12 | the `if` and `while` headers |

Three rules hold across all of them, and the split above does not weaken any:

- **Every diagnostic is a named function in `diagnostics.rs`, never inline in a
  syntax module.** It keeps wording consistent and makes the whole diagnostic
  surface reviewable in one sitting.

- **Only the *specification* moves to the feature's section; the *definition*
  still goes in `diagnostics.rs`.** Splitting the file per phase would trade the
  one-file review for nothing.

- **A constructor takes `&RParser` only when its message depends on source
  text.** Everything else takes ranges and builds with `ParseDiagnostic::new`.
  `err_builder` is not used in this crate: `biome_parser` defines it as
  `ParseDiagnostic::new` with a `&self` it never reads, so it makes a pure
  constructor look parser-dependent and hides which ones actually are. Under this
  rule the seven lexer diagnostics and `expected_statement_separator` are pure,
  and `expected_expression` takes `p` because `expected_node` reads `p.text(range)`
  to name the token it found instead.

The sink stays at the call site. `p.error()` is the only thing that reaches
`ParserContext`, and it is not neutral — it drops a diagnostic that starts where
the previous one starts — so nothing in `diagnostics.rs` pushes.

---

## 10. Phase 6 — Test harness and corpus import — LANDED

`aether_r_parser_tests` (the `Document` format, `check`, `run_file`), the
`gen_corpus_tests!` proc-macro, `xtask accept`, and 126 imported tests — 89 from
tree-sitter-r and 37 of air's snapshots — every one `[pending]`. The oracle is
**not** part of this phase and has not landed; §10.4 is still that PR's spec.

`parse_root` gained a placeholder that wraps the whole file in one `R_STATEMENT`
holding one `R_BOGUS_EXPRESSION`, and raises one diagnostic. Phase 5 left it
consuming nothing, and the tree sink folds whatever it does not consume into the
`EOF` token's text — so every corpus input would have round-tripped, raised
nothing, held no bogus node, and counted as a *clean* parse, failing every
`[pending]` marker. §11.1 deleted the placeholder and its diagnostic when
`parse_statement_list` landed.

### 10.1 The markdown format

One **pair** of files per concept (§10.3.1), one named test per `##` heading.
Files are fine-grained — one test in a file is fine.

**Two files, never one.** `X.md` is human territory and no tool writes to it;
`X.snap.md` is machine-generated and no human edits it. A reviewer can then see at
a glance which half of a diff was authored and which half was derived, and no test
runner rewrites the file that defines the tests.

The `.snap.md` double extension (rather than air's `calls.R.snap` convention) is
deliberate: GitHub still renders it as markdown, which is most of the point of
using markdown for snapshots at all. They are deliberately **not** marked
`linguist-generated` — that collapses them behind "load diff", and a snapshot
diff is exactly what a parser change should be reviewed through. `.gitattributes`
pins `*.md` to `eol=lf` instead, since the renderer emits `\n` while fence
contents and prose keep the file's own line endings.

A heading's body is an ordered sequence of two kinds of element, in any
combination:

- **` ```r ` blocks.** One heading may hold several. **Each is parsed
  independently** — separate programs that happen to be related, not one program
  split across cells. Most headings have exactly one.

- **Prose.** Anything that is not a fenced block. Copied verbatim into the
  snapshot and otherwise ignored — never parsed, never a test. Write it before the
  block it explains.

Nothing may precede the first heading. The path already says what the file is
about and provenance lives in `MANIFEST.toml`, so a file-level preamble is an
error where prose under a heading is not.

**Prose is for why a test exists**, which is exactly what an R comment cannot
carry: a comment inside the ` ```r ` cell is *input*, so it changes the tree,
lands in the `cst` cell as trivia, and has to round-trip. Prose costs the parser
nothing.

The snapshot **re-emits the heading, the prose, and the ` ```r ` cells** rather
than referring back to the input file. That redundancy is the feature: a `cst`
block is unreadable without the source right above it, and a reviewer approving a
snapshot diff should never have to open a second file to know what they are
approving. Each block's generated cells follow **that block**, not the end of the
heading, so source and tree stay adjacent however many blocks a heading holds.

| Cell | File | Contents |
|---|---|---|
| prose | input (authored), snapshot (echoed) | ignored by the harness; never parsed |
| ` ```r ` | input (authored), snapshot (echoed) | the R source; one parse per block |
| ` ```cst ` | snapshot only | `format!("{:#?}", parse.tree())` — the *typed AST* view, which is what air's existing snapshots show and what makes tree shape reviewable |
| ` ```diagnostics ` | snapshot only | rendered `ParseDiagnostic`s, trailing whitespace stripped so an editor cannot rewrite a committed snapshot; the cell is omitted entirely when there are none |

The AST view, not the raw `SyntaxNode` view. Air's snapshots show both; the AST
view is far more readable and the raw view adds little once the round-trip
assertion is in place. There is no opt-in raw cell until something needs one.

**Markers** go in the heading, in `[brackets]` after the name, in the **input**
file. They are echoed into the snapshot so it reads standalone, but the input file
is the only place they are edited. A name may itself end in brackets, so only a
trailing bracket group whose contents hold no further brackets is read as markers.

**A marker applies to every block under its heading.** The heading is the unit of
intent: if two snippets need different markers they belong under different
headings, which is already how §10.3.1 groups the accepted and rejected forms of a
construct — side by side in one file, under names that say which is which. A
per-block override would put two marker syntaxes in the format to save a heading.

| Marker | Meaning |
|---|---|
| *(none)* | Every block must parse with **zero** diagnostics and no bogus nodes. |
| `[pending]` | Feature not implemented yet. Harness asserts **no** block parses cleanly. **An unexpected clean parse is a hard failure.** Each block is echoed followed by `*Not yet supported.*` in place of its generated cells. Round-trip still applies (§10.2) — a pending test may fail to parse, but must never lose a token. |
| `[error]` | Every block must produce at least one error diagnostic. Fails if any parses cleanly. |
| `[deviation]` | Excluded from the tree-sitter oracle comparison (§10.4). Orthogonal to the others — combine as `[error, deviation]`. |

**The pairing invariant.** The snapshot's heading sequence must match the input's
exactly — same names, same order, same markers, same number of blocks. It is
asserted as a `shape()` comparison *before* any content diffing, so a drifted pair
is not reported as a confusing content diff. Block count is in there so that
adding a block to an existing heading fails the same loud way adding a heading
does. Prose is deliberately *not*: it is echoed rather than derived, so a prose
edit shows up as an ordinary content diff that `just accept` fixes, and folding it
into the assertion would only make the message worse.

Names must be **unique within a file**, and `Document::parse` rejects a duplicate
outright with the offending name in the message. The harness and both manifest
checks address tests by `(file, name)`, so a collision is not cosmetic — it makes
`every_imported_test_still_exists` satisfiable by the wrong test.

### 10.2 The harness

`Document`, `check`, `corpus`, `manifest`, and `run_file` are library code in
`crates/aether_r_parser_tests/src/`, with the corpus in that crate's `tests/`.
Library rather than `tests/`, because `xtask accept` needs `Document` and `check`
too and cannot reach into another crate's `tests/` directory. The crate depends on
`aether_r_parser`; nothing depends on it except `xtask`, so the graph stays
acyclic and `aether_r_parser` ships to air with no test-only dependencies.

**This harness is for the parser only.** The lexer keeps its own `#[cfg(test)]`
suite inside `aether_r_parser` (§8.6): those tests read private fields, and a
token stream is small enough to assert exactly rather than snapshot. There is no
lexer corpus and `xtask accept` never looks at `src/`.

**Tests never write to disk.** nextest runs each test in its own process, in
parallel, so anything in the test path that writes files is a latent race. The
input/snapshot split makes this natural rather than a rule to remember: tests
*compare*, `xtask accept` *writes*, and the two never touch the same file. There
is no `AETHER_UPDATE_SNAPSHOTS` env var.

The markdown parser is hand-written and line-oriented — no markdown crate, since
what is needed is byte-exact round-tripping and general markdown parsers do not
give that. Two rules are what make `xtask accept` idempotent, which CI depends on
(§5.2), since it regenerates and then asserts a clean tree:

- **Prose is stored verbatim, minus its boundary blank lines**, and re-emitted
  with exactly one blank line before and after. Internal blank lines survive, so a
  multi-paragraph note round-trips, and loosely spaced input converges after one
  `just accept` rather than gaining a blank line on every pass.

- **A block's generated cells belong to the block, not the heading.** Rendering is
  a single walk of `elements`, emitting prose as-is and each block as its ` ```r `
  cell followed by its cells. There is no separate generated section to keep in
  sync with anything.

Two shapes are rejected outright: anything before the first `## `, and a heading
with no ` ```r ` block. The second parses fine and tests nothing, which is exactly
the failure the `[pending]` marker exists to make loud, so it must not be reachable
by forgetting a cell.

#### 10.2.1 What `check()` enforces, in order

1. **Round-trip**, on EVERY block — including `[error]` and `[pending]` ones.
   Pending inputs exercise the recovery paths hardest, so they are where dropped
   tokens are most likely; a parser that cannot yet parse a construct must still
   reproduce it verbatim. This is invariant 1 in `CLAUDE.md`, and "for ALL inputs"
   is meant literally.

2. **The marker contract** of §10.1's table. `[pending]` returns early here, so
   nothing below applies to it: a pending block may legitimately produce
   undiagnosed bogus nodes before its feature exists.

3. **No bogus node and no missing required slot**, on any block not marked
   `[error]`.

4. **Every bogus node is overlapped by a diagnostic that points at it** —
   invariant 2. This has to be per-node overlap rather than "are there any
   diagnostics at all": a recovery path that produces a bogus node without calling
   `p.error(...)` yields a tree that round-trips and satisfies its `[error]` marker
   via some *unrelated* diagnostic, and the whole-parse form would be satisfied by
   the very diagnostic that masks the problem. In practice it only bites on
   `[error]` tests, since step 3 has already rejected bogus nodes everywhere else —
   which is exactly where it is needed. It matters most in phases 7–13 while
   recovery is being written, not in Phase 14, by which point the bad snapshots are
   already committed.

Three things about step 4 that the code states without explaining:

- **Overlap, not equality.** A recovery that consumes three tokens may legitimately
  be explained by a diagnostic anchored on only the first of them.

- **Only primary ranges count.** `with_detail` labels live in the diagnostic's
  advices, not in `location()`, so `expected_selector`'s secondary label on the
  operator does not contribute to overlap. That is the conservative reading and it
  is intentional; do not "fix" it by walking advices, or a diagnostic that merely
  gestures at a bogus node will start counting as an explanation of it.

- **Empty slots are out of scope.** A missing required slot is not a node and has
  no range to overlap, so it cannot participate. Step 3 rejects it on every clean
  block, and the designed error paths emit a diagnostic alongside it — `a$`
  produces a missing `right` slot *and* `expected_selector` (§11.4).

Step 3's detection is one predicate: the AST dump contains `missing (required)`,
or some descendant node's kind `is_bogus()`. Air's `has_bogus_nodes_or_empty_slots`
(from `crates/air_r_parser/tests/spec_test.rs`) is deliberately not used — its
empty-slot half only inspects *list* nodes, so a missing required slot on an
ordinary node, which is the `a$` case §11.4 cares about, is invisible to it.
**One gap is open here**, raised in review and not yet settled: air's helper did
cover empty slots in list nodes and the dump grep does not, which is the coverage
§7 assumed when it made `R_STATEMENT_LIST` a list kind.

#### 10.2.2 Test registration

One `#[test]` per corpus **input** file, generated by `gen_corpus_tests!` globbing
the tree, so adding a file needs no registration edit. It is a small proc-macro
crate, `crates/aether_r_test_macros/`, modelled on air's `tests_macros::gen_tests!`;
the glob skips `*.snap.md`, or every snapshot would get its own bogus test. Module
paths mirror directories, which makes nextest filters read naturally:

```bash
cargo nextest run -E 'test(corpus::expressions::extract)'
cargo nextest run -E 'test(/corpus::control/)'          # all control-flow files
```

Per-file is the right granularity to start. Because the tests are read-only,
upgrading to one `#[test]` per `##` heading later is safe — a heading stays the
unit either way, however many blocks it holds — by parsing headings at expansion
time.

### 10.3 The corpus

The importer was a throwaway `xtask` subcommand: written, run once, and never
committed — settled with the owner. The corpus is the artifact; a live dependency
on two sibling checkouts is not, and `MANIFEST.toml` is what makes re-deriving the
placements unnecessary. Two rules it followed still govern hand-written tests:

- **A fenced cell cannot express a leading blank line or a missing final
  newline**, so bodies are trimmed of blank lines at either end and given exactly
  one trailing newline. Several tree-sitter bodies opened with a blank line and
  several of air's `.R` files ended without a newline; neither is a deliberate test
  of anything, and the lexer's own suite already covers EOF with no trailing
  newline (`1 #`, `#`).

- **A tree-sitter body is never split into several blocks**, even though §10.1
  allows several. The reason is semantic rather than stylistic: a bundled body is
  *one program*, so its snippets parse together and the tree records how they sit
  beside each other. Splitting them would parse each alone and quietly test
  something else. Multi-block headings are for tests aether writes itself, where
  the grouping was never a single program to begin with.

#### 10.3.1 One file per concept

tree-sitter-r has only four corpus files because its test runner needs it that
way. That is an artifact of its infrastructure, not a meaningful grouping, and it
is the reason `expressions.txt` is a 49-test grab bag. **Aether splits by concept
instead**, roughly mirroring `src/syntax/`, and files may be as small as one test.

- **The `-errors` split disappears.** Error expectations are carried by the
  `[error]` marker on each test, not by which file it lives in, so `Not A
  Parenthesized Expression 1` sits directly beside `Parenthesized Expression`. That
  is strictly better for review: you see the accepted and rejected forms of a
  construct together.

- **Turning on a feature is usually one directory.** Phase 11 flips the markers in
  `functions/*.md` and nothing else. The phase-to-directory correspondence below is
  deliberate.

#### 10.3.2 Where the imported tests landed

`MANIFEST.toml` records every placement and a test enforces it (§10.3.3), so it is
the answer to "where did test X go" — not a table here, which would only rot. What
the manifest does not carry is which phase owns which directory:

| Phase | Files |
|---|---|
| 7 | `literals/*.md` |
| 8 | `expressions/{unary,binary,colon,specials,formula,help,assignment,precedence}.md`, `statements/newlines.md` |
| 9 | `expressions/{call,subset,subset2,parenthesized}.md`, `recovery/unmatched-delimiters.md` (finalised in 14) |
| 10 | `expressions/{extract,namespace}.md` |
| 11 | `functions/*.md` |
| 12 | `control/*.md` |
| 13 | `statements/{braces,semicolons}.md` |

**Counts:** literals 19, expressions 40, functions 7, control 11, statements 2,
recovery 10 — 89 from tree-sitter-r, plus 37 of air's (§10.3.4), for the 126
entries in `MANIFEST.toml`.

Three placements are judgement calls, noted so a reviewer is not surprised:

- **`Subset and Subset2 Precedence`** is in `precedence.md` rather than
  `subset2.md`, since it is about binding order. It is the one Phase 9 test in a
  Phase 8 file.

- **The two `#200` `else`-prefix identifier tests** are in `control/if.md`, not
  `literals/identifiers.md`. They are lexer tests in substance, but their R source
  is an `if`/`else` block and they only fail in that context.

- **`Dollar, At, Namespace, Namespace Internal With Expression RHS`** (three tests)
  exercise all four operators at once and live in `extract.md`, so `namespace.md`
  looks thinner than it is.

`[error]` is carried by everything that came from an `-errors.txt` file, plus three
that sit in `expressions.txt` because tree-sitter accepts them and deviations 2 and
3 require the RHS: `Dollar No RHS`, `Slot No RHS`, and `Namespace With Missing
RHS`. Those three are also the `[error, deviation]` ones.

#### 10.3.3 `MANIFEST.toml` — the anti-drop guard

Every imported test is listed with the `source` it came from, whatever project
that was, so no naming convention inside the `.md` files has to carry provenance.
Two tests guard it:

- `every_imported_test_still_exists` — each entry's name is still a heading in the
  file the manifest names, and exactly 89 entries have a `tree-sitter-r/` source.

- `no_imported_test_is_duplicated` — each imported name appears in exactly one
  corpus file. Copying a test into a second file forks it silently otherwise: one
  copy drifts while the first check stays green.

Renaming an imported test or moving it between files is then a deliberate two-file
change: the `.md` and the manifest. That is the point.

Tests aether writes itself in later phases are simply absent from the manifest and
need no bookkeeping — they are new, so there is no prior version of them to lose.

#### 10.3.4 Air's own snapshots

Air's `crates/air_r_parser/tests/snapshots/{ok,error,undefined}/**/*.R` is 39
files, of which **37 merged into the same concept files**, not a separate
`air-regressions.md` — a regression test about calls belongs next to the other call
tests. They carry no marker of where they came from: a test's name should describe
the R it exercises, and a reader scanning `expressions/call.md` has no reason to
care which upstream project a case arrived from. Where a name was already taken,
the importer disambiguated by what the test covers — `Calls With Trailing Comma`,
not `Calls (air)`. The `undefined/*` ones became `[error]`, since that directory
exists in air precisely because nobody was happy with the current silent behavior
(deviation 2).

**The two `ok/crlf/*.R` files stayed out of the corpus**, settled with the owner. A
` ```r ` cell would have to hold literal CRLF bytes, and keeping those alive
through git, an editor, and the renderer costs more than the two tests are worth.
They are ported into `aether_r_parser`'s own `#[cfg(test)]` suite instead, where
the source is a Rust string literal that nothing can rewrite: `trailing_comments.R`
as a trivia test in `token_source/tests.rs`, and `multiline_string_value.R` as a
lexer test in `lexer/tests/strings.rs`. They are absent from `MANIFEST.toml`, which
addresses tests by corpus file, and this paragraph is the record that they were not
dropped.

**A merged snapshot may lag its file's nominal phase.** Air's files were written to
exercise a feature, not to stay within one, so `ok/comments.R` is `1 + 1 # Trailing`
and `ok/dots.R` contains `{ ... }` and `...()`. Landing them in `literals/` is still
right — a comment test belongs with the other comment tests — but it means the
phase that owns a *directory* does not necessarily turn every heading in it green.
Leave the stragglers `[pending]` and let the phase that implements what they need
flip them. The per-phase counts in §11 are of imported tree-sitter tests for this
reason.

### 10.4 The oracle — LANDED

`aether_r_oracle::compare(source, actual)` parses the source with air's
tree-sitter-r-backed parser and walks that tree against aether's, returning
`Match`, `Mismatch { path, expected, actual }`, or `OracleUnavailable`. `check()`
calls it on every block, and the failure names the first divergence:

```
R_ROOT > [0] > R_STATEMENT_LIST > [0] > R_DOUBLE_VALUE
  expected: R_DOUBLE_VALUE @ 0..1
  actual:   R_BOGUS_EXPRESSION @ 0..1
```

**Air's crates are a dependency, not a vendored copy.** Vendoring `treesitter.rs`
and `parse.rs` is not possible: the walker builds air's syntax kinds, and aether
deleted the two it emits for lists (deviations 1b and 1c). Hand-porting them would
put aether's own decisions inside the implementation the oracle exists to check
against. Air and aether pin
the same biome rev and the same tree-sitter-r rev, so `air_r_parser` resolves as a
git dependency with nothing to reconcile. Re-pin the rev if air's walker changes.

**Normalization is one canonical pass applied to both trees**, not a
per-deviation list:

- Rename `R_EXPRESSION_LIST` to `R_STATEMENT_LIST` and `R_BRACED_EXPRESSIONS` to
  `R_BRACED_STATEMENTS`. A mechanical diff of the two kind vocabularies says
  these are the only renames. `R_STATEMENT`, `R_BOGUS_SELECTOR`, and
  `ERROR_TOKEN` are aether-only and have no counterpart to compare against.

- Splice out `R_STATEMENT` wrappers and drop `SEMICOLON` tokens, which is
  deviation 1. This is what lets the semicolon-free majority of the corpus keep
  oracle coverage rather than being skipped wholesale.

- Collapse `R_STRING_VALUE`'s children to one synthetic `STRING` token spanning
  the node, which is deviation 12. Air's three tokens and aether's one meet
  there, and the comparison still pins where every string starts and ends —
  which is what the raw-string delimiter logic gets wrong when it gets anything
  wrong.

**Whitespace polarity (deviation 10) is handled by comparing trimmed ranges**
rather than by re-tagging trivia on both trees. It reaches the same end — polarity
cannot cause a mismatch — with no re-tagging code. Comment attachment is then
compared *explicitly*, as the leading and trailing comment lists on each token, so
the half that reaches air's formatter stays checked. That distinction is the
point: a comparison written over "trivia" rather than over whitespace would blind
the oracle to the one kind of attachment drift with user-visible consequences.

Walking is by child index, not slot index. Splicing out `R_STATEMENT` gives the
two trees different slot counts by construction, so slot-index walking cannot
work; paths carry `[i]` segments instead of slot names.

**Three things skip the comparison**, and between them they cover every
intentional difference:

1. **Aether did not parse the block cleanly.** `docs/deviations.md` assumes this
   — deviation 7's `1Li` is `float 1` plus `identifier Li` to tree-sitter and two
   expressions on one line to aether, and tree-sitter accepts it, so nothing else
   would skip it.

2. **tree-sitter bailed**, which it does on any input with an error, returning a
   whole-file bogus node. `compare` reports `OracleUnavailable` and `check()`
   treats it as a skip. Error recovery is exactly where aether is *supposed* to
   differ.

3. **The block is marked `[deviation]`**, for the differences that survive a clean
   parse on both sides: the semicolon tests and `Specials`, plus the
   extract/namespace missing-RHS tests that deviations 2 and 3 turn into errors.

Only the third needs a marker, which is why the corpus carries just twelve of
them. That branch is also the one with no test yet: proving it needs a block that
parses cleanly in aether *and* disagrees with tree-sitter, which the placeholder
parser cannot produce. Phase 8's `Specials` and Phase 13's semicolons are the
first real cover for it.

#### 10.4.1 Scope: temporary

**Settled with the owner: the oracle is deleted once the conversion is
complete.** It is a development aid for catching drift while the parser is being
written — most valuably comment attachment (§13) — not a permanent correctness
gate.

It is **not** feature-gated and **not** excluded from CI, which is a deliberate
trade: a crate scheduled for deletion does not earn the build machinery it would
take to isolate it. The cost is that every build, CI included, compiles air and
tree-sitter-r, and needs a C toolchain for the latter. There is no `just oracle`
recipe, since with no feature to enable it would be identical to `just test`.

**Do not defend it.** Because it is scheduled for deletion, the oracle does not
earn much maintenance. If a Biome rev bump or a change in air breaks it, the
reasonable responses are to fix it cheaply or to stop using it — not to spend a
day restoring a tool whose remaining life is a handful of phases. What must
survive its removal is the *reviewed snapshots*, which is why the phase checklist
(§4) calls for eyeballing every changed cell rather than trusting the oracle to
catch everything.

The counterpart risk that a local-only tool quietly stops being run is gone: it
runs on every test run, so it fails the moment it disagrees. What replaces it is
the risk that a phase turns a marker on and the oracle goes red for a difference
that is intentional but unmarked — the fix there is a `[deviation]` marker and a
line in `docs/deviations.md`, not a weaker comparison.

### 10.5 `quick_test.rs`

Air's scratchpad, kept because the owner uses it: an `#[ignore]`d test in
`crates/aether_r_parser_tests/tests/` that parses a string literal and prints the
tree and diagnostics. `just quick` runs it — nextest needs `--run-ignored all` and
`--no-capture` to show `println!` output.

---

## 11. Phases 7–13 — The parser, one feature at a time

From here on each phase is small enough to review in one sitting. The shape is
always the same: add syntax functions, add diagnostics, flip `[pending]` markers
off the tests the feature enables, regenerate, review the diff.

### 11.1 Phase 7 — Literals, identifiers, program — LANDED

`syntax.rs`, `syntax/statement_list.rs`, and the one-token arms of
`syntax/expr.rs`.

`parse_root` takes no scope helper, because the defaults *are* top-level
behaviour — newlines are significant and `else` may not follow one (§9.2 on
polarity). Its `parse_statement_list` call replaced Phase 6's placeholder, and
`parse_placeholder_statement` went with it (§10).

**`parse_statement_list` landed here rather than in Phase 13**, because
`parse_root` calls it from this phase onward. What Phase 13 adds is the `;` slot
and empty statements (§11.7); what this phase had to get right was the loop, the
recovery branch, and the **statement separator rule** — R separates statements by
a newline or a `;` and by nothing else. Deferring that would have let `1 2` parse
clean through Phase 8, where `statements/newlines.md` goes green.

**`ERROR_TOKEN` is an arm of `parse_primary_expression` rather than a job for
statement-list recovery**, and that is what keeps the damage to one token:
recovery would skip to the next statement boundary and swallow whatever followed
the stray character on the same line. It needs no entry in any recovery set,
though — it starts no production, so every `at(...)` test in the parser is already
false for it, and the arm is the only way to reach it.

`unterminated_string` carries the two-range shape
`unterminated_backtick_identifier` already uses: the opening delimiter, with what
was found instead as a secondary label. A string runs to EOF for the same reason
a backticked name does (§8.4), and both are raised by the lexer, which is the
only layer that still knows a delimiter was opened.

**A string is one more one-token arm.** The lexer hands back a single token, so
`R_STRING_VALUE` has one slot and there is nothing to decide:

```rust
R_SINGLE_QUOTED_STRING_LITERAL | R_DOUBLE_QUOTED_STRING_LITERAL | R_RAW_STRING_LITERAL => {
    let m = p.start();
    p.bump_any();
    Present(m.complete(p, R_STRING_VALUE))
}
```

An earlier draft gave it a `parse_string_value` of its own in `syntax/value.rs`,
which is why the layout above no longer lists that file. Every caller either
already knew it had a string or had to spell out the same three literal kinds to
find out, so the function only ever moved a bump one level down and forced its
callers to launder a `ParsedSyntax` that could not be `Absent`.

### 11.2 Phase 8 — Binary and unary operators (the Pratt core) — LANDED

`src/syntax/expr.rs`. This is the heart of the parser; everything else hangs off
it.

#### The precedence table

Taken from R's `src/main/gram.y`. Higher binds tighter.

| Rank | Name | Operators | Assoc |
|---|---|---|---|
| 1 | HELP | `?` (binary) | left |
| 2 | LEFT_ASSIGN | `<-` `<<-` `:=` | **right** |
| 3 | EQUALS_ASSIGN | `=` | **right** |
| 4 | RIGHT_ASSIGN | `->` `->>` | left |
| 5 | TILDE | `~` | left |
| 6 | OR | `\|` `\|\|` | left |
| 7 | AND | `&` `&&` | left |
| 8 | UNARY_NOT | `!` (unary) | left |
| 9 | COMPARISON | `<` `<=` `>` `>=` `==` `!=` | left (nonassoc in R) |
| 10 | PLUS_MINUS | `+` `-` (binary) | left |
| 11 | MULTIPLY_DIVIDE | `*` `/` | left |
| 12 | SPECIAL_OR_PIPE | `%...%` `\|>` | left |
| 13 | COLON | `:` | left |
| 14 | UNARY_PLUS_MINUS | `+` `-` (unary) | left |
| 15 | EXPONENTIATE | `^` `**` | **right** |
| 16 | EXTRACT | `$` `@` | left (see below) |
| 17 | NAMESPACE | `::` `:::` | left (see below) |
| 18 | CALL | `f(…)` `x[…]` `x[[…]]` | — (postfix) |

**Ranks 16 and 17 arrived in Phase 10 (§11.4)**, once `$`, `@`, `::`, and `:::`
had a right-hand side of their own.

**`function`, `while`, `for`, `repeat`, and `if` get no rank.** R's grammar gives
them one, and it does one job there: make the body greedy, i.e. say how far right
`function(x) …` or `if (c) …` extends. A recursive-descent parser answers that by
construction — the body is a plain `parse_expression` call that runs to its
natural terminator (§11.5, §11.6).

The one case where a greedy body and a body parsed at the lowest binding power
would differ is the binary `?` at rank 1: `function() a ? b` could group either
way. R's grammar makes a function body `expr_or_assign_or_help`, which includes
help and so points at greedy, but do not take that on faith: let the oracle settle
it the first time it runs against `functions/definition.md`, and record the answer
here if it is not greedy.

**Rank 18 appears in no lookup table.** Postfix is handled structurally in the
loop and applies unconditionally, so it is never compared against a binding power,
and the number survives only as `BindingPower::HIGHEST` — which exists to give the
`debug_assert!` in `parse_expression_with_binding_power` something to assert
*against*. The assertion holds because the incoming power is `LOWEST` from
`parse_expression`, a prefix power (max 14) from `parse_prefix_expression`, and
otherwise the loop's own right-hand power, which is `bp + 1` for left
associativity and tops out at 18 because the highest infix rank is `:::` at 17.
Adding an infix operator at 18 or above breaks that, which is what the assertion
catches; the fix is to renumber so postfix stays tightest, not to relax the
assertion.

**Associativity of `$` `@` `::` `:::`.** tree-sitter-r forces these **right**
associative and explains why in a long comment: it is a workaround for the
`optional()` RHS, so that `foo::bar` prefers the longer match `[foo::bar]` over
`[foo::][bar]`. Since aether makes the RHS **required** (deviation 2), that
workaround is unnecessary and we use **left** associativity, matching R's
`gram.y`. The resulting trees should be identical — the RHS is restricted to a
selector, so `a$b$c` can only nest one way — but say so in the PR and let the
oracle confirm it.

#### What the loop is shaped like, and why

**Every arm of `parse_primary_expression` returns, and none falls through to a
shared tail.** The one-token arms repeat `start`/`bump_any`/`complete` rather than
naming a kind for a tail below the `match`, and that is the point: an arm can be
read on its own, and the `match` is uniform — token in, `ParsedSyntax` out, once
per arm. Hoisting the tail would save about fifty lines and buy back a two-tier
read where some arms produce a value and others produce a node.

**`Absent` from `parse_primary_expression` is a result, not a failure.** It is how
`f(,,)` gets its empty arguments, how `f(a = )` gets its empty value, and how
`parse_statement_list` knows to recover. Only the caller knows whether an operand
was required, so the function raises no diagnostic of its own — `ERROR_TOKEN` is
the one arm that could and deliberately does not, because the lexer has already
pointed at the character (§11.1).

**Precedence facts the corpus pins**, in `Operator precedence` and
`Precedence Tests`:

- `-1^2` → `-(1^2)` — unary minus (14) binds looser than `^` (15).

- `!a & b` → `(!a) & b` — `!` (8) binds tighter than `&` (7).

- `-2:3` → `-(2:3)`? **No** — unary `-` is 14 and `:` is 13, so it is `(-2):3`,
  the sequence from -2 to 3. Contrast `-1^2` above: `^` (15) outranks unary `-`,
  `:` does not.

- `a <- b <- c` → `a <- (b <- c)` — right assoc.

- `a -> b -> c` → `(a -> b) -> c` — left assoc.

- `2^3^2` → `2^(3^2)` — right assoc.

- `a %in% b + c` → `a %in% (b + c)`? **No** — `%in%` is 12, `+` is 10, so
  `(a %in% b) + c`. This one trips people up.

The newline behaviour of the operator table is a separate axis: every operator
here eats a following newline except `::` and `:::`, and that exception is itself
suspended inside `(`/`[`/`[[`. Both landed in Phase 10, and §11.4 has the rule and
the two tests that pin it.

**A construct split across a newline settled without a deviation.**
`Not A Call, Subset, Or Subset2` arrives from `expressions-errors.txt` as
`[error]`, and under R its `f\n(x)` is two perfectly valid statements. The
separator rule agrees — that half parses clean, as an identifier and then a
parenthesized expression. What makes the block an error is `foo\n[bar]`, where `[`
starts no expression at all. So aether errors where tree-sitter-r errors, the
marker holds, and nothing moved into `docs/deviations.md`.

#### The diagnostics this phase adds

None of its own. The Pratt loop is what `expected_expression` was specified for —
an infix operator whose right-hand side never arrives, and a prefix operator with
nothing to apply to — but `parse_statement_list` needs it a phase earlier, so it
landed in §11.1 and §9.4's table says so. Later phases reuse it rather than
wording their own: the body of a `function`, the branches of an `if`, and a
statement that turns out to be only a `;` all mean the same thing to the reader.

### 11.3 Phase 9 — Calls, subset, subset2 — LANDED

`src/syntax/call.rs`, `src/syntax/parenthesized_expression.rs`, and
`parse_postfix_expression` in `expr.rs`.

**`(` is the one token in this phase with two jobs**, and the Pratt structure
separates them with no lookahead at all. `parse_postfix_expression` meets it with
an operand already to its left and builds an `R_CALL`; `parse_primary_expression`
(§11.2) meets it where an operand is due and builds a group. `f(x)` and `f (x)`
are both calls; `f\n(x)` is neither, because the statement break ends the loop
before postfix is reached, and it becomes two statements exactly as R reads it.

The body of a group is a full `parse_expression`, not one restricted to §11.2's
table: R's `'(' expr_or_assign_or_help ')'` admits `=` and `?`, so `(a = 1)` is an
assignment and parses clean. `()` has no such excuse and takes the
`expected_expression` diagnostic, then the `expect` closes the node.

#### `[[` and `]]` — the one genuinely hard tokenization problem

tree-sitter-r solves this with an external-scanner scope stack, and it has a filed
bug about it (#177, `Unsupported Subset2 With ]] Over Multiple Lines`) plus four
errors-corpus tests about mismatched brackets.

**Greedy openers, one-at-a-time closers, joined by a re-lex.** §8.4 makes the case
for the asymmetry and names the precedents; what it means here is that a run of
closing brackets reaches the parser as single `]` tokens, and the parser puts a
pair back together wherever the scope says both belong to the same `[[`.
`L_BRACK2` arrives whole; `R_BRACK2` is manufactured.

**The join happens on every bump, at one call site, off the scope** — the tail of
`RParser::do_bump()`, which `expect`, `bump_ts`, and the skipping bumps inside
`ParseRecoveryTokenSet` all funnel through. `biome_js_parser` overrides the same
method for its keyword-escape check. The join fires only inside a `[[`, where the innermost open construct owns
both brackets, and only on brackets that are adjacent, so `x[[a] ]` and `x[[a]\n]`
stay errors. §9.2's placement rule is what makes one site sufficient: because a
bump lexes the *next* token, the opener is consumed inside the scope and the
closer outside it, which is why `parse_subset2_arguments` bumps `[[` inside
`with_scope` and `expect`s `]]` outside it.

So a terminator check is a plain predicate. `ARGUMENT_LIST_ENDS` names every
closing delimiter rather than the one the caller opened, and
`ARGUMENT_LIST_BOUNDARIES` derives from it by adding `,`, so the recovery set is
computed rather than restated.

**The list does not know which closer is its own, and that is the point.** It
stops at every one of them; `p.expect(close)` in the caller is the only place that
knows which was expected, and it reports the difference. A list that cannot name
its terminator cannot leak it either — eating one that belongs to an enclosing
construct would strip it of its terminator and turn one error into a cascade, the
same rule as `a$)` in §11.4. `parse_statement_list` (§11.7) takes the same shape,
a bare `close` and a const recovery set, for the same reason. An argument list
stopping at a foreign closer costs it its own terminator, and `fn(}` in
`expressions/call.md` pins the shape: an empty `RArgumentList`, a missing `)`, and
the `}` surviving as a statement of its own.

**The join was first written into the argument list's terminator check, and that
was wrong.** Worth recording, because the reasoning that put it there is
seductive: only something that knows the nesting can tell a `]` that closes *this*
list from a `]` that closes someone else's, and the terminator check is the one
thing that knows. True — but it made the check side-effecting, which made calling
it an obligation at every exit of the loop, and the recovery branch grew an exit
that did not. `x[[a b]]` lost its terminator and stranded both brackets in the
enclosing statement list. The scope knows the same fact and knows it everywhere,
so the obligation disappears rather than being documented. §12.1 carries the
general audit rule that falls out of it.

**Nothing is ever undone, and the ladder cases confirm it** — worth stating,
because it is where a reader will stop. `x[[a[b]]]` lexes as
`x` `[[` `a` `[` `b` `]` `]` `]`. The inner `[` is not a `[[` scope, so its `]`
arrives unjoined; consuming it lands the parser back in the `[[` scope, where the
two that are left join. `x[[a[[b]]]]` is the same story twice. Because a join
reads forward from the current token and never rewinds past it, no split, merge,
or unget is ever needed.

The nesting ladder lives in `expressions/subset.md` as aether-authored tests:
`x[a[b]]`, `x[[a[b]]]`, `x[a[[b]]]`, `x[[a[[b]]]]`, `x[y[z[[a]]]]`. All clean
parses, no markers — which is the point, since the failure mode here produces
errors on *valid* code and the imported corpus only covers mismatched brackets and
`#177`. The last rung is the one that pins §9.2's placement rule: it leaves two
`]` that close two different `[` immediately after a `]]`, which is exactly what a
`[[` scope outliving its own closer would wrongly join. The shorter rungs cannot
catch that — they leave only one `]` behind the `]]`. `x[[]]` in
`expressions/subset2.md` pins the other half, where the join has to land on the
bump of the `[[` itself.

**`re_lex` against buffered lookahead** is live, not hypothetical.
`TokenSourceWithBufferedLexer` caches lookahead tokens, and re-lexing the current
token must invalidate that cache; `BufferedLexer::re_lex` rewinds the inner lexer
and clears the lookahead whenever the kind changes, which is the contract
`biome_js_parser` relies on for `>>`, and `RTokenSource::re_lex` delegates
straight to it. `parse_argument`'s lookahead (§11.3) meets it head-on: in
`x[[a]]`, the token buffered past the name *is* the `]` the next bump asks to
join. `test_re_lex_after_lookahead` in `src/token_source/tests.rs` pins the
mechanism and `Lookahead For `=` Survives The `]]` Join` in
`expressions/subset2.md` pins it end to end. Both matter on a fork rev bump,
because a stale cache here yields a wrong tree that still round-trips, which is
precisely the class of bug no invariant in this project catches.

The mirror case (`x[[a]` — a `[[` that should have been two `[`) is a genuine
syntax error in R, and gets a diagnostic and recovery rather than cleverness.

#### Argument lists and holes

R's argument syntax is unusually permissive and the `.ungram` already documents it
precisely. The listed behavior is reproduced exactly:

```
Allowed:      fn()  fn(,)  fn(,,)  fn(a)  fn(a, b)  fn(,,a,,b,,)
              fn(a, # comment_between_commas
              ,)
Not allowed:  fn(a b)
```

Four things about `parse_argument_list` are not obvious from reading it:

- **Progress is asserted across the whole iteration, not per argument.**
  `fn(,,a,,b,,)` means an iteration may legitimately consume only a comma, and an
  empty argument consumes nothing at all, so per-argument is not an assertion the
  loop can make. As written the loop cannot stall — every iteration that does not
  break bumps at least the `,` — but the guard is what keeps that true of the
  next edit, since the earlier shape of this loop *could* stall.

- **The recovery branch is reached by `fn(a b)`, not by `fn(a}`.** A foreign
  closer is in `ARGUMENT_LIST_ENDS`, so it ends the list cleanly; only a token
  that is neither a boundary nor a `,` makes `expect(T![,])` fire and hands the
  rest to `ParseRecoveryTokenSet`. `Side By Side Arguments` in
  `expressions/call.md` is the test.

- **`enable_recovery_on_line_break()` is deliberately not set.** Recovery runs
  inside `(`/`[`/`[[`, where newlines carry no meaning (§9.2).

- **Recovery happens *inside* an `RArgument`**, filling its
  `value: AnyRExpression?` slot. The list is generated as
  `make_separated_list_syntax(kind, children, RArgument::can_cast, …)`, so only an
  `RArgument` may be a child — a bare bogus node would be dropped or would reshape
  the list. `parse_statement_list` (§11.7) has the same constraint and the same
  shape.

**One token of lookahead, and the clause owns the whole decision.** An argument
name and the start of an argument value are the same tokens, so `p.cur()` alone
cannot tell them apart. Every arm of `parse_argument_name_clause` is guarded on
`p.nth_at(1, T![=])` and builds the entire `R_ARGUMENT_NAME_CLAUSE` — name, `=`,
and all — so `parse_argument` reads as the two optional halves `r.ungram`
declares:

```rust
fn parse_argument(p: &mut RParser) {
    let m = p.start();

    parse_argument_name_clause(p).ok();
    parse_expression(p).ok();

    m.complete(p, R_ARGUMENT);
}
```

Each arm repeats the guard, the two markers, and the `bump(T![=])`, and that is
the deliberate half. Hoisting any of it means a token set naming the same five
kinds a second time and a caller that has to re-derive what the callee already
knew; what is left instead is one self-contained arm per name form, which cannot
drift from the kind it guards. It is the same trade `parse_primary_expression`
makes in §11.2, for the same reason.

This is sound because R admits only a bare symbol, string, `...`, `..i`, or
`NULL` as an argument name — `f(a$b = 1)` is not legal — so the name is always
exactly one token. `nth` skips trivia, so `f(a\n= 1)` and `f(a # c\n= 1)` are name
clauses too, which `A Newline Or Comment Between A Name And Its `=`` in
`expressions/call.md` pins.

**An earlier draft parsed the name unconditionally** and, when no `=` followed,
handed it to the Pratt loop as an already-parsed lhs — on the grounds that
`p.nth(1)` would re-lex the head of every argument of every call in the file. It
does not: `BufferedLexer` caches the looked-ahead token and `next_token` pops it
from that cache on the following bump, so each token is still lexed exactly once
and the cost is one `LexerCheckpoint` clone per argument head. That fallback is
what forced `parse_expression_with_lhs` to be `pub(crate)`, and removing it took
the Pratt loop's only external entry point with it. `precede` survives, but only
in its ordinary role of wrapping a node that is already known to be a name.

`==` never reaches the lookahead as two tokens: the lexer's maximal munch (§8.4)
returns a single `==`, so `f(a == 1)` finds no `=` at offset 1 and the `==` is
consumed by the Pratt loop as an ordinary infix operator.

`NULL`-as-a-name needed no special handling in the end. `AnyRArgumentName` reuses
`R_NULL_EXPRESSION` rather than declaring a kind of its own, so the same atom is
valid in both positions and `parse_expression` builds it either way — as it does
for the other four forms.

#### The diagnostics this phase adds

None. `expected_argument` was specified for this phase and turned out not to be
needed. A hole is legal in an argument list, so `f(a,,b)` raises nothing at all;
the missing-comma case (`f(a b)`) is `expect(T![,])`, which words itself; and where
recovery has to fill an argument, the `R_BOGUS_EXPRESSION` it builds is already
overlapped by that same diagnostic. §11.5's `expected_parameter` still lands,
because R rejects `function(a,,b)` — that asymmetry is why the two lists do not
share a constructor.

### 11.4 Phase 10 — Extract and namespace — LANDED

`src/syntax/expr.rs`. Small phase, but it is where aether's first user-visible
deviation lands, so it stayed separate for reviewability.

#### What changed in the Pratt loop

**Three infix functions, not one with three branches.** `parse_infix_expression`
is gone, replaced by `parse_infix_binary_expression`,
`parse_infix_extract_expression`, and `parse_infix_namespace_expression`, each
paired with its own binding power table and tried in turn by the loop. The three
operator sets are disjoint, so branching inside one function bought nothing and
cost a reader the ability to follow any one operator family top to bottom. Only
the binary table carries `Associativity`: the other two take one `parse_selector`
call on the right rather than a recursive `parse_expression_with_binding_power`,
so a bumped rank would never reach anything that compares it.

Rank 16 (`$`, `@`) and rank 17 (`::`, `:::`) sit above `^` and below postfix
(§11.2 on why left-associative rather than tree-sitter-r's right, for the table
that still records it).

Both selector functions capture the operator kind and `p.cur_range()` **before**
`p.bump_any()`, because neither survives the bump and the diagnostics need both —
the kind to name the operator in the message, the range for the secondary label.

`parse_selector` is `AnyRSelector` — identifier, string, `...`, `..i` — with the
same one-arm-per-kind shape as `parse_argument_name_clause` in `call.rs`.

**`::` and `:::` restrict their left operand as well**, which this section did not
originally account for and the four imported error tests demand. It cannot be a
`parse_` call: by the time the operator is in hand, the operand is already a
`CompletedMarker`. `parse_namespace_left` tests it with `AnyRSelector::can_cast`
and, when it fails, diagnoses and wraps it in an `R_BOGUS_SELECTOR` before the
namespace node is preceded. `a::b::c` and `a()::b` keep their operator and their
right-hand name that way. Leaving the bad slot for the factory to notice is the
alternative, and it collapses the whole expression into one
`R_BOGUS_EXPRESSION` — the failure mode §7 added the kind to avoid.

#### The newline rule

**The two newline rows below are the ones to get right, and neither is covered by
an imported test.** R's rule, read off `yylex` in `src/main/gram.y` rather than
inferred from tree-sitter-r:

- `NS_GET`/`NS_GET_INT` are the only operators in aether's precedence table
  missing from R's `EatLines` switch, so a newline after `::` or `:::` is
  significant where a newline after any other infix operator is not. `$` and `@`
  *are* in that switch, so `a$\nb` is legal R and must parse clean — the two
  operator families genuinely differ here and it would be easy to make both
  accidentally permissive.

- The restriction is suspended inside `(`, `[`, and `[[`, because R discards
  newlines in those contexts before consulting `EatLines` at all. `{` pushes
  `LBRACE` and does not get this, so `foo::\nbar` inside braces is still an error.

`newlines_are_significant()` draws the line in exactly that place, which is why
the guard is one clause in the `::` arm rather than a scope walk.

| Source | tree-sitter-r | aether |
|---|---|---|
| `foo::\nbar` | two expressions, no error, since the RHS is `optional()` | error, same shape (deviation 3) |
| `f(foo::\nbar)` | clean | **clean**, matching R |
| `a$\nb` | clean | clean, matching R |

Both aether cases are aether-authored tests: `Namespace Newline Is Significant
Only Outside Parens` in `expressions/namespace.md` and `Dollar Tolerates A
Newline` in `expressions/extract.md`. Neither carries a marker — they are *clean*
parses, which is the surprising half, and the oracle confirms tree-sitter-r
agrees on both, so nothing moved into the inherited **"tree-sitter-r vs. R"**
heading (§8.4).

The `foo::\nbar` row is the one place the guard's diagnostic is not
`expected_selector`. Reporting "expected a name" at the parser's position names
the *next* line's first token, which is usually a perfectly good name, so the
message accuses the wrong thing. `selector_after_line_break` names the newline
instead and anchors on the operator.

#### Recovery: diagnose, leave the slot missing, consume nothing

One rule, and it is the same one for `a$`, `a$1`, `a${b}`, `a$if`, and `a$)`.
`parse_selector(p).or_add_diagnostic(…)` raises `expected_selector` at the
offending token and leaves `right` empty. Whatever follows is not the selector's
problem — the Pratt loop finds no infix operator at `1`, breaks, and `1` becomes
the next statement. `a$1 + 2` follows from that: the `+ 2` reattaches to the
sibling, so the second statement is the whole `1 + 2`.

`docs/deviations.md` entry 2 carries the argument in full, including the two
corpus comments that settle it and the `df$`-eats-the-next-line case. The short
version: an earlier draft consumed one atom into an `R_BOGUS_SELECTOR` to stop the
`1` leaking out as a phantom statement, and that destroys the structure the
imported corpus explicitly calls useful — `foo${bar}` would lose a braced block
and `foo$if` an entire if-statement.

**Not consuming keeps the shape identical**, which is the mechanical payoff.
Consuming makes those rows differ from tree-sitter in *shape*; leaving the slot
missing makes it identical, sibling and all, since empty slots are invisible to
`shape_of`, which walks `children_with_tokens()`
(`aether_r_oracle/src/shape.rs`). What that buys is reviewability rather than
oracle coverage — `check()` skips the oracle for any block that has errors, so
these rows were never compared and dropping their `[deviation]` markers does not
change what runs. It records that the tree is air's.

**The cascading diagnostic this was expected to cost does not happen.** `a$1`
looked like it would raise `expected_selector` and then
`expected_statement_separator`, since `parse_statement_list`'s check fires when
there is no line break between the two statements (§11.1). Both would land on the
`1`, and `Parser::error` drops a diagnostic that starts where the previous one
starts. So `a$1` reads as one complaint, and the pair only appears when the two
positions genuinely differ, as in `a${ b }`.

**`R_BOGUS_SELECTOR` has exactly one producer**, and it is not the one §7
predicted. The RHS never builds one, for the reason above. The LHS of `::` does:
there the tokens are already consumed, so containment costs nothing and a bogus
`left` is strictly better than a bogus everything.

#### Corpus

**Enabled:** `expressions/extract.md` and `expressions/namespace.md` in full — 18
headings between them, which were 5 `[error, deviation]`, 6 `[error]`, and 7 plain
`[pending]`. Air's `undefined/extract_expression_error.R` and
`undefined/namespace_expression_error.R` live in these two files as well. The two
newline tests above are aether-authored, so they are absent from `MANIFEST.toml`
and do not move the 89 count. `Precedence Tests` in `expressions/precedence.md`
also shed its `[pending]`; `A$"B"^NA` and `a::b$c` were the only things it was
waiting on.

**Four marker changes, two of them the point of the recovery decision.**

*The five `[deviation]` markers came off.* `Dollar No RHS`, `Slot No RHS`,
`Extract With A Literal RHS`, `Namespace With Missing RHS`, and `Namespace With A
Literal RHS` all carried `[error, deviation]` because an earlier draft's bogus RHS
changed the shape. It no longer does. `Extract With A Literal RHS` even carries
air's own output inline, and it is what aether produces: `RExtractExpression` with
`right: missing (required)`, then `RDoubleValue` as a sibling.

*`Dollar, At, Namespace, Namespace Internal With Expression RHS` gained
`[error]`.* It was plain `[pending]` because `foo${bar}` is clean under
tree-sitter's `optional()` RHS. Deviation 2 makes the RHS required, so aether
raises `expected_selector` and the test says so. Its shape still matches, so it
needs no `deviation`.

*`Namespace get` and `Namespace get internal` gained `[error]` too*, which this
section did not anticipate. Both open with a bare `foo::` or `foo:::` on a line of
its own — the newline row above, and an error in aether where tree-sitter's
`optional()` RHS keeps it quiet. Same shape either way, so again no `deviation`.

#### The diagnostics this phase adds

`$`, `@`, `::`, and `:::` all funnel through `expected_selector`, so it names the
operator that was actually written — a message listing all four tells the reader
nothing about which one they typed. The secondary label points at the operator,
which is what capturing `cur_range()` before the bump exists for.

```rust
/// What `AnyRSelector` accepts
const SELECTORS: &[&str] = &["identifier", "string", "`...`", "`..i`"];

/// The name on the right of `$`, `@`, `::`, or `:::` is missing
pub(crate) fn expected_selector(
    p: &RParser,
    operator: RSyntaxKind,
    operator_range: TextRange,
    range: TextRange,
) -> ParseDiagnostic {
    expected_any(SELECTORS, range, p).with_detail(
        operator_range,
        format!("`{}` requires a name on the right.", text_of(operator)),
    )
}
```

`expected_selector_on_the_left` is its mirror, for the operand
`parse_namespace_left` rejects, and shares `SELECTORS` with it.
`selector_after_line_break` is the third, for the newline row. `text_of` is
`operator.to_string().unwrap_or("operator")` — punctuation kinds render as their
own text, so all three stay correct if the token set changes.

`format!` rather than `markup!`: `with_detail` takes a `biome_console::fmt::Display`,
which `String` already satisfies, and `biome_parser` builds its own details the
same way. It keeps `biome_console` out of the crate's dependency list.

These are the constructors `docs/deviations.md` entries 2 and 3 lean on — every
row where air produces no error and aether produces one is a row where one of them
fires.

### 11.5 Phase 11 — Function definitions

`src/syntax/function.rs`.

```rust
fn parse_function_definition(p: &mut RParser) -> ParsedSyntax {
    debug_assert!(p.at(T![function]) || p.at(T!["\\"]));

    let m = p.start();
    p.bump_any();                       // `function` or `\`
    // Newlines between `function` and `(` are insignificant. tree-sitter-r has
    // `repeat($._newline)` here and a corpus test with `function\n\n() 1 + 1`
    // and even `function\n\n# comment\n\n() 1 + 1`.
    parse_parameters(p).or_add_diagnostic(p, diagnostics::expected_parameters);
    // Newlines between `)` and the body are also insignificant.
    parse_expression(p).or_add_diagnostic(p, diagnostics::expected_expression);
    Present(m.complete(p, R_FUNCTION_DEFINITION))
}
```

**The newline subtlety.** A statement break would normally stop us. Here it must
not: after `function`, after `)`, and after the parameter list, newlines carry no
meaning. The clean way is to *not test* for one in these positions at all — the
only tests live in the Pratt loop and in `parse_else_clause`. Since
`parse_function_definition` calls `parse_expression` unconditionally for the body,
newlines are naturally skipped. Verify against the `Functions` corpus test, which
contains all four spacing variants.

Parameters use `Scope::Parenthesis` (newlines eaten) and a comma-separated list where each
`RParameter` is `name` + optional `RParameterDefault`. Names may be `IDENT`,
`DOTS`, or `DOTDOTI` — note `return` is a plain identifier in R and is a valid
parameter name (`Functions With return As Parameter Name`).

**Enable:** all 8 imported tests across `corpus/functions/` — `definition.md`,
`lambda.md`, `parameters.md` — two of them `[error]`. Air's merged
`ok/function_definition.R` comes with them only if it stays within function
syntax; check it rather than assuming (§10.3.4).

#### The diagnostics this phase adds

Two, and the pair is easy to conflate. One is the whole `(…)` group going
missing; the other is a hole inside a group that is present.

```rust
/// The whole `(…)` group of a function definition is missing, as in `function 1`
pub(crate) fn expected_parameters(p: &RParser, range: TextRange) -> ParseDiagnostic {
    expected_node("parameter list", range, p)
}

/// A hole in a parameter list, as in `function(a,,b)`
pub(crate) fn expected_parameter(p: &RParser, range: TextRange) -> ParseDiagnostic {
    expected_node("parameter", range, p)
}
```

`expected_parameter` is the asymmetry with §11.3 worth remembering: R accepts
`f(a,,b)` and rejects `function(a,,b)`, so the identical shape is a hole in an
argument list and an error in a parameter list. That is why the two lists do not
share a constructor even though they share a separator.

### 11.6 Phase 12 — Control flow

`src/syntax/control.rs`. `for`, `while`, `repeat` are mechanical. `if`/`else` is
not.

```rust
fn parse_if_statement(p: &mut RParser) -> ParsedSyntax {
    debug_assert!(p.at(T![if]));

    let m = p.start();
    p.bump(T![if]);
    p.with_scope(Scope::Parenthesis, |p| {
        p.expect(T!['(']);
        parse_expression(p).or_add_diagnostic(p, diagnostics::expected_condition);
    });
    p.expect(T![')']);
    parse_expression(p).or_add_diagnostic(p, diagnostics::expected_expression);
    parse_else_clause(p).ok();
    Present(m.complete(p, R_IF_STATEMENT))
}
```

#### The `else` rule — read this twice

In R, `else` may follow the consequence on a **later line only inside `{}`**. At
top level, a newline before `else` terminates the `if` and the `else` is a syntax
error. tree-sitter-r handles this with a dedicated external token
(`_external_else`) and `scan_else_with_leading_newlines`, with an explicit comment
in the grammar: *"No `repeat($._newline)` here. Specially handled in the scanner
instead."*

```r
# legal
{
  if (cond) 1
  else 2
}

# NOT legal (two statements, second is a syntax error)
if (cond) 1
else 2
```

There is a **third** case, and it is the one that decides the shape of the check:

```r
# legal — inside `(`, newlines are eaten entirely, braces or no braces
f(if (a) 1
  else 2)
```

So the rule is not "inside braces". It is "a newline before `else` is fatal only
where newlines are significant *and* we are not in braces" — and since the scopes
where newlines are significant are exactly `TopLevel` and `Brace` (§9.2), removing
`Brace` leaves one scope. The whole rule is `Scope::TopLevel`:

```rust
fn parse_else_clause(p: &mut RParser) -> ParsedSyntax {
    if !p.at(T![else]) { return Absent }

    // A newline before `else` is fatal at top level and nowhere else: `{}` allows
    // it outright, and `(`/`[`/`[[` never see a line break in the first place.
    if p.scope() == Scope::TopLevel && p.has_preceding_line_break() {
        return Absent;   // caller finishes the `if`; `else` becomes a bogus statement
    }

    let m = p.start();
    p.bump(T![else]);
    parse_expression(p).or_add_diagnostic(p, diagnostics::expected_expression);
    Present(m.complete(p, R_ELSE_CLAUSE))
}
```

This is the one site that compares against a `Scope` variant by name rather than
asking `newlines_are_significant()`, and it is why `scope()` is a plain read
(§9.2). Note what it does *not* need: nothing has to be reset on the way into
`(`/`[`/`[[` or restored on the way out, because there is a single field and
`with_scope` saves it.

Add all three cases to `control/if.md` as aether-authored tests. tree-sitter-r's
corpus covers the braced and top-level forms; the parenthesised one is the case a
newly-collapsed flag could plausibly regress.

**The comment case.** tree-sitter's scanner has a special carve-out: a comment
between the consequence and the `else` must not break the `else` attachment.

```r
{
  if (cond) {
  }
  # comment
  else {
  }
}
```

Because our `else` detection is purely `p.at(T![else])` on the next *non-trivia*
token, comments are handled for free. This is a case where the hand-written parser
is simply cleaner than the scanner. Add it as an explicit test in
`control/if.md` as an aether-authored test (a *matching* case, not a deviation) so
we notice if it regresses.

**The `else_idx` case** (tree-sitter-r #200): `elseμ <- 1` must lex as one
identifier. Handled in the lexer by lexing the full identifier and *then* checking
for keyword-hood, so it needs no parser support. Two corpus tests cover it.

**Enable:** all 15 imported tests across `corpus/control/` — `if.md`, `for.md`,
`while.md`, `repeat.md`. Air's four merged `ok/*_statement.R` snapshots come with
them where they stay within control-flow syntax (§10.3.4). Note the two `#200` `else`-prefix identifier tests live in
`if.md` rather than `literals/identifiers.md`; see §10.3.2.

#### The diagnostic this phase adds

```rust
/// The condition of an `if` or `while` is missing
pub(crate) fn expected_condition(p: &RParser, range: TextRange) -> ParseDiagnostic {
    expected_node("condition", range, p)
}
```

Only the two headers that have a condition use it. `for` names its own parts, and
`repeat` has no header at all, so neither reaches this.

### 11.7 Phase 13 — Blocks, statement lists, semicolons

`src/syntax/statement_list.rs`. This lands the semicolon deviation.

**What landed in Phase 7 is simpler than the body below.** Its recovery branch
consumes exactly one token into an `R_BOGUS_EXPRESSION` instead of running
`ParseRecoveryTokenSet`, so a run of junk becomes one bogus statement per token
rather than one per run. That is the shape Biome deprecated:
`SingleTokenParseRecovery` lives in `biome_js_parser` rather than the shared
crate, is marked `#[deprecated]`, and survives at a single call site. Replacing
it with the version below — recovery set, `enable_recovery_on_line_break()`, and
all — is part of this phase, not a refactor to schedule separately. Read the body
below as a diff against the landed `parse_statement_list`, not as a fresh
function. Its marker names and its `expression.range(p)` as the separator
diagnostic's first range are already right and stay as they are; what does change
is the landed `match` on `parse_expression`, which becomes an `Option`, because
the `;` has to be eaten after either branch rather than inside one.

```rust
fn parse_braced_statements(p: &mut RParser) -> ParsedSyntax {
    debug_assert!(p.at(T!['{']));

    let m = p.start();
    // The one scope that makes newlines significant *again* from inside a
    // paren: `f({\n  a\n  b\n})` is three statements, not one.
    p.with_scope(Scope::Brace, |p| {
        p.bump(T!['{']);
        parse_statement_list(p, T!['}'])
    });
    p.expect(T!['}']);
    Present(m.complete(p, R_BRACED_STATEMENTS))
}

/// Tokens a failed statement may resynchronise on: the statement separator, the
/// `}` of a braced block, and EOF as the universal backstop. Listed whether or
/// not this particular list is the one a `}` closes, for the reason §11.3 gives:
/// a closer that belongs to someone else must survive to reach them.
const STATEMENT_LIST_RECOVERY_SET: TokenSet<RSyntaxKind> = token_set![T![;], T![EOF], T!['}']];

fn parse_statement_list(p: &mut RParser, close: RSyntaxKind) {
    let statement_list = p.start();
    let mut progress = ParserProgress::default();

    while !p.at(T![EOF]) && !p.at(close) {
        progress.assert_progressing(p);

        let statement = p.start();
        let expression = parse_expression(p).ok();

        if expression.is_none() && !p.at(T![;]) {
            // Nothing here starts an expression. Recover INSIDE the statement,
            // so the bogus node fills the `expression` slot rather than becoming
            // a stray child of `R_STATEMENT_LIST` — that list is `RStatement*`
            // and `slot_to_node` panics on anything else (§7).
            //
            // `R_BOGUS_EXPRESSION` is the right kind because `AnyRExpression`
            // already carries an `RBogusExpression` variant, so it casts into the
            // `expression` slot unchanged. Note air's `R_EXPRESSION_LIST` accepts
            // bogus expressions as DIRECT children (its factory arm is
            // `make_node_list_syntax(kind, children, AnyRExpression::can_cast)`);
            // aether's `R_STATEMENT_LIST` will not, which is exactly why the
            // marker has to wrap the recovery.
            let range = p.cur_range();
            let recovered =
                ParseRecoveryTokenSet::new(R_BOGUS_EXPRESSION, STATEMENT_LIST_RECOVERY_SET)
                .enable_recovery_on_line_break()
                .recover(p);

            if recovered.is_err() {
                // Already at a recovery token with nothing consumed: abandon the
                // empty statement rather than completing a childless
                // `R_STATEMENT`, which `r.ungram` says is never produced.
                statement.abandon(p);
                break;
            }
            p.error(diagnostics::expected_expression(p, range));
        }

        // R separates statements by a newline or a `;` and accepts nothing else,
        // so `1 2` is an error even though both halves parse on their own.
        // Without this the list silently accepts it: `parse_expression` returns
        // `1`, `eat(;)` is a no-op, the statement completes, and `2` becomes a
        // second statement with no diagnostic.
        //
        // Emit and continue rather than recover: `2` becoming its own statement
        // is the most useful reading, and progress is already guaranteed because
        // `parse_expression` consumed.
        //
        // Guarded on the expression being present so the recovery branch above —
        // which has already emitted `expected_expression` — does not also
        // complain about a separator it was never going to reach.
        //
        // A bare `has_preceding_line_break()` suffices where the Pratt loop needs
        // `newlines_are_significant()` too: the only scopes a statement list runs
        // in are `TopLevel` and `Brace`, and in both of them it is already true.
        let ate_semicolon = p.eat(T![;]);
        if let Some(expression) = expression
            && !ate_semicolon
            && !p.at(T![EOF])
            && !p.at(close)
            && !p.has_preceding_line_break()
        {
            p.error(diagnostics::expected_statement_separator(
                expression.range(p), p.cur_range(),
            ));
        }

        statement.complete(p, R_STATEMENT);
    }

    statement_list.complete(p, R_STATEMENT_LIST);
}
```

Cases to test explicitly (air already has snapshot files for all of these under
`tests/snapshots/ok/semicolons/`):

| Source | `R_STATEMENT_LIST` contents (`stmt` = `RStatement`) |
|---|---|
| `1;2` | stmt(`1`, `;`), stmt(`2`, —) |
| `1 ; 2` | same, `;` gets surrounding whitespace as trivia |
| `1;;2` | stmt(`1`, `;`), stmt(—, `;`), stmt(`2`, —) |
| `;1` | stmt(—, `;`), stmt(`1`, —) |
| `1;` | stmt(`1`, `;`) |
| `1;# comment` | stmt(`1`, `;`), comment is trailing trivia of `;` |
| `;# comment` | stmt(—, `;`) — the comment has no expression to attach to |
| `{\n  ;# c\n}` | same, inside braces |

The last three matter more for the formatter than for the parser, and the last
two are aether-authored (air has no snapshot for a comment on a bare `;`). With
the semicolon removed and replaced by a hard line, a comment with no expression
beside it becomes **dangling on the `RStatement`**, which is a case air's comment
rules do not currently have — see §12.3 step 4. `1;;2` is in the table for the
same reason: the middle statement must not print a blank line.

All three are `[deviation]`, so they do not move the 89 count.

**Enable:** all of `corpus/statements/` — `braces.md`, `semicolons.md`, and the
two headings in `newlines.md` that have been waiting on braces since Phase 8.
This is the one directory that really does finish here: braces and statement
lists are the last constructs any merged air snapshot in it can depend on. Every
test in `semicolons.md` comes from air's snapshots rather than tree-sitter and
carries `[deviation]`.

#### The diagnostics this phase adds

None. `expected_statement_separator` landed in Phase 7 with
`parse_statement_list` (§11.1); a `;` is a second way to satisfy a rule it already
enforces, not a new rule. What this phase owes it is the wording check — the
message names both separators, so it must not start claiming a `;` is required
where a newline would do.

The argument-list analogue (`f(a b)`) stays a different rule, enforced by
`expect(T![,])` in §11.3 — the same shape of mistake, a different separator, and
different wording. Keep them apart: a reader who sees "expected a newline or a
`;`" inside a call has been told something false.

---

## 12. Phase 14 — Recovery polish and air integration

### 12.1 Recovery review pass

By this point every feature is in, and recovery has been added ad hoc. Do a
dedicated pass:

- Every `parse_*` that can fail has a recovery set that includes the enclosing
  construct's closing token, plus `T![;]`, plus `enable_recovery_on_line_break()`
  wherever a newline genuinely bounds the construct.

- **No list check is allowed to be load-bearing beyond what it returns.** Biome's
  `ParseSeparatedList::parse_list` checks its terminator in exactly one place,
  which is why none of its thirty-odd implementors can leak one;
  `parse_argument_list` (§11.3) once checked in three places, and while that was
  merely untidy on its own, it cost a bug when its terminator check also re-lexed
  the `]]`. The recovery branch grew an exit that never called it, so `x[[a b]]`
  lost its terminator and stranded both brackets in the enclosing statement list.
  Both halves are fixed: the join moved to `RParser::do_bump()`, and
  the loop was rewritten around `ARGUMENT_LIST_ENDS` so that it has one `break`
  and no terminator of its own to leak. The general rule still wants the audit: a
  predicate that a loop calls at some exits and not others must not be the only
  thing doing a job. Check every `break` out of a list loop on that basis.

- Every bogus node is overlapped by a diagnostic. `check()` step 4 has enforced
  this since Phase 6 (§10.2), so the Phase 14 job is to review the *wording* and
  the *placement* of those diagnostics, not to add the check — a diagnostic that
  satisfies the overlap test can still point at a range that reads badly. Air's
  `has_bogus_nodes_or_empty_slots` exists for the weaker version of this and its
  comment says so: *"used in the parser test to ensure it doesn't emit bogus nodes
  without diagnostics."* Placement is worth a second look where the diagnostic
  is raised *before* recovery: `parse_argument_list` reports `expected ','` at
  `p.cur_range()`, so `f(a b c)` points at `b` while the bogus node swallows
  `b c`. Biome's `or_recover_with_token_set` — 66 uses against 10 bare
  `recover()` calls — reports at `recovered.range(p)` instead, covering the whole
  buried run. Decide which reads better and apply it uniformly.

- The parser never loops forever. Add a `ParserProgress`-style guard (Biome has
  one) to every unbounded loop, not just the statement list. nextest's
  `slow-timeout = { period = "10s", terminate-after = 3 }` (§5.1) is the backstop:
  a runaway loop shows up as a terminated test rather than a hung CI job, but the
  guard is what makes it a *useful* failure with a stack trace.

- The parser never panics on any input. Since fuzzing is out of scope for now, at
  minimum run the parser over every `.R` file you can find in air's repo
  (`crates/air_r_formatter/tests/**`) as a smoke test, asserting no panic and
  round-trip fidelity. This is cheap and catches a surprising amount.

### 12.2 Performance

Only measure once correctness is done. A hand-written parser should be
comfortably faster than tree-sitter + tree walk + event replay, but confirm rather
than assume. Add a Criterion benchmark over a few large real R files and record
the number in the PR. If it is *slower*, the likeliest culprit is `Vec<Trivia>`
reallocation — pre-size it. That is the only allocation on the hot path:
`next_non_trivia_token` buffers nothing per gap (§9.1), so trivia costs one push
per piece into a vector that grows once. The parser itself is single-pass:
§11.3's argument names are parsed optimistically and reshaped with `precede`, so
no construct backtracks and no token is lexed twice.

### 12.3 The swap into air

A single PR against `posit-dev/air`. Aether is now the source of truth for the
parser, the syntax layer, the factory, and `r.ungram`; air deletes all four and
becomes a consumer.

**Restore the parse span in aether first, before the sha is pinned.** Phase 5
dropped `tracing` from `aether_r_parser`: nothing in aether's workspace installs
a subscriber, so the `debug_span!("parse")` that air ships produced no output
here and cost four crates for it. Air installs one, so the span has to go back
around `parse_with_cache` or `air format --log-level debug` and the LSP silently
lose the only span the parser emits. Nothing in air's build fails if this is
forgotten, which is the whole reason it is written down.

Its placement moves with it. Air wrapped `build_tree` alone, so its `"parse"`
span excluded the tree-sitter parse it was named for; around `parse_with_cache`
it covers lexing, parsing, and tree building. Expect the number it reports to be
larger than air's was, and say so in the PR — that is the span finally measuring
what it claims, not a regression.

1. **Delete** `crates/air_r_parser/`, `crates/air_r_syntax/`,
   `crates/air_r_factory/`, and `xtask/codegen/r.ungram` plus the grammar codegen
   that produced them. Air keeps its *formatter* codegen, which still needs
   `formatter_crate_name`.
2. **Depend on aether, using Cargo's package rename** so no `use` statement in air
   changes:
   ```toml
   [workspace.dependencies]
   air_r_parser  = { package = "aether_r_parser",  git = "https://github.com/posit-dev/aether", rev = "<sha>" }
   air_r_syntax  = { package = "aether_r_syntax",  git = "https://github.com/posit-dev/aether", rev = "<sha>" }
   air_r_factory = { package = "aether_r_factory", git = "https://github.com/posit-dev/aether", rev = "<sha>" }
   ```
   Remove `tree-sitter` and `tree-sitter-r` from the workspace entirely.
   `crates/biome_ungrammar/` also leaves air, since only the grammar codegen used it.
3. **Verify the rename is invisible, then fix the one API that deliberately is
   not.** The Cargo rename accounts for every `use` statement in air, so
   `cargo build --workspace` should reduce to a known list of errors. Anything
   *outside* that list means air was reaching into a crate-private path, and is
   the thing this step exists to catch.

   The known list, all of it `Parse`'s error API. `ParseError` was a
   tree-sitter-era stand-in from when the parser could report one whole-file
   error and nothing better; aether returns real `ParseDiagnostic`s, so the
   struct and the methods that collapsed to it are gone:

   - `into_error()` and `error()` no longer exist. One caller,
     `workspace/src/format.rs:46`, which is the interesting one — rebuild
     `FormatSourceError` from `into_diagnostics()` rather than from a single
     collapsed string. That is step 5's win arriving one step early, and the
     LSP inherits it.

   - `has_error()` is now `has_errors()`, matching `AnyParse` and every other
     Biome parser. Six call sites, all mechanical: `workspace/src/format.rs`
     (two), `lsp/src/handlers_format.rs` (two), `lsp/src/handlers_ext.rs`
     (two). Do not touch `handlers_ext.rs:37` — that is tree-sitter's
     `Node::has_error` and it leaves with tree-sitter.

   - `air_r_formatter/tests/quick_test.rs:37` also calls `has_error()`.

   - `Parse` is now `RParse`. Three sites name the type rather than going
     through inference: `workspace/src/format.rs:2` (the `use`),
     `workspace/src/format.rs:66` (a `&Parse` parameter), and
     `lsp/src/documents.rs:27` (the `Document` field). The Cargo package
     rename cannot cover this one — it renames crates, not items — so either
     rewrite the three, or add `use aether_r_parser::RParse as Parse;` if the
     churn is unwelcome mid-swap. Prefer rewriting; the alias reintroduces the
     name the rename exists to remove.

   `into_result()` had no caller in air and needs nothing.

4. **Update `air_r_formatter`** for the `R_STATEMENT` wrapper, real semicolon
   tokens, and one-token strings. Expect churn in
   `r/expressions/expression_list.rs` and `r/expressions/braced_expressions.rs`
   (renamed to `braced_statements.rs`) and their snapshots.

   The string half is two files and no snapshot churn, since the formatter is
   R-in / R-out: `r/auxiliary/string_value.rs`, where three writes become one,
   and `string_content.rs`, which normalizes the whole token rather than the
   content token. Deviation 12 has the rest of the accounting.

   **Semantics, settled with the owner:** `format_removed(&semicolon)` plus a
   hard line break, so `1;2` formats as two lines. Three things follow, and the
   first two are new rules rather than mechanical edits:

   - **A comment with no expression beside it becomes dangling.** For `1; # c`
     the comment has a `preceding_node` on the same line and Biome's default
     placement should make it trailing of the expression, where it prints
     correctly. But for a bare `;# c` — or a leading `;`, or `{ ;# c }` — the
     statement has no expression, so both `preceding_node` and `following_node`
     are `None` and the comment attaches as **dangling on the `RStatement`**.
     `FormatRStatement` must then call `format_dangling_comments(node)`
     explicitly, or Biome's "every comment is printed exactly once" assertion
     fires in debug and the comment is silently dropped in release.
   - **An empty statement must not emit a line.** `1;;2` is
     `stmt(1, ;)`, `stmt(—, ;)`, `stmt(2, —)`; joining with hard lines
     unconditionally yields `1\n\n2`. The join must skip statements that produce
     no content, and the predicate is "no expression **and** no dangling
     comment", which is what ties it to the rule above. `{ ; }` and a leading
     `;` are the same shape.
   - **Verify first: does `format_removed` print the token's comments itself, or
     has the `Comments` map already claimed them during attachment?** Both firing
     double-prints the comment; neither firing drops it. Air's existing
     `format_removed` call sites should settle which, and the answer decides
     whether the dangling rule above is sufficient on its own.

   The three corpus tests for these live in `statements/semicolons.md` (§11.7).
5. **Update the LSP** to surface per-diagnostic ranges instead of one whole-file
   error. This is the most user-visible win of the whole project — call it out in
   the changelog.
6. **Delete the oracle** (§10.4.1). Once air is on aether, tree-sitter-r is not
   the reference for anything: remove `crates/aether_r_oracle/`, its call in
   `check()` along with `CheckError::OracleMismatch`, the `air_r_parser` /
   `air_r_syntax` workspace deps, and the `tree-sitter` / `tree-sitter-r` ones
   from **aether** as well as air.
   The `[deviation]` marker stays — it still documents which tests intentionally
   differ, and `docs/deviations.md` still cross-references it — but it becomes
   inert in the harness. Say so in its row in §10.1's marker table when you do.
7. **Final acceptance gate:** air's entire formatter snapshot suite is unchanged
   except for semicolon-related files.

   **This is also where deviation 10 gets settled.** Its audit says no
   consumer in air reads whitespace attachment polarity, so the suite should not
   move on its account — but that is an argument, and this is the experiment.
   Run the suite before touching the formatter, so a whitespace-polarity
   regression cannot hide inside the semicolon churn. Any file that moves and is
   not semicolon-related is deviation 10 failing its premise, and the fix is to
   restore air's rule in `RTokenSource` — a self-contained change to one
   function, which is the other reason the audit was worth doing up front.

**Deferred follow-up, separate PR.** The Cargo rename is settled for the swap
itself ("fine for now"), which leaves open whether air's imports should eventually
say `aether_r_syntax` outright. If the owner wants that, it is a mechanical
find-and-replace across air landed *after* the swap. Nothing forces the question
— the Cargo mapping is a supported, permanent arrangement, not a temporary shim,
so this can stay undone indefinitely without accruing debt.

One consequence to keep in mind while it stays undone: a reader of air sees
`use air_r_syntax::…` and will not find that crate in air's tree or on crates.io.
Worth one line in air's `CLAUDE.md` or the workspace `Cargo.toml` pointing at
aether, so the indirection is discoverable from the place someone hits it.

---

## 13. Risk register

| Risk | Why it bites | Mitigation |
|---|---|---|
| **Trivia attachment drift** | A one-piece difference in *comment* attachment silently changes formatter output; a whitespace one changes every snapshot. | Largely retired by deviation 10: the rule is a latching bool with no lookahead, so there is little left to get subtly wrong. Comment attachment is unchanged from air and the oracle still compares it, since normalization touches whitespace only (§10.4). Landed in Phase 5, ahead of any grammar work; what exposure remains is settled by air's formatter snapshots at swap-in (§12.3 step 7). |
| **`[[` / `]]` joining** | Genuinely ambiguous; tree-sitter needs an external scanner and still has open bugs. The lexer half landed in Phase 4; the parser half landed in Phase 9. | One join site, in `RParser::do_bump()`, reading the scope (§11.3). The corpus works it from both ends: the nesting ladder in `expressions/subset.md` on valid code, the mismatched-bracket set in `recovery/unmatched-delimiters.md` on invalid. Do not put a scope stack in the lexer — §8.4 says why it cannot work here — and do not make the lexer glue closers. |
| **Codegen drift** | If aether's generated code diverges from air's, the swap-in fails in confusing ways. | Retired: Phase 2 asserted byte-identity, and CI asserts `just codegen` idempotence on every PR. |
| **Oracle unavailable on error tests** | tree-sitter bails on any error, and aether's own error blocks are skipped too (§10.4), so the error corpus gets no oracle coverage at all. | Accept it; those are exactly the tests where we *intend* to differ. Rely on reviewed snapshots there. |
| **Oracle breaks** | It depends on air at a pinned rev, so a rev bump on either side can break the build or the comparison. | It runs with every test, so a break is loud rather than silent. Fix cheaply or abandon it — it is deleted at swap-in regardless (§10.4.1), and reviewed snapshots, not the oracle, are what carry forward. |
| **`[pending]` rot** | A large batch of inert tests invites ignoring them. | Unexpected pass is a *hard failure*, so the count only moves deliberately. Report `N/89 enabled` in CI output. |
| **Biome fork rev skew** | Air's fork could move mid-project. | Pin, and treat a rev bump as its own PR with a full snapshot regeneration. |

---

## 14. `CLAUDE.md` for the aether repo

`CLAUDE.md` exists at the repo root and is the context every session starts from.
It carries the commands, the four invariants, the corpus conventions, and the
reference implementations. It is short on purpose — do not grow it into a copy of
this plan.

Two things keep it honest as phases land:

- **It describes the repo as it will be**, and says so, so a command or directory
  belonging to an unlanded phase is not a bug in the file.

- **The invariants are the part that must not drift.** Round-trip for all inputs,
  a diagnostic for every bogus node, no panics and no infinite loops, and tree
  shape matching tree-sitter-r except where `docs/deviations.md` says otherwise.
  §10.2's `check()` enforces the first three mechanically.
