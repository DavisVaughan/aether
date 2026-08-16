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
    │       ├── syntax.rs          # ← Phase 7 onward
    │       └── syntax/
    │           ├── expr.rs        # Pratt loop, primary exprs
    │           ├── call.rs        # call / subset / subset2 / arg lists
    │           ├── function.rs    # function defs, lambdas, parameters
    │           ├── control.rs     # if/else, for, while, repeat
    │           ├── block.rs       # braced exprs, statement lists, semicolons
    │           └── value.rs       # literals, strings, keywords-as-values
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

Phases 1–6 have landed. Their sections below are compressed to the decisions
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
| 7 | Literals, identifiers, program | `1`, `"a"`, `foo`, `TRUE`, `NA_integer_`, `..1`, `...` | |
| 8 | Binary & unary operators | The Pratt core, precedence table, newline sensitivity | |
| 9 | Calls, subset, subset2 | `f(a, b=1)`, `x[i]`, `x[[i]]`, hole arguments | |
| 10 | Extract & namespace | `$`, `@`, `::`, `:::` and the deviation from tree-sitter | |
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
catch it. Phase 10 (§11.4) is what finally uses the kind.

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

**Lookahead lexes in `RLexContext::default()`**, so it is only meaningful for
tokens whose lexing is context-free — it cannot peek past a `STRING_OPEN`, whose
body would come back lexed as code. That constraint is why §11.3 parses argument
names optimistically rather than looking ahead for the `=`.

The kinds such a lookahead returns are useless, but they are only useless: it
leaves nothing behind for the real pass to trip over, because the lexer keeps no
state a rewind cannot restore. §8.2.1 is where that is arranged, and it is load
bearing — the lookahead pass and the rewind that undoes it are both
`BufferedLexer`'s business, not the parser's, so no parser-side discipline can
substitute for it.

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

#### 8.2.1 The lexer keeps no string state

**This is an invariant, not an implementation detail, and §11.3 depends on it.**
A string is three tokens, so the lexer could remember what it opened:

```rust
// NOT what aether does
string_delimiter: Option<RStringDelimiter>,  // set at STRING_OPEN, cleared at STRING_CLOSE
```

That would buy dataless context variants and spare the parser from naming a
delimiter. It is also wrong, because `BufferedLexer` rewinds the inner lexer on
its own initiative in two places: `next_token` calls `reset_lookahead()` whenever
`!context.is_regular()`, so every string body request can rewind, and `re_lex`
rewinds before and after re-lexing.

Meanwhile `LookaheadIterator::next()` lexes ahead with `LexContext::default()`,
so a lookahead pass runs `lex_token()` over string bodies and would set such a
field as a side effect. `LexerCheckpoint` has no slot for it, so the rewind
restores the position and leaves the field describing whichever string the
lookahead happened to stop inside:

```r
"x" r"(y)"
```

Lex `STRING_OPEN`, checkpoint, lex four tokens in the regular context, rewind.
The position comes back to 1; a `string_delimiter` field comes back holding
`Raw { quote: b'"', hyphens: 0, bracket: Paren }`. Lexing the body of `"x"`
against that delimiter scans for `)"` and swallows the rest of the line. Round
trip still holds — no bytes are lost — so the corpus would not catch it; only the
token boundaries are wrong.

**So the delimiter travels in the context**, and the caller reads it back off the
`STRING_OPEN` token's text with `RStringDelimiter::from_open()`. The text is the
only channel available in any case: `BufferedLexer`'s `inner` is private with no
accessor and no `Deref`, and `TokenSourceWithBufferedLexer::lexer()` hands back
the `BufferedLexer`, not the `RLexer`. Nothing above the lexer can read a lexer
field, so "ask the lexer for the delimiter" is not implementable at any layer,
however the lexer is written.

`from_open()` and the lexer's own dispatch share
`RStringDelimiter::try_from_raw_string_open()`, which reads no further than the
delimiter itself. That leaves exactly one parser of the hyphen-and-bracket
grammar, and the sharing is what makes `from_open()`'s `expect` safe — the lexer
emits a `STRING_OPEN` starting with `r` only where `try_from_raw_string_open()`
answered `Some` on the same bytes.

`open_length()` and `close_length()` keep the two lengths in one place.
`checkpoint.rs` pins the lookahead-then-rewind sequence above, that each open
form reads back as its delimiter, and that a name like `rnorm` answers `None`.

§9.1.1 is the parser's side of this, and it calls the same `from_open()` the test
driver does, so the two cannot drift.

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
`p.re_lex(RReLexContext::JoinBrackets)` where the nesting says to. There is no
scope stack in the lexer. This is the single most important design call in the
lexer and the one most likely to be undone by accident.

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

The concrete payoff: a token that appears unbidden has to be spelled out in every
terminator check and every recovery set anywhere in the parser that mentions `]`,
and missing one lets recovery swallow two closers, stripping an enclosing
construct of its terminator. A `]]` that only ever exists because a subset2 asked
for it cannot be swallowed by anything. §11.3 has the single join site and the
`x[[a[b]]]` worked example.

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
is no `Backtick` variant in `RStringDelimiter`, and an unterminated backtick is a
lexer diagnostic (`unterminated_backtick_identifier`), where the three
unclosed-quote cases surface through the parser's `expect(STRING_CLOSE)` in
§11.1.

Backticked names may span newlines, and so may strings — R's `token()` dispatches
both to the same `StringValue()` function, which turns a newline into an escaped
newline rather than an error. An unterminated one genuinely runs to EOF, which is
what tree-sitter does too. The way to keep the error useful is where the
diagnostic points, not where the scan stops: at the opening delimiter, with EOF
as a secondary label. `enable_recovery_on_line_break()` will not help the parser
resynchronise after one, and it should not be expected to.

**Strings are three tokens**: `STRING_OPEN`, `STRING_CONTENT`, `STRING_CLOSE`.
Content is one token spanning everything between the delimiters, escapes
included — neither air's walker nor `r.ungram` models `escape_sequence`
separately. `STRING_CONTENT` is **optional**: an empty body emits none, so a
`StringContent` request on a body that closes immediately comes back as
`STRING_CLOSE`, and `""` is two tokens where `"a"` is three. Both consumers of
the lexer decide the next context from the kind that came back, never from the
one they asked for. §11.1's `parse_string_value()` is where that matters.

**`ERROR_TOKEN`** is a character that starts no token at all: one character
consumed, `unexpected_character` raised, `ERROR_TOKEN` returned. Deviation 6 in
`docs/deviations.md` carries the argument for why it is not `IDENT`. The
reachable cases are narrow — every printable ASCII character starts some R token,
so this is only control characters and non-ASCII characters that are neither
`XID_Start` nor `XID_Continue`. §11.1 is the parser's side.

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
  Three sites do so: the Pratt loop's statement break and its `::` arm (§11.2),
  and `parse_else_clause()` (§11.6). Nothing else reads the raw flag.

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

`skip_as_trivia_with_context()` resumes as if at the start of the file, so trivia
after a skipped token leads the next token instead of trailing what was skipped.
Phase 14's recovery is what produces those pieces.

`BumpWithContext` is what carries `RLexContext` down for string bodies, and
`TokenSourceWithBufferedLexer` supplies `NthToken` lookahead for free.

#### 9.1.1 The string delimiter comes from the token text

The lexer keeps no memory of the string it opened (§8.2.1), so the delimiter is
named by the caller, which is `parse_string_value` (§11.1):

```rust
let delimiter = RStringDelimiter::from_open(p.cur_text());
```

**Neither `RTokenSource` nor `RParser` has a `string_delimiter()` method**, and
neither should grow one. `Parser::cur_text()` is provided, and its body is already
the whole of what such a method would do. The token source has no internal use for
a delimiter either: `bump_with_context` takes the context as an argument, so
context only ever flows parser → source, and a method there would exist solely to
be forwarded out again.

Behind `at(STRING_OPEN)` the call is total — the lexer emitted that token by
parsing the same bytes successfully — so it needs no guard beside it, and it is
stateless, so a checkpoint taken anywhere inside a string is as good as any other.
Read the delimiter once at the open and pass it to both bumps. Do not thread it
through `RParser` state, and do not look it up again at `STRING_CONTENT`, whose
text is the body rather than a delimiter.

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
| `expected_expression` | §11.2, Phase 8 | a binary or prefix operator with nothing to its right |
| `expected_argument` | §11.3, Phase 9 | `parse_argument_list` |
| `expected_selector` | §11.4, Phase 10 | the `$`, `@`, `::`, `:::` arm of the Pratt loop |
| `expected_parameters`, `expected_parameter` | §11.5, Phase 11 | `parse_function_definition` |
| `expected_condition` | §11.6, Phase 12 | the `if` and `while` headers |
| `expected_statement_separator` | §11.7, Phase 13 | `parse_statement_list` |

Two rules hold across all of them, and the split above does not weaken either:

- **Every diagnostic is a named function in `diagnostics.rs`, never an inline
  `err_builder` in a syntax module.** It keeps wording consistent and makes the
  whole diagnostic surface reviewable in one sitting.

- **Only the *specification* moves to the feature's section; the *definition*
  still goes in `diagnostics.rs`.** Splitting the file per phase would trade the
  one-file review for nothing.

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
`[pending]` marker. §11.1 deletes the placeholder and its diagnostic when
`parse_statement_list` lands.

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

### 11.1 Phase 7 — Literals, identifiers, program

`src/syntax/value.rs`, `src/syntax/expr.rs`, `src/syntax.rs`.

```rust
pub(crate) fn parse_root(p: &mut RParser) {
    let m = p.start();
    p.eat(UNICODE_BOM);
    // No scoped helper here: the defaults *are* top-level behaviour — newlines
    // are significant and `else` may not follow one. See §9.2 on polarity.
    parse_statement_list(p, StatementListEnd::Eof);
    p.expect(T![EOF]);
    m.complete(p, R_ROOT);
}
```

That `parse_statement_list` call replaces Phase 6's placeholder, and
`parse_placeholder_statement` with its inline diagnostic goes with it (§10).

**Phase 7 writes the first `parse_statement_list`, not Phase 13.** §11.7 shows the
finished version, but `parse_root` calls it from this phase onward, so a working
version has to exist now. What Phase 13 *adds* is the `;` slot and the
empty-statement handling; what Phase 7 must already get right is the loop, the
recovery branch, and — importantly — the **statement separator rule**, since R
separates statements by a newline or a `;` and by nothing else. Deferring that to
Phase 13 would let `1 2` parse clean for six phases, including Phase 8, where
`statements/newlines.md` is supposed to go green. Write it here against
§11.7's body with the semicolon lines omitted.

**What `parse_expression` is in this phase.** `parse_statement_list` calls it and
Phase 8 owns it, so what Phase 7 writes is the stub it grows from — the atom
dispatch, with nothing above it:

```rust
// crates/aether_r_parser/src/syntax/expr.rs
pub(crate) fn parse_expression(p: &mut RParser) -> ParsedSyntax {
    parse_primary_expression(p)
}
```

Phase 8 puts the Pratt loop behind that name (§11.2).
`parse_primary_expression` stays where it is and gains an arm per phase, which is
how each later phase's construct becomes reachable at all.

```rust
/// Anything that can start an expression and consists of exactly one token.
fn parse_literal_expression(p: &mut RParser) -> ParsedSyntax {
    let kind = match p.cur() {
        R_INTEGER_LITERAL => R_INTEGER_VALUE,
        R_DOUBLE_LITERAL  => R_DOUBLE_VALUE,
        R_COMPLEX_LITERAL => R_COMPLEX_VALUE,
        IDENT             => R_IDENTIFIER,
        DOTS              => R_DOTS,
        DOTDOTI           => R_DOT_DOT_I,
        T![next]          => R_NEXT_EXPRESSION,
        T![break]         => R_BREAK_EXPRESSION,
        T![true]          => R_TRUE_EXPRESSION,
        T![false]         => R_FALSE_EXPRESSION,
        T![null]          => R_NULL_EXPRESSION,
        T![inf]           => R_INF_EXPRESSION,
        T![nan]           => R_NAN_EXPRESSION,
        T![na_logical] | T![na_integer] | T![na_double]
        | T![na_complex] | T![na_character] => R_NA_EXPRESSION,
        STRING_OPEN       => return parse_string_value(p),
        _ => return Absent,
    };
    let m = p.start();
    p.bump_any();
    Present(m.complete(p, kind))
}

/// A character the lexer could not make a token of. Copy `biome_js_parser`'s
/// `parse_primary_expression` arm exactly: start a marker, bump the token, and
/// complete it as a bogus node.
///
/// No diagnostic here — the lexer already raised `unexpected_character` and
/// pointed at the character. A second one at the same range is noise, and
/// invariant 2 (bogus nodes always come with at least one diagnostic) is already
/// satisfied by the lexer's.
fn parse_error_token(p: &mut RParser) -> ParsedSyntax {
    if !p.at(ERROR_TOKEN) { return Absent }
    let m = p.start();
    p.bump_any();
    Present(m.complete(p, R_BOGUS_EXPRESSION))
}

/// `R_STRING_VALUE` is three tokens; the last two need a lex context.
fn parse_string_value(p: &mut RParser) -> ParsedSyntax {
    if !p.at(STRING_OPEN) { return Absent }

    let m = p.start();
    // Read once, off the open token's text. `cur_text()` is trait-provided and
    // the guard above makes `from_open` total, so this needs no helper (§9.1.1).
    let delimiter = RStringDelimiter::from_open(p.cur_text());
    p.bump_with_context(STRING_OPEN, RLexContext::StringContent(delimiter));
    if p.at(STRING_CONTENT) {
        p.bump_with_context(STRING_CONTENT, RLexContext::StringEnd(delimiter));
    }
    p.expect(STRING_CLOSE);                        // diagnostic if unterminated
    Present(m.complete(p, R_STRING_VALUE))
}
```

The empty-body case is why the `STRING_CONTENT` bump is conditional: `""` lexes as
`STRING_OPEN` then `STRING_CLOSE`, because the lexer answers a `StringContent`
request on an empty body with the close (§8.4). The same `delimiter` value serves
both bumps — do not re-read it at `STRING_CONTENT`, whose text is the body.

`parse_error_token` hangs off `parse_primary_expression`, alongside
`parse_literal_expression` — not off `parse_literal_expression` itself, which
answers "one token, one value node" and would be lying. Placing it in the atom
dispatch rather than leaving it to statement-list recovery is what keeps the
damage to one token: recovery would skip to the next statement boundary and
swallow whatever followed the stray character on the same line.

`ERROR_TOKEN` needs no entry in any recovery set. It starts no production, so
every `at(...)` test in the parser is already false for it, and the token can
only be reached by the arm above.

**Enable: the 19 imported tree-sitter tests across all five `corpus/literals/`
files.** That is **19 of 89 green** at the end of the very first parser phase,
which is a good confidence signal.

The *files* do not go fully green, because §10.3.4 merges air's snapshots into
them and several reach past literals: `ok/comments.R` is `1 + 1 # Trailing`
(needs Phase 8) and `ok/dots.R` contains `{ ... }` and `...()` (needs Phases 13
and 9). Those stay `[pending]` until their phase. `cargo nextest run -E
'test(/corpus::literals/)'` still passes — pending tests pass by asserting failure
— without every heading in those files being enabled.

Three of the four error tests (Unclosed Double Quote, Unclosed Single Quote,
Unclosed Raw String) exercise the parser's `expect(STRING_CLOSE)` and its recovery.
Unclosed Backtick takes a different path entirely and exercises the *lexer's*
`unterminated_backtick_identifier` (§8.4), since a backtick-quoted name is an
`IDENT`, not a string.

**An unterminated string gets no constructor of its own for now.** The three
unclosed-quote corpus tests surface through `expect(STRING_CLOSE)` above, which
is `ParseDiagnostic` enough to start. Add one only if this phase's snapshots read
badly — and if so, give it the two-range shape
`unterminated_backtick_identifier` already uses, pointing at the opening
delimiter with EOF as a secondary label, since a string runs to EOF for the same
reason a backticked name does (§8.4).

### 11.2 Phase 8 — Binary and unary operators (the Pratt core)

`src/syntax/expr.rs`. This is the heart of the parser. Take the time to get it
right; everything else hangs off it.

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

**`function`, `while`, `for`, `repeat`, and `if` get no rank.** R's grammar gives
them one, and it does one job there: make the body greedy, i.e. say how far right
`function(x) …` or `if (c) …` extends. A recursive-descent parser answers that by
construction — the body is a plain `parse_expression` call that runs to its
natural terminator (§11.5, §11.6).

**Rank 18 is absent from the binding-power functions below.** Postfix is handled
structurally in the Pratt loop and applies unconditionally, so it is never
compared against a `min_bp`. The number survives in code only as
`POSTFIX_BINDING_POWER`, which exists purely to give the `debug_assert!` in
`parse_expression_with_lhs` something to assert *against* — that 18 really is the
ceiling. Adding an infix operator at rank 18 or above would break that and is what
the assertion catches; the fix in that case is to renumber so postfix stays
tightest, not to relax the assertion.

The one case where a greedy body and a body parsed at `min_bp = 2` — everything
but `?` — would differ is the binary `?` at rank 1: `function() a ? b` could
group either way. R's grammar makes a function body `expr_or_assign_or_help`,
which includes help and so points at greedy, but do not take that on faith: let
the oracle settle it the first time it runs against `functions/definition.md`,
and record the answer here if it is not greedy.

**Associativity of `$` `@` `::` `:::`.** tree-sitter-r forces these **right**
associative and explains why in a long comment: it is a workaround for the
`optional()` RHS, so that `foo::bar` prefers the longer match `[foo::bar]` over
`[foo::][bar]`. Since aether makes the RHS **required** (deviation 2), that workaround is
unnecessary and we use **left** associativity, matching R's `gram.y`. The
resulting trees should be identical — the RHS is restricted to a selector, so
`a$b$c` can only nest one way — but say so in the PR and let the oracle confirm it.

```rust
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub(crate) struct BindingPower(u8);

#[derive(Debug, Clone, Copy)]
enum Assoc { Left, Right }

/// Rank 18 — `f(…)`, `x[…]`, `x[[…]]`. Appears in no lookup table because
/// postfix is handled structurally in the Pratt loop, but it is named so the
/// `debug_assert!` guarding "postfix is the tightest-binding construct" reads as
/// a claim about the precedence table rather than a bare `18`.
const POSTFIX_BINDING_POWER: BindingPower = BindingPower(18);

fn infix_binding_power(kind: RSyntaxKind) -> Option<(BindingPower, Assoc)> {
    use RSyntaxKind::*;
    Some(match kind {
        T![?]                                    => (BindingPower(1),  Assoc::Left),
        T![<-] | T![<<-] | T![:=]                => (BindingPower(2),  Assoc::Right),
        T![=]                                    => (BindingPower(3),  Assoc::Right),
        T![->] | T![->>]                         => (BindingPower(4),  Assoc::Left),
        T![~]                                    => (BindingPower(5),  Assoc::Left),
        T![|] | T![||]                           => (BindingPower(6),  Assoc::Left),
        T![&] | T![&&]                           => (BindingPower(7),  Assoc::Left),
        T![<] | T![<=] | T![>] | T![>=]
        | T![==] | T![!=]                        => (BindingPower(9),  Assoc::Left),
        T![+] | T![-]                            => (BindingPower(10), Assoc::Left),
        T![*] | T![/]                            => (BindingPower(11), Assoc::Left),
        SPECIAL | T![|>]                         => (BindingPower(12), Assoc::Left),
        T![:]                                    => (BindingPower(13), Assoc::Left),
        T![^] | T![**]                           => (BindingPower(15), Assoc::Right),
        T![$] | T![@]                            => (BindingPower(16), Assoc::Left),
        T![::] | T![:::]                         => (BindingPower(17), Assoc::Left),
        _ => return None,
    })
}

fn prefix_binding_power(kind: RSyntaxKind) -> Option<BindingPower> {
    Some(match kind {
        T![?]         => BindingPower(1),
        T![~]         => BindingPower(5),
        T![!]         => BindingPower(8),
        T![+] | T![-] => BindingPower(14),
        _ => return None,
    })
}
```

#### The loop

```rust
pub(crate) fn parse_expression(p: &mut RParser) -> ParsedSyntax {
    parse_expression_with_bp(p, BindingPower(0))
}

fn parse_expression_with_bp(p: &mut RParser, min_bp: BindingPower) -> ParsedSyntax {
    let Present(lhs) = parse_prefix_expression(p) else { return Absent };
    Present(parse_expression_with_lhs(p, lhs, min_bp))
}

/// The Pratt loop, entered with an already-parsed left operand.
///
/// Split out so a caller that has speculatively parsed an atom can hand it over
/// instead of rewinding. `parse_argument` (§11.3) is the one such caller: it
/// parses a potential argument *name*, and when no `=` follows, the atom is
/// simply the start of the argument's value and continues from here.
fn parse_expression_with_lhs(
    p: &mut RParser,
    mut lhs: CompletedMarker,
    min_bp: BindingPower,
) -> CompletedMarker {
    // Postfix is rank 18 and applies unconditionally, so there is no `min_bp`
    // guard around it below. That is only sound while 18 is genuinely the
    // ceiling: `min_bp` arrives as 0 from `parse_expression` and
    // `parse_argument`, as a prefix bp (max 14) from `parse_prefix_expression`,
    // and otherwise as this loop's own `right_bp` — which is `bp + 1` for
    // left-assoc, topping out at 18 because the highest infix rank is `:::` at
    // 17.
    //
    // Asserted rather than branched on. A branch here would be dead code that
    // silently starts mattering if someone adds an infix operator at rank 18 or
    // above; the assertion instead fails loudly and points at the real problem,
    // which is that the precedence table needs renumbering to keep postfix
    // tightest. Placed at entry so it fires on any input, not only ones that
    // happen to contain a call.
    debug_assert!(
        min_bp <= POSTFIX_BINDING_POWER,
        "min_bp {min_bp:?} exceeds postfix rank {POSTFIX_BINDING_POWER:?}; \
         postfix must remain the tightest-binding construct — renumber the \
         precedence table in §11.2",
    );

    loop {
        // R's statement-termination rule: at top level or inside `{`, a newline
        // before an infix operator ends the expression. `1\n+2` is two
        // statements; `1 +\n2` is one. Inside `(`/`[`/`[[` newlines are eaten
        // and this check is disabled.
        if p.scope().newlines_are_significant() && p.has_preceding_line_break() {
            break;
        }

        // Postfix `(`, `[`, `[[` bind tightest and are handled here rather than
        // through `infix_binding_power`. Unconditional: there is no caller for
        // which refusing a postfix would be correct — `a::b(c)` is `(a::b)(c)`.
        if let Some(next) = parse_postfix(p, lhs) { lhs = next; continue }

        let Some((bp, assoc)) = infix_binding_power(p.cur()) else { break };
        if bp < min_bp { break }

        let m = lhs.precede(p);
        let operator = p.cur();
        // Captured before the bump: `expected_selector` labels the operator
        // itself, so it needs the range while it is still current.
        let operator_range = p.cur_range();
        p.bump_any();

        // Newlines AFTER an infix operator are insignificant for every operator
        // in the table above except `::` and `:::`. Verified against R's own
        // lexer rather than taken from tree-sitter-r's note: `yylex` sets
        // `EatLines = 1` after each of `+ - * / ^ < <= >= > == != | & || &&
        // |> => %…% ? ! = : ~ $ @ <- <<- := -> ->>` (and the loop/function
        // keywords), and `NS_GET`/`NS_GET_INT` appear nowhere in that switch.
        // The preceding SYMBOL/STR_CONST sets `EatLines = 0`, so the flag is
        // still 0 when `::` is scanned and the newline survives as a token the
        // grammar has no rule for.
        //
        // Note `$` and `@` ARE in R's list, so `df$\n  x` is legal R and must
        // parse clean here. Only the namespace operators are restricted.
        let right_bp = match assoc {
            Assoc::Left  => BindingPower(bp.0 + 1),
            Assoc::Right => bp,
        };

        let node_kind = match operator {
            // R suspends the restriction inside `(`, `[`, `[[` — the newline
            // discard there fires before `EatLines` is consulted at all:
            //
            //     if (EatLines || *contextp == '[' || *contextp == '(')
            //         goto again;
            //
            // `{` pushes LBRACE, not `(`, so braces do NOT get this —
            // `newlines_are_significant()` draws the line in the same place,
            // which is the whole of the check.
            T![::] | T![:::]
                if p.scope().newlines_are_significant() && p.has_preceding_line_break() => {
                p.error(diagnostics::expected_selector(
                    p, operator, operator_range, p.cur_range(),
                ));
                R_NAMESPACE_EXPRESSION   // `right` slot left missing
            }
            T![$] | T![@] | T![::] | T![:::] => {
                parse_selector(p).or_add_diagnostic(p, |p, range| {
                    diagnostics::expected_selector(p, operator, operator_range, range)
                });
                match operator {
                    T![$] | T![@] => R_EXTRACT_EXPRESSION,
                    _ => R_NAMESPACE_EXPRESSION,
                }
            }
            _ => {
                parse_expression_with_bp(p, right_bp)
                    .or_add_diagnostic(p, diagnostics::expected_expression);
                R_BINARY_EXPRESSION
            }
        };
        lhs = m.complete(p, node_kind);
    }

    lhs
}

fn parse_prefix_expression(p: &mut RParser) -> ParsedSyntax {
    if let Some(bp) = prefix_binding_power(p.cur()) {
        let m = p.start();
        p.bump_any();
        parse_expression_with_bp(p, bp)
            .or_add_diagnostic(p, diagnostics::expected_expression);
        return Present(m.complete(p, R_UNARY_EXPRESSION));
    }
    parse_primary_expression(p)
}

/// One operand: no operators, nothing to its left.
///
/// The fan-out point of the expression grammar — every phase from here to 13
/// makes its construct reachable by adding an arm. Shown finished; Phase 8 has
/// the two §11.1 arms and `_ => Absent`, and nothing else.
fn parse_primary_expression(p: &mut RParser) -> ParsedSyntax {
    let literal = parse_literal_expression(p);                        // §11.1
    if literal.is_present() {
        return literal;
    }

    match p.cur() {
        T!['(']                 => parse_parenthesized_expression(p), // §11.3
        T!['{']                 => parse_braced_statements(p),        // §11.7
        T![function] | T!["\\"] => parse_function_definition(p),      // §11.5
        T![if]                  => parse_if_statement(p),             // §11.6
        T![for]                 => parse_for_statement(p),
        T![while]               => parse_while_statement(p),
        T![repeat]              => parse_repeat_statement(p),
        ERROR_TOKEN             => parse_error_token(p),              // §11.1
        _ => Absent,
    }
}
```

**`Absent` here is a result, not a failure.** It is how `f(,,)` gets its empty
arguments, how `f(a = )` gets its empty value, and how `parse_statement_list`
knows to recover. Only the caller knows whether an operand was required, so this
function raises no diagnostic of its own — `parse_error_token` is the one arm
that could and deliberately does not, because the lexer has already pointed at
the character (§11.1).

**Precedence facts worth writing explicit tests for**, all of which the
`Operator precedence` and `Precedence Tests` corpus tests cover:

- `-1^2` → `-(1^2)` — unary minus (14) binds looser than `^` (15).
- `!a & b` → `(!a) & b` — `!` (8) binds tighter than `&` (7).
- `-2:3` → `-(2:3)`? **No** — unary `-` is 14 and `:` is 13, so it is `(-2):3`,
  the sequence from -2 to 3. Contrast `-1^2` above: `^` (15) outranks unary `-`,
  `:` does not.
- `a <- b <- c` → `a <- (b <- c)` — right assoc.
- `a -> b -> c` → `(a -> b) -> c` — left assoc.
- `2^3^2` → `2^(3^2)` — right assoc.
- `a %in% b + c` → `a %in% (b + c)`? **No** — `%in%` is 12, `+` is 10, so
  `(a %in% b) + c`. This one trips people up; test it.

The newline behaviour of the operator table is a separate axis and is settled
above: every operator here eats a following newline except `::` and `:::`, and
that exception is itself suspended inside `(`/`[`/`[[`. The two tests pinning it
down live in Phase 10 with the rest of the namespace work (§11.4).

**Three aether-authored tests for the statement separator rule** go in
`statements/newlines.md`, which this phase turns green. Two of them guard
interactions rather than the rule itself:

````markdown
## Two Expressions On One Line [error]

```r
1 2
```

## Two Expressions On One Line Inside Braces [error]

```r
{ 1 2 }
```

## A Comment Does Not Hide A Line Break

```r
1 # c
2
```
````

The third has no marker because it is a **clean** parse, and it is the one that
can regress silently. It only passes because §8.2's `after_line_break` flag is sticky
across trivia, so the line break still reaches `2` with a comment sitting between.
If that stickiness breaks, this test fails and the lexer bug is caught at its
source rather than surfacing later as a mystery `expected_statement_separator`.
The second confirms the rule survives `parse_braced_statements` clearing
newlines significant again (§11.7). All three are aether-authored, so they are absent from
`MANIFEST.toml` and do not move the 89 count.

**Verify when the Phase 9 tests come off `[pending]`:**
`Not A Call, Subset, Or Subset2` and the three `Not A Parenthesized Expression`
tests all arrive as `[error]` from `expressions-errors.txt`, and all concern a
construct split across a newline. Under R, `f\n(x)` is two perfectly valid
statements. This separator check is the code that decides whether aether agrees
with tree-sitter that they are errors; if it diverges, the row belongs in
`docs/deviations.md` under the inherited **"tree-sitter-r vs. R"** heading (§8.4),
not aether's own list.

**Enable:** `expressions/unary.md`, `expressions/binary.md`,
`expressions/colon.md`, `expressions/specials.md`, `expressions/formula.md`,
`expressions/help.md`, `expressions/assignment.md`, `statements/newlines.md`, and
all of `expressions/precedence.md` except `Subset and Subset2 Precedence`
(which needs Phase 9). 13 tests.

#### The diagnostic this phase adds

`expected_expression` is the first of the `expected_*` family to land (§9.4).
Both call sites in the loop above are its whole reason for existing: an infix
operator whose right-hand side never arrives, and a prefix operator with nothing
to apply to.

```rust
/// An expression is required and absent
pub(crate) fn expected_expression(p: &RParser, range: TextRange) -> ParseDiagnostic {
    expected_node("expression", range, p)
}
```

Later phases reuse it rather than wording their own — the function body of a
`function`, the branches of an `if`, and a statement that turns out to be only a
`;` all mean the same thing to the reader.

### 11.3 Phase 9 — Calls, subset, subset2

`src/syntax/call.rs`.

```rust
/// Postfix: `f(x)`, `x[i]`, `x[[i]]`. Returns `None` if no postfix applies.
fn parse_postfix(p: &mut RParser, lhs: CompletedMarker) -> Option<CompletedMarker> {
    // `f\n(x)` at statement level is two statements, not a call. Already
    // guaranteed by the statement-break check in the caller, but the errors
    // corpus tests this directly ("Not A Call, Subset, Or Subset2").
    match p.cur() {
        T!['('] => {
            let m = lhs.precede(p);
            parse_call_arguments(p);
            Some(m.complete(p, R_CALL))
        }
        T!['['] => {
            let m = lhs.precede(p);
            parse_subset_arguments(p);
            Some(m.complete(p, R_SUBSET))
        }
        T!["[["] => {
            let m = lhs.precede(p);
            parse_subset2_arguments(p);
            Some(m.complete(p, R_SUBSET2))
        }
        _ => None,
    }
}

/// `(a)` — a node of its own, not a formatting hint, so air can round-trip it.
fn parse_parenthesized_expression(p: &mut RParser) -> ParsedSyntax {
    if !p.at(T!['(']) { return Absent }
    let m = p.start();
    p.bump(T!['(']);
    p.with_scope(Scope::Parenthesis, |p| {
        parse_expression(p).or_add_diagnostic(p, diagnostics::expected_expression);
    });
    p.expect(T![')']);
    Present(m.complete(p, R_PARENTHESIZED_EXPRESSION))
}
```

**`(` is the one token in this phase with two jobs**, and the Pratt structure
separates them with no lookahead at all. `parse_postfix` meets it with an operand
already to its left and builds an `R_CALL`; `parse_primary_expression` (§11.2)
meets it where an operand is due and builds a group. `f(x)` and `f (x)` are both
calls; `f\n(x)` is not, because the statement break ends the loop before
`parse_postfix` is reached. What it is *instead* — two statements, as in R, or an
error, as in tree-sitter — is the question §11.2 leaves open under "Verify when
the Phase 9 tests come off `[pending]`", and `Not A Call, Subset, Or Subset2` is
where it gets settled.

The body is a full `parse_expression`, not one restricted to §11.2's table: R's
`'(' expr_or_assign_or_help ')'` admits `=` and `?`, so `(a = 1)` is an
assignment and must parse clean. `()` has no such excuse and takes the
`expected_expression` diagnostic, then the `expect` closes the node.

#### `[[` and `]]` — the one genuinely hard tokenization problem

tree-sitter-r solves this with an external-scanner scope stack, and it has a filed
bug about it (#177, `Unsupported Subset2 With ]] Over Multiple Lines`) plus four
errors-corpus tests about mismatched brackets.

**Approach: greedy openers, one-at-a-time closers, join via re-lex.** §8.4 makes
the case for the asymmetry and names the precedents; what it means here is that a
run of closing brackets reaches the parser as single `]` tokens, and the subset2
that wants a `]]` asks for one. `L_BRACK2` arrives whole; `R_BRACK2` exists only
because a list asked for it.

**The join must happen at the loop guard, not after the list returns.** This is
the detail the whole approach turns on. Only the guard can tell a `]` that closes
*this* list from a `]` that closes someone else's, and if it cannot, the list
never recognises its own terminator: at `]` the guard `p.at(end)` is false, it is
not at `,` either, so every valid `x[[1]]` leaves through the error-recovery
branch instead of the loop condition. It happens to survive that today only
because `]` is in the recovery set and recovery therefore consumes nothing —
resting a correct parse of valid code on a recovery set's contents is exactly the
kind of load-bearing accident this section exists to prevent.

So the list has to own its terminator, including the case where the lexer refused
to spell that terminator out:

```rust
// `RReLexContext::JoinBrackets` landed with the lexer in Phase 4; this is its one
// and only call site.

// crates/aether_r_parser/src/syntax/call.rs
//
/// What closes an argument list. A type rather than a bare `RSyntaxKind`,
/// for the same reason `StatementListEnd` (§11.7) is one: the recovery set is
/// derived from the terminator instead of hardcoded at each call site. Here
/// there is a second reason — `[[` has a terminator the lexer deliberately does
/// not produce, and only the terminator itself knows to ask for it.
#[derive(Debug, Clone, Copy)]
pub(crate) enum ArgumentListEnd {
    RParen,   // `f(…)`
    RBrack,   // `x[…]`
    RBrack2,  // `x[[…]]` — its `]]` arrives as two `]` and has to be asked for
}

impl ArgumentListEnd {
    /// True when the current token closes this list.
    ///
    /// **Side-effecting by design.** For `RBrack2`, an adjacent pair of `]` is
    /// joined into one `]]` before the answer is returned, so every caller
    /// downstream — the loop guard, the recovery set, and the final `expect` —
    /// sees the exact closing token and none of them has to know the join
    /// happened.
    fn join_and_check(self, p: &mut RParser) -> bool {
        match self {
            Self::RParen => p.at(T![')']),
            Self::RBrack => p.at(T![']']),
            Self::RBrack2 => {
                // `x[[a]]` — this list wants both brackets, and gets them only
                // if they are adjacent. `x[[a] ]` and #177's `x[[a]\n]` are
                // errors in tree-sitter-r and stay errors here (§8.4).
                if p.at(T![']']) {
                    p.re_lex(RReLexContext::JoinBrackets);
                }
                p.at(T!["]]"])
            }
        }
    }

    /// Deliberately uniform across all three variants, and deliberately wider
    /// than this list's own terminator: recovery must never consume a closing
    /// delimiter of ANY kind. Eating one turns a single error into a cascade by
    /// stripping an enclosing construct of its terminator — the same rule as
    /// `a$)` in §11.4, where the `)` is left alone for exactly this reason.
    ///
    /// There is no `T!["]]"]` here, and that is the payoff of §8.4's direction
    /// rather than an oversight: nothing but `join_and_check` produces that
    /// token, and it only does so when the list is about to terminate, so
    /// recovery can never be looking at one. A lexer that glued closers would
    /// oblige this set — and every other set in the parser that mentions `]` —
    /// to name both arities, and to swallow two closers wherever one was missed.
    fn recovery_set(self) -> TokenSet<RSyntaxKind> {
        token_set![
            T![,],
            T![')'], T![']'], T!['}'],
            T![EOF],
        ]
    }
}

fn parse_subset2_arguments(p: &mut RParser) {
    let m = p.start();
    p.bump(T!["[["]);
    p.with_scope(Scope::Bracket2, |p| {
        parse_argument_list(p, ArgumentListEnd::RBrack2);
    });
    // The list joined its `]]` on the way out, so this is a plain expect.
    p.expect(T!["]]"]);
    m.complete(p, R_SUBSET2_ARGUMENTS);
}
```

**Nothing is ever undone, and the ladder cases confirm it** — worth stating,
because it is where a reader will stop. `x[[a[b]]]` lexes as
`x` `[[` `a` `[` `b` `]` `]` `]`. The inner `[` takes the first `]` without asking
anyone; the outer subset2 then joins the two that are left. `x[[a[[b]]]]` is the
same story twice: the inner subset2 joins a pair, and the outer joins the pair
behind it. Because a join reads forward from the current token and never rewinds
past it, no split, merge, or unget is ever needed.

Add the nesting ladder as aether-authored tests in `expressions/subset.md`:
`x[a[b]]`, `x[[a[b]]]`, `x[a[[b]]]`, `x[[a[[b]]]]`. All clean parses, no markers —
which is the point, since the failure mode here produces errors on *valid* code
and the imported corpus only covers mismatched brackets and `#177`.

**Verify `re_lex` against buffered lookahead** at the pinned rev before writing
this. `TokenSourceWithBufferedLexer` caches lookahead tokens, and re-lexing the
current token must invalidate that cache. `BufferedLexer::re_lex` clears the
lookahead whenever the kind changes, which is the contract `biome_js_parser`
relies on for `>>` — but confirm it holds through `RTokenSource`, because a stale
cache here yields a wrong tree that still round-trips, which is precisely the
class of bug no invariant in this project catches.

The mirror case (`x[[a]` — a `[[` that should have been two `[`) is a genuine
syntax error in R; produce a diagnostic and recover rather than trying to be
clever. Match tree-sitter's behavior only insofar as the errors corpus demands.

#### Argument lists and holes

R's argument syntax is unusually permissive and the `.ungram` already documents it
precisely. Reproduce the listed behavior exactly:

```
Allowed:      fn()  fn(,)  fn(,,)  fn(a)  fn(a, b)  fn(,,a,,b,,)
              fn(a, # comment_between_commas
              ,)
Not allowed:  fn(a b)
```

```rust
fn parse_argument_list(p: &mut RParser, end: ArgumentListEnd) {
    let m = p.start();
    let mut progress = ParserProgress::default();
    let mut first = true;

    loop {
        // Joins a `]]` as a side effect when `end` is `RBrack2`, so from here
        // down the terminator is always a single, exact token.
        if end.join_and_check(p) || p.at(T![EOF]) { break }

        // `fn(,,a,,b,,)` means an iteration may legitimately consume only a
        // comma, so progress is asserted across the whole iteration rather than
        // per-argument. What it catches is neither being consumed: `f(a}` leaves
        // `expect(,)` erroring without advancing and `parse_argument` producing
        // an empty argument, which without this guard repeats forever. Six of the
        // ten tests in `recovery/unmatched-delimiters.md` are this shape.
        progress.assert_progressing(p);

        if !first {
            // Not at `,` and not at the terminator: recover to one or the other
            // rather than spinning. `enable_recovery_on_line_break()` is
            // deliberately NOT set — we are inside `(`/`[`/`[[`, where newlines
            // carry no meaning (§9.2).
            //
            // The recovery happens INSIDE an `RArgument`, filling its
            // `value: AnyRExpression?` slot. The list is generated as
            // `make_separated_list_syntax(kind, children, RArgument::can_cast, …)`,
            // so only `RArgument` may be a child — a bare bogus node here would be
            // dropped or reshape the list. Same constraint, same shape, as
            // `parse_statement_list` (§11.7).
            if !p.at(T![,]) {
                let arg = p.start();
                let recovered = ParseRecoveryTokenSet::new(
                    R_BOGUS_EXPRESSION,
                    end.recovery_set(),
                )
                .recover(p);
                if recovered.is_err() {
                    arg.abandon(p);
                    break;
                }
                arg.complete(p, R_ARGUMENT);
            }
            p.expect(T![,]);
        }
        first = false;

        parse_argument(p);
    }

    m.complete(p, R_ARGUMENT_LIST);
}

/// One argument: `[name =] [value]`, both halves optional.
///
/// An empty argument opens and immediately closes the marker, pushing no
/// children. This is the documented technique in `r.ungram`, and it is what
/// makes `fn(,,a,,b,,)` representable.
///
/// **Parse optimistically, reshape with `precede`.** The name and the start of a
/// value are the same tokens, so the decision cannot be made before parsing —
/// but it does not need a rewind either. Parse the atom, then look at what
/// follows: an `=` means wrap it in an `R_ARGUMENT_NAME_CLAUSE`, anything else
/// means it was the head of the value all along and continues into the Pratt
/// loop as its lhs. Nothing is ever unparsed.
///
/// This is sound because R admits only a bare symbol, string, `...`, `..i`, or
/// `NULL` as an argument name — `f(a$b = 1)` is not legal — so
/// `parse_argument_name` can never consume more than the value would have.
fn parse_argument(p: &mut RParser) {
    let m = p.start();

    match parse_argument_name(p) {
        // `name = value`.
        Present(name) if p.at(T![=]) => {
            let clause = name.precede(p);
            p.bump(T![=]);
            clause.complete(p, R_ARGUMENT_NAME_CLAUSE);
            parse_expression(p).ok();
        }
        // Not a name clause after all — resume with the atom as the lhs.
        Present(atom) => { parse_expression_with_lhs(p, atom, BindingPower(0)); }
        Absent => { parse_expression(p).ok(); }
    }

    m.complete(p, R_ARGUMENT);
}

/// identifier | string | `...` | `..i` | `NULL`.
///
/// `NULL` is allowed here and nowhere else — tree-sitter-r's comment notes this
/// has been true since the beginning of R (`fn(NULL = 1)` parses) and is
/// surprising because `NA`/`Inf` are not allowed, and `NULL` is not special in
/// `x$NULL` or `function(NULL) {}`. See their issue #164.
fn parse_argument_name(p: &mut RParser) -> ParsedSyntax {
    match p.cur() {
        IDENT       => { let m = p.start(); p.bump_any(); Present(m.complete(p, R_IDENTIFIER)) }
        DOTS        => { /* R_DOTS */ }
        DOTDOTI     => { /* R_DOT_DOT_I */ }
        T![null]    => { /* see the open question below */ }
        STRING_OPEN => parse_string_value(p),
        _ => Absent,
    }
}
```

**No checkpoint, no `nth` offsets.** The earlier sketch of this function looked
one token ahead for `=` and fell back to `ParserContextCheckpoint` for strings,
because a string is three tokens — or **two** when empty, so `f("" = 1)` puts the
`=` at `nth(2)` while `f("a" = 1)` puts it at `nth(3)`. Worse, that offset is not
merely variable but unknowable: `BufferedLexer` lexes ahead with
`RLexContext::default()`, so `p.nth(2)` past a `STRING_OPEN` would re-lex the
string body in `Regular` context and return something unrelated to what the
string-context bump will actually produce. Confirmed at the pinned rev —
`LookaheadIterator::next()` calls `lexer.next_token(Lex::LexContext::default())`,
and `next_token` calls `reset_lookahead()`, which rewinds, as soon as a
non-regular context arrives. It is the standard Biome contract and the reason
`BumpWithContext` is a separate trait.

The kinds that lookahead returns are wrong, then, but nothing worse: §8.2.1 keeps
the lexer free of state that a rewind cannot restore, so a lookahead that wanders
into a string body cannot corrupt the real pass over it. Were that not arranged,
this section would be describing a correctness requirement rather than a
readability preference.

Parsing first sidesteps all of it. Every token is lexed exactly once, all five
name forms take one path, and the `f("" = 1)` asymmetry stops existing rather
than being handled.

`==` never reaches the `p.at(T![=])` test as two tokens: the lexer's maximal munch
(§8.4) returns a single `==`, so `f(a == 1)` takes the `Present(atom)` arm and the
`==` is consumed by the Pratt loop as an ordinary infix operator.

**Open question before writing this — check `r.ungram`'s `AnyRArgumentName`.** If
it declares a distinct kind for `NULL`-as-a-name rather than reusing
`R_NULL_EXPRESSION`, then `parse_argument_name` completes the atom as that kind,
and the `Present(atom)` fallback hands the Pratt loop a node whose kind is only
valid in name position. Air's ungram settles it; if a distinct kind exists, either
delay the `complete` until the `=` is known, or special-case `T![null]` back to
the ordinary literal path. The other four forms are unaffected — an
`R_IDENTIFIER` or `R_STRING_VALUE` is equally valid in both positions.

**Enable:** `expressions/call.md`, `expressions/subset.md`,
`expressions/subset2.md`, `expressions/parenthesized.md`, the remaining
`Subset and Subset2 Precedence` in `expressions/precedence.md`, and the four
bracket-mismatch tests in `recovery/unmatched-delimiters.md`
(`Opening Bracket2, …` and the three `Opening Bracket…Bracket2` variants).

The other six delimiter tests in that file stay `[pending]` until Phase 14 — they
depend on brace and statement-list recovery that does not exist yet.

#### The diagnostic this phase adds

```rust
/// An argument is required and absent
pub(crate) fn expected_argument(p: &RParser, range: TextRange) -> ParseDiagnostic {
    expected_node("argument", range, p)
}
```

Note what this is *not* for. A hole is legal in an argument list, so `f(a,,b)`
raises nothing at all; this fires only where an argument is required and the
recovery has nothing to put there. The missing-comma case (`f(a b)`) is a
separate rule, enforced by `expect(T![,])` in `parse_argument_list` above, and
the parameter-list analogue (`function(a,,b)`, which R *does* reject) gets its
own constructor in §11.5.

### 11.4 Phase 10 — Extract and namespace

Small phase, but it is where aether's first user-visible deviation lands, so keep
it separate for reviewability.

```rust
/// `AnyRSelector` = identifier | string | `...` | `..i`.
fn parse_selector(p: &mut RParser) -> ParsedSyntax {
    match p.cur() {
        IDENT   => { let m = p.start(); p.bump_any(); Present(m.complete(p, R_IDENTIFIER)) }
        DOTS    => { /* R_DOTS */ }
        DOTDOTI => { /* R_DOT_DOT_I */ }
        STRING_OPEN => parse_string_value(p),
        _ => Absent,
    }
}
```

Behavior matrix — this is the deviation table for `docs/deviations.md`:

| Source | tree-sitter-r | aether |
|---|---|---|
| `a$b` | `extract_operator` | `R_EXTRACT_EXPRESSION`, clean |
| `a$"b"` | `extract_operator` | clean |
| `a$` | `extract_operator`, rhs absent, **no error** | node + missing slot + `expected_selector` |
| `a$1` | rhs absent, `1` becomes a sibling statement, **no error** | bogus rhs (one atom) + `expected_selector` |
| `a$NULL`, `a$TRUE`, `a$(b)`, `a${b}` | same as above | bogus rhs (one atom) + `expected_selector` |
| `a$1 + 2` | as above, `+ 2` reattached to the sibling | `(a$«1») + 2` — infix not consumed |
| `foo::` | `namespace_operator`, rhs absent, no error | missing slot + `expected_selector` |
| `foo::\nbar` | error (grammar says no newlines allowed) | error, matched |
| `f(foo::\nbar)` | *(unverified — see below)* | **clean**, matching R |
| `a$\nb` | clean | clean, matching R |

**The two newline rows are the ones to get right, and neither is covered by an
imported test.** R's rule, read off `yylex` in `src/main/gram.y` rather than
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

`newlines_are_significant()` draws the line in exactly that place, so the check in
§11.2 is one guard clause. Add both as aether-authored tests in `expressions/namespace.md`
and `expressions/extract.md`:

````markdown
## Namespace Newline Is Significant Only Outside Parens

```r
f(stats::
  median)
```

## Dollar Tolerates A Newline

```r
df$
  x
```
````

The first needs no marker — it is a *clean* parse, which is the surprising half.
Whether tree-sitter-r agrees is worth checking rather than assuming when the
oracle first runs against it; if it errors, the row above becomes a real deviation
and moves into `docs/deviations.md` under the inherited **"tree-sitter-r vs. R"**
heading (§8.4), not aether's own list.

**The `a$1` case — settled: consume into a bogus RHS.** Today air produces
`right: missing (required)` *and* a separate `RDoubleValue` sibling for the `1`,
silently. Aether consumes the `1` into a bogus RHS so it does not leak out as a
phantom statement.

**How much it consumes is the part that needs deciding, and the answer is exactly
one atom** — a prefix expression, with no postfix and no infix. Not a full
`parse_expression`, because that would change the shape of valid code around the
error:

| Source | Tree | Why |
|---|---|---|
| `a$1 + 2` | `(a$«1») + 2` | `+` is left for the Pratt loop, as in valid code |
| `a$1(x)` | `(a$«1»)(x)` | mirrors `a$f(x)`, which is `(a$f)(x)` in R — call is rank 18, `$` is 16 |
| `a$(b)` | `a$«(b)»` | the parenthesized expression is one atom |
| `a$-1` | `a$«-1»` | prefix operators are part of the atom |
| `a$)` | `a$` + missing slot | `)` starts no expression; **do not consume it**, or the enclosing call loses its terminator and one error becomes a cascade |

So the recovery calls `parse_prefix_expression` — the same function the Pratt loop
uses for its lhs — and wraps whatever comes back. When it returns `Absent`, this
degrades to the plain `a$` case: missing slot, one diagnostic, nothing consumed.

Either way it is **one diagnostic**, not one per malformed token, and the
round-trip invariant holds because the offending tokens are inside the tree rather
than dropped.

**The bogus kind is `R_BOGUS_SELECTOR`**, which exists only because Phase 3 added
it and put `RBogusSelector` in the `AnyRSelector` union (§7). Without a bogus
variant in that union the factory silently reshapes the whole `a$1` into one bogus
node instead of an `R_EXTRACT_EXPRESSION` with a bogus RHS — no panic, no compile
error, and round-trip still passes. So `«…»` in the table above reads
`R_BOGUS_SELECTOR` throughout, wrapping whatever single atom
`parse_prefix_expression` returned: an `R_DOUBLE_VALUE` for `a$1`, an
`R_PARENTHESIZED_EXPRESSION` for `a$(b)`, an `R_UNARY_EXPRESSION` for `a$-1`.

Two aether-authored tests are worth adding for the boundary cases, since the
imported corpus only covers bare `a$1`: `a$1 + 2` for the "infix is not consumed"
rule, and `a$)` inside a call — `f(a$)` — for the "do not eat the terminator"
rule. Both `[error]`.

**Enable:** `expressions/extract.md` and `expressions/namespace.md` in full — 10
tests, of which three carry `[error, deviation]` (Dollar No RHS, Slot No RHS,
Namespace With Missing RHS) and two carry `[error]` (the `if` RHS and
Newline Before Operator cases). Air's `undefined/extract_expression_error.R` and
`undefined/namespace_expression_error.R` live in these two files as well and
become `[error]` tests. The two newline tests above are aether-authored, so they
are absent from `MANIFEST.toml` and do not move the 89 count.

#### The diagnostic this phase adds

`$`, `@`, `::`, and `:::` all funnel through one constructor, so it names the
operator that was actually written — a message listing all four tells the reader
nothing about which one they typed. The secondary label points at the operator,
which is why the Pratt loop captures `operator_range` **before** the bump
(§11.2).

```rust
/// The name on the right of `$`, `@`, `::`, or `:::` is missing
pub(crate) fn expected_selector(
    p: &RParser,
    operator: RSyntaxKind,
    operator_range: TextRange,
    range: TextRange,
) -> ParseDiagnostic {
    // Punctuation kinds render as their own text, so this stays correct if the
    // token set changes
    let operator = operator.to_string().unwrap_or("operator");

    expected_any(&["identifier", "string", "`...`", "`..i`"], range, p).with_detail(
        operator_range,
        format!("`{operator}` requires a name on the right."),
    )
}
```

`format!` rather than `markup!`: `with_detail` takes a `biome_console::fmt::Display`,
which `String` already satisfies, and `biome_parser` builds its own details the
same way. It keeps `biome_console` out of the crate's dependency list.

This is the constructor the deviation table above leans on — every row where air
produces no error and aether produces one is a row where this fires.

### 11.5 Phase 11 — Function definitions

`src/syntax/function.rs`.

```rust
fn parse_function_definition(p: &mut RParser) -> ParsedSyntax {
    if !p.at(T![function]) && !p.at(T!["\\"]) { return Absent }
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

**Enable:** the 7 imported tests across `corpus/functions/` — `definition.md`,
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
    if !p.at(T![if]) { return Absent }
    let m = p.start();
    p.bump(T![if]);
    p.expect(T!['(']);
    p.with_scope(Scope::Parenthesis, |p| {
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

**Enable:** the 11 imported tests across `corpus/control/` — `if.md`, `for.md`,
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

`src/syntax/block.rs`. This lands the semicolon deviation.

```rust
fn parse_braced_statements(p: &mut RParser) -> ParsedSyntax {
    if !p.at(T!['{']) { return Absent }
    let m = p.start();
    p.bump(T!['{']);
    // The one scope that makes newlines significant *again* from inside a
    // paren: `f({\n  a\n  b\n})` is three statements, not one.
    p.with_scope(Scope::Brace, |p| parse_statement_list(p, StatementListEnd::RCurly));
    p.expect(T!['}']);
    Present(m.complete(p, R_BRACED_STATEMENTS))
}

/// Where a statement list stops: end of file, or the `}` of a braced block.
/// Owning the terminator as a type means the recovery set is derived from it
/// rather than hardcoded, so a future list terminator cannot forget to add
/// itself to the set. `ArgumentListEnd` (§11.3) is the same pattern, and carries
/// the additional job of asking the lexer for a `]]`.
#[derive(Debug, Clone, Copy)]
pub(crate) enum StatementListEnd {
    Eof,
    RCurly,
}

impl StatementListEnd {
    fn matches(self, kind: RSyntaxKind) -> bool {
        match self {
            Self::Eof => kind == T![EOF],
            Self::RCurly => kind == T!['}'],
        }
    }

    /// Tokens a failed statement may resynchronise on: the statement separator,
    /// this list's own terminator, and EOF as the universal backstop.
    fn recovery_set(self) -> TokenSet<RSyntaxKind> {
        let base = token_set![T![;], T![EOF]];
        match self {
            Self::Eof => base,
            Self::RCurly => base.union(token_set![T!['}']]),
        }
    }
}

fn parse_statement_list(p: &mut RParser, end: StatementListEnd) {
    let m = p.start();
    let mut progress = ParserProgress::default();

    while !p.at(T![EOF]) && !end.matches(p.cur()) {
        progress.assert_progressing(p);

        let stmt = p.start();
        let previous_range = p.cur_range();
        let had_expression = parse_expression(p).is_present();

        if !had_expression && !p.at(T![;]) {
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
            let recovered = ParseRecoveryTokenSet::new(R_BOGUS_EXPRESSION, end.recovery_set())
                .enable_recovery_on_line_break()
                .recover(p);

            if recovered.is_err() {
                // Already at a recovery token with nothing consumed: abandon the
                // empty statement rather than completing a childless
                // `R_STATEMENT`, which `r.ungram` says is never produced.
                stmt.abandon(p);
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
        // Guarded on `had_expression` so the recovery branch above — which has
        // already emitted `expected_expression` — does not also complain about a
        // separator it was never going to reach.
        //
        // A bare `has_preceding_line_break()` suffices where the Pratt loop needs
        // `newlines_are_significant()` too: the only scopes a statement list runs
        // in are `TopLevel` and `Brace`, and in both of them it is already true.
        let ate_semicolon = p.eat(T![;]);
        if had_expression
            && !ate_semicolon
            && !p.at(T![EOF])
            && !end.matches(p.cur())
            && !p.has_preceding_line_break()
        {
            p.error(diagnostics::expected_statement_separator(
                p, previous_range, p.cur_range(),
            ));
        }

        stmt.complete(p, R_STATEMENT);
    }

    m.complete(p, R_STATEMENT_LIST);
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

**Enable:** `corpus/statements/` — `braces.md` and `semicolons.md`
(`newlines.md` went green back in Phase 8). This is the one directory that really
does finish here: braces and statement lists are the last constructs any merged
air snapshot in it can depend on. Every test in `semicolons.md` comes
from air's snapshots rather than tree-sitter and carries `[deviation]`.

#### The diagnostic this phase adds

```rust
/// Two expressions with nothing between them, as in `1 2`
pub(crate) fn expected_statement_separator(
    p: &RParser,
    previous: TextRange,
    range: TextRange,
) -> ParseDiagnostic {
    p.err_builder(
        "Expected a newline or a `;` between two expressions.",
        range,
    )
    .with_detail(previous, "This expression ends here.")
}
```

R separates statements by a newline or a `;` and accepts nothing else. `previous`
labels the expression that already ended, so the message can point at where the
separator belonged rather than only at what followed it.

The argument-list analogue (`f(a b)`) is a different rule, enforced by
`expect(T![,])` in §11.3 — the same shape of mistake, a different separator, and
different wording. Keep them apart: a reader who sees "expected a newline or a
`;`" inside a call has been told something false.

This is also the last of the family, so §9.4's table is complete once it lands.

---

## 12. Phase 14 — Recovery polish and air integration

### 12.1 Recovery review pass

By this point every feature is in, and recovery has been added ad hoc. Do a
dedicated pass:

- Every `parse_*` that can fail has a recovery set that includes the enclosing
  construct's closing token, plus `T![;]`, plus `enable_recovery_on_line_break()`
  wherever a newline genuinely bounds the construct.
- Every bogus node is overlapped by a diagnostic. `check()` step 4 has enforced
  this since Phase 6 (§10.2), so the Phase 14 job is to review the *wording* and
  the *placement* of those diagnostics, not to add the check — a diagnostic that
  satisfies the overlap test can still point at a range that reads badly. Air's
  `has_bogus_nodes_or_empty_slots` exists for the weaker version of this and its
  comment says so: *"used in the parser test to ensure it doesn't emit bogus nodes
  without diagnostics."*
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

4. **Update `air_r_formatter`** for the `R_STATEMENT` wrapper and real
   semicolon tokens. Expect churn in `r/expressions/expression_list.rs` and
   `r/expressions/braced_expressions.rs` (renamed to `braced_statements.rs`) and
   their snapshots.

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
| **`[[` / `]]` joining** | Genuinely ambiguous; tree-sitter needs an external scanner and still has open bugs. The lexer half landed in Phase 4; the parser half is Phase 9. | One join site, in the argument list's terminator check (§11.3). Six corpus tests exercise it. Do not invent a scope stack, and do not make the lexer glue closers. |
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
