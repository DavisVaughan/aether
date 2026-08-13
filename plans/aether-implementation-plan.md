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
| 3 | Vendor `tree-sitter-r` **and** air's tree-sitter→Rowan conversion as a differential oracle. **Local development tool, behind a Cargo feature, not run in CI**, and deleted outright at swap-in. |
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
| `crates/air_r_parser/src/parse.rs` | The tree-sitter walker. This is what we are replacing, and what the oracle will vendor. |
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
    │       │
    │       ├── parse.rs           # ← Phase 5: public `parse()` entry point
    │       ├── options.rs         # ← Phase 5
    │       ├── token_source.rs    # ← Phase 5: RTokenSource
    │       ├── parser.rs          # ← Phase 5: RParser + context flags
    │       ├── syntax.rs          # ← Phase 7 onward
    │       └── syntax/
    │           ├── expr.rs        # Pratt loop, primary exprs
    │           ├── call.rs        # call / subset / subset2 / arg lists
    │           ├── function.rs    # function defs, lambdas, parameters
    │           ├── control.rs     # if/else, for, while, repeat
    │           ├── block.rs       # braced exprs, statement lists, semicolons
    │           └── value.rs       # literals, strings, keywords-as-values
    ├── aether_r_test_macros/      # ← Phase 6. proc-macro: `gen_corpus_tests!`
    │   └── src/lib.rs             #   modelled on air's `tests_macros`
    ├── aether_r_parser_tests/     # ← Phase 6. Every corpus test, plus the
    │   ├── src/                   #   harness that runs them. Depends on the
    │   │   ├── document.rs        #   parser; nothing depends on it but `xtask`.
    │   │   ├── check.rs           # run one test, produce its cells (§10.2)
    │   │   └── runner.rs          # `run_file`: read-only compare; panics on
    │   │                          #   mismatch, for use from a `#[test]`
    │   └── tests/
    │       ├── main.rs            # globs corpus/**/*.md → crate::run_file
    │       ├── quick_test.rs      # scratchpad, #[ignore]d
    │       └── corpus/            # THE TESTS — §10.3.2 has the full mapping
    │           ├── MANIFEST.toml  # every imported test name → its home file
    │           ├── literals/      # each X.md has a sibling X.snap.md
    │           ├── expressions/
    │           ├── functions/
    │           ├── control/
    │           ├── statements/
    │           └── recovery/
    └── aether_r_oracle/           # ← Phase 6. Dev-only differential oracle
        ├── Cargo.toml             # depends on tree-sitter, tree-sitter-r
        └── src/
            ├── lib.rs
            └── treesitter.rs      # vendored air parse.rs walker
```

Everything above the `← Phase N` markers exists. The parser crate currently holds
the lexer and its diagnostics and nothing else; `lib.rs` carries an
`#[allow(dead_code)]` on both modules until `RTokenSource` drives them in Phase 5.

---

## 4. Phase plan

Fourteen phases. Each is one reviewable PR. Phases 1–6 are infrastructure and
should land quickly; the owner wants to review the *parser* in small pieces, not
the scaffolding.

**Every phase from 7 onward ends with the same three steps:**
1. Flip the `[pending]` markers off the corpus tests the feature enables.
2. Regenerate snapshots, eyeball every changed cell.
3. Run `just oracle` and confirm the diff is clean for all non-`[deviation]`
   tests. This is a deliberate local step — the oracle is feature-gated and CI
   does not run it (§10.4.1).

Phases 1–4 have landed. Their sections below are compressed to the decisions
later phases depend on; the repo, `docs/deviations.md`, and the code's own
comments are the source of truth for everything else.

| Phase | Title | Deliverable | |
|---|---|---|---|
| 1 | Repo bootstrap | Workspace builds, CI green, nothing else | ✅ |
| 2 | Vendor + rename syntax layer | `just codegen` reproduces `aether_r_syntax` + `aether_r_factory`, identical to air's modulo the rename | ✅ |
| 3 | Grammar deviations | `.ungram` + kinds updated, regenerated, documented | ✅ |
| 4 | Lexer | Standalone `RLexer`, plus seventeen `#[cfg(test)]` token-stream test files under `src/lexer/tests/` (§8.6) | ✅ |
| 5 | Parser scaffolding | `RParser`, `RTokenSource`, diagnostics, `parse()` returns an empty `R_ROOT` | |
| 6 | Test harness + corpus import | All 89 tree-sitter tests imported as `[pending]`; oracle wired up | |
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

- No recipe and no CI job passes `--features oracle`. The default test run builds
  no C dependency and stays fast (§10.4.1).

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
returns true for it. The harness's `has_bogus_nodes_or_empty_slots` check
(§10.2) depends on that.

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

- `RParser` reads the flag only through `at_statement_break()` (§9.2), which
  gates it on `IGNORE_NEWLINES`. That predicate has three callers: the Pratt loop
  (§11.2), `parse_postfix()` (§11.3), and `parse_else_clause()` (§11.6). Nothing
  else reads the raw flag.

Do not replicate tree-sitter's scope-stack approach of conditionally swallowing
newlines in the lexer. In a recursive-descent parser the equivalent information
is structural — the parser knows whether it is inside `(`/`[`/`[[` — so a lexer
scope stack would duplicate that knowledge and is the classic source of desync
bugs.

`Trivia` kinds map as follows, matching air's current `parse.rs` so the snapshots
line up:

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

## 9. Phase 5 — Parser scaffolding

No R grammar yet. Just the machinery, ending with `parse("")` returning a valid
empty `R_ROOT`.

### 9.1 `RTokenSource`

```rust
// crates/aether_r_parser/src/token_source.rs
use crate::lexer::{RLexContext, RLexer, RReLexContext};
use aether_r_syntax::RSyntaxKind::{self, *};
use biome_parser::lexer::{BufferedLexer, LexerCheckpoint};
use biome_parser::prelude::{TokenSource, Trivia, BumpWithContext, TokenSourceWithBufferedLexer};
use biome_rowan::{TextRange, TriviaPieceKind};

pub(crate) struct RTokenSource<'src> {
    lexer: BufferedLexer<RSyntaxKind, RLexer<'src>>,
    trivia: Vec<Trivia>,
}

impl<'src> RTokenSource<'src> {
    pub fn from_str(source: &'src str) -> Self {
        let lexer = BufferedLexer::new(RLexer::from_str(source));
        let mut source = Self { lexer, trivia: Vec::new() };
        source.next_non_trivia_token(RLexContext::default(), true);
        source
    }

    fn next_non_trivia_token(&mut self, context: RLexContext, first: bool) {
        // Buffer the gap before assigning positions: whether a whitespace piece
        // is trailing depends on what FOLLOWS it, so it cannot be decided as it
        // is lexed. This is the one place aether cannot copy `JsTokenSource`,
        // whose default is the opposite of air's — see the rules below.
        let mut gap: Vec<(TriviaPieceKind, TextRange)> = Vec::new();

        let terminator = loop {
            let kind = self.lexer.next_token(context);
            let trivia_kind = match kind {
                WHITESPACE => TriviaPieceKind::Whitespace,
                NEWLINE    => TriviaPieceKind::Newline,
                COMMENT    => TriviaPieceKind::SingleLineComment,
                _ => break kind,
            };
            gap.push((trivia_kind, self.lexer.current_range()));
        };

        // Air's `between_two_tokens`: a real token before AND after. Before the
        // first token, and in the final gap to EOF, everything is leading.
        let between_two_tokens = !first && terminator != EOF;
        let mut seen_newline = false;

        for i in 0..gap.len() {
            let (kind, range) = gap[i];
            let next = gap.get(i + 1).map(|(k, _)| *k);

            let trailing = if seen_newline {
                // Everything from the newline onward leads the next token.
                false
            } else {
                match kind {
                    // A comment on the same line as the preceding token is
                    // trailing of it. Air handles this in `handle_comment_leave`,
                    // which does not see `between_two_tokens`'s end-of-file
                    // reset, so it applies in the final gap too: `1 #` ends with
                    // a *trailing* comment.
                    TriviaPieceKind::SingleLineComment => !first,

                    // Whitespace is trailing only when it is the tail of the
                    // line: immediately before a same-line comment, or before
                    // the newline that ends the line. Whitespace with a real
                    // token after it on the same line is LEADING of that token —
                    // `1 + 2` attaches the spaces to `+` and `2`, not to `1`.
                    TriviaPieceKind::Whitespace => match next {
                        Some(TriviaPieceKind::SingleLineComment) => !first,
                        Some(TriviaPieceKind::Newline) => between_two_tokens,
                        _ => false,
                    },

                    TriviaPieceKind::Newline => false,
                    _ => false,
                }
            };

            if kind.is_newline() {
                seen_newline = true;
            }
            self.trivia.push(Trivia::new(kind, range, trailing));
        }
    }
}

impl<'src> TokenSource for RTokenSource<'src> {
    /// Delegate; keep no copy. `BufferedLexer` stores a `LexerCheckpoint` per
    /// cached lookahead token and reads the flag off *that* when lookahead is
    /// live, so delegation stays correct under `nth()`. A field maintained here
    /// in `next_non_trivia_token` would not — it would describe whichever token
    /// was last pulled through this function, not the current one.
    ///
    /// `JsTokenSource` is one line, and so is this.
    fn has_preceding_line_break(&self) -> bool {
        self.lexer.has_preceding_line_break()
    }

    // ... cur, cur_range, bump, skip_as_trivia, finish
}
```

**Trivia attachment must match air's existing rules exactly**, because a mismatch
shows up in every snapshot. The rules, read off air's `derive_trivia` and
`handle_comment_leave` rather than paraphrased:

- Before the first token, everything is **leading**. Same in the final gap
  between the last token and EOF — except for comments, see below.
- Between two real tokens, whitespace is **trailing only when it is the tail of
  the line**: immediately before a same-line comment, or before the newline that
  ends the line. Whitespace followed by a real token on the same line is
  **leading** of that token.
- A comment on the same line as the preceding token is **trailing** of it, along
  with the whitespace before it. A comment after a newline is leading.
- The newline itself, and everything after it, is **leading**. `\r\n` is one
  piece.

**Note the polarity: air's default is leading, and `JsTokenSource`'s is trailing.**
Copying JS here would put a diff in every snapshot that contains a space between
two tokens, which is all of them. Air's choice is arguably the odd one — see the
"Inherited from air" section of `docs/deviations.md` — but aether copies it
so the oracle stays useful, and revisits it after the swap if at all.

Air ships eight `test_parse_trivia_*` unit tests covering exactly these cases;
port them verbatim into aether. They are the cheapest possible guard against the
highest-risk drift in the project. The four that pin the rules above:

| Input | Expected |
|---|---|
| `1 + \n1` | `ws` leading, `ws` trailing, `nl` |
| `1 #` | `ws` trailing, `cmt` trailing |
| `1  \n ` | `ws` leading, `nl`, `ws` leading |
| `1 + \r\n1` | `ws` leading, `ws` trailing, `nl` spanning both bytes |

The first and third are worth staring at: the same whitespace-before-a-newline is
trailing in one and leading in the other, purely because a token follows in the
first. That asymmetry is `between_two_tokens` in the loop above.

Expect to spend real time here — trivia attachment is the highest-risk area for
silent snapshot churn, and it is exactly the kind of thing the differential oracle
exists to catch.

Implement `BumpWithContext` (for string lexing) and `TokenSourceWithBufferedLexer`
(which gives `NthToken` lookahead automatically).

#### 9.1.1 Handing the string delimiter to the parser

The lexer keeps no memory of the string it opened (§8.2.1), so the delimiter
`RLexContext::StringContent` and `RLexContext::StringEnd` name has to come from
the caller. That caller is `parse_string_value` (§11.1), and it needs no help
from either `RTokenSource` or `RParser` to get it:

```rust
let delimiter = RStringDelimiter::from_open(p.cur_text());
```

**Neither type gets a `string_delimiter()` method.** Verified at the pinned rev
rather than assumed — `Parser::cur_text()` is *provided*, and its body is already
the whole of what such a method would do:

```rust
// biome_parser/src/lib.rs
fn cur_text(&self) -> &str {
    &self.source().text()[self.cur_range()]
}
```

So a `RTokenSource::string_delimiter()` would hand-roll `cur_text()` out of
`TokenSource::text()` and `current_range()`, and `RParser` would then forward it
— two methods and a hop to reach an accessor `RParser` already has for free from
the trait it implements.

The token source has no *internal* use for the delimiter either, which is what
makes this clear-cut rather than a toss-up. `BumpWithContext::bump_with_context`
takes the context as an argument, so the context always flows parser → token
source; the token source never derives one. A method there would exist solely to
be forwarded out again.

The reading itself belongs to `RStringDelimiter` (§8.2.1) — the caller's job is
to know *which* token to read, and the delimiter's job is to know what its own
text looks like. The §8.6 test driver calls the same `from_open`, so the two
stand-ins for it cannot drift.

Three properties worth knowing before writing `parse_string_value`:

- **It is total.** Called behind `at(STRING_OPEN)` there is no failure case to
  handle: `from_open`'s raw fallback `expect` is unreachable, because the lexer
  only emitted the token by parsing the same bytes successfully. That guard is
  already the function's first line, so no `debug_assert` is needed beside the
  call.

- **It is stateless.** Nothing is remembered between the three bumps, so a
  checkpoint taken anywhere in a string and rewound to is as good as any other
  position — `RTokenSource` gains no field, and its checkpoint stays a delegation
  to `BufferedLexer`'s.

- **It costs one text slice per string,** and re-reads a prefix the lexer already
  walked. That is the price of the lexer being a pure function of position and
  context, and it is a few bytes on the open token, not on the body.

Do **not** thread the delimiter through `RParser` state, and do not look it up
again at `STRING_CONTENT` — that token's text is the body, not a delimiter.
`parse_string_value` reads it once, at the open, and passes the same value to
both bumps.

### 9.2 `RParser`

```rust
// crates/aether_r_parser/src/parser.rs
use crate::token_source::RTokenSource;
use aether_r_syntax::RSyntaxKind;
use biome_parser::diagnostic::merge_diagnostics;
use biome_parser::event::Event;
use biome_parser::prelude::*;
use biome_parser::ParserContext;

pub(crate) struct RParser<'src> {
    context: ParserContext<RSyntaxKind>,
    source: RTokenSource<'src>,
    options: RParserOptions,
    /// Stack-discipline context flags, saved and restored by `with_flags`.
    ///
    /// A bare field rather than a `state` struct: `biome_js_parser` has a
    /// `JsParserState` because it aggregates seven heterogeneous things (label
    /// sets, strict mode, duplicate-binding tracking, speculative-parsing
    /// memoization). R has exactly one, and no candidates to grow into it —
    /// there is no strict mode, no labelled `break`, and no backtracking at all
    /// (§12.2). Reintroduce a wrapper if a second kind of state ever appears.
    flags: RContextFlags,
}

bitflags::bitflags! {
    /// Context that changes how newlines are interpreted. Both flags answer the
    /// same question — "what does a line break mean here?" — and nothing else
    /// belongs in this struct.
    ///
    /// **Polarity is deliberate: empty means top-level behaviour.** A forgotten
    /// flag then produces a spurious statement break, which is a loud parse
    /// error at the first multi-line call in the corpus. The inverse spelling
    /// (`NEWLINE_SENSITIVE`) fails silently instead — `1\n+2` would quietly
    /// become one expression rather than two statements, a wrong tree with no
    /// diagnostic that the round-trip assertion does not catch either.
    #[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
    pub(crate) struct RContextFlags: u8 {
        /// Set inside `(`, `[`, or `[[`, where newlines carry no meaning — R's
        /// parser eats them, and so does tree-sitter-r's external scanner
        /// (`consume_whitespace_and_ignored_newlines`).
        const IGNORE_NEWLINES           = 1 << 0;
        /// Set inside `{`. R permits `else` on a line after the consequence only
        /// within braces; at top level the newline ends the `if` and the `else`
        /// is a syntax error.
        ///
        /// Named for what it licenses rather than for the `{` that sets it,
        /// because it has exactly one reader (`parse_else_clause`). If a second
        /// brace-specific rule ever appears, rename it back — one setter and one
        /// reader make that a two-line change.
        const ALLOW_NEWLINE_BEFORE_ELSE = 1 << 1;
    }
}

impl<'src> Parser for RParser<'src> {
    type Kind = RSyntaxKind;
    type Source = RTokenSource<'src>;
    fn context(&self) -> &ParserContext<Self::Kind> { &self.context }
    fn context_mut(&mut self) -> &mut ParserContext<Self::Kind> { &mut self.context }
    fn source(&self) -> &Self::Source { &self.source }
    fn source_mut(&mut self) -> &mut Self::Source { &mut self.source }
}

impl<'src> RParser<'src> {
    /// Run `f` with `set` inserted and `clear` removed, restoring the previous
    /// flags afterwards. `clear` is `empty()` at every call site but
    /// `parse_braced_statements`, which is the one construct that re-enters
    /// newline-significant territory from inside a paren.
    pub(crate) fn with_flags<F, R>(&mut self, set: RContextFlags, clear: RContextFlags, f: F) -> R
    where F: FnOnce(&mut Self) -> R {
        let saved = self.flags;
        self.flags.insert(set);
        self.flags.remove(clear);
        let result = f(self);
        self.flags = saved;
        result
    }

    /// True when a newline before an `else` is tolerated because we are inside
    /// braces. The only reader is `parse_else_clause` (§11.6); everything else
    /// asks `at_statement_break()`. An accessor rather than a public field, so
    /// that "exactly one reader" is enforced by privacy rather than by hope.
    pub(crate) fn allows_newline_before_else(&self) -> bool {
        self.flags.contains(RContextFlags::ALLOW_NEWLINE_BEFORE_ELSE)
    }

    /// True when a newline before the current token should end the current
    /// expression. False inside `(`/`[`/`[[`.
    ///
    /// The single predicate for newline significance. Both newline rules in the
    /// language — statement termination and `else` attachment — funnel through
    /// it; nothing else should read `IGNORE_NEWLINES` directly.
    pub(crate) fn at_statement_break(&self) -> bool {
        !self.flags.contains(RContextFlags::IGNORE_NEWLINES)
            && self.has_preceding_line_break()
    }

    pub(crate) fn finish(self) -> (Vec<Event<RSyntaxKind>>, Vec<Trivia>, Vec<ParseDiagnostic>) {
        let (trivia, lexer_diagnostics) = self.source.finish();
        let (events, parse_diagnostics) = self.context.finish();
        let diagnostics = merge_diagnostics(lexer_diagnostics, parse_diagnostics);
        (events, trivia, diagnostics)
    }
}
```

### 9.3 Public `parse()` — keep air's shape

Air's `Parse` struct must keep working, since air's `workspace`, `lsp`, and
`air_r_formatter` crates all consume it. But air's version holds
`error: Option<ParseError>` (a single string), which is no longer enough.

**The shape:** `Parse` keeps its existing method names, and gains diagnostics.
Preserving the names is what makes §12.3 step 3 true — air builds after the swap
with no `.rs` change outside the formatter — so it is a constraint, not a
preference.

```rust
// crates/aether_r_parser/src/parse.rs
pub struct Parse {
    root: SendNode,
    diagnostics: Vec<ParseDiagnostic>,
}

impl Parse {
    pub fn syntax(&self) -> RSyntaxNode { /* unchanged */ }
    pub fn tree(&self) -> RRoot { /* unchanged */ }
    pub fn has_error(&self) -> bool { self.diagnostics.iter().any(|d| d.is_error()) }
    pub fn diagnostics(&self) -> &[ParseDiagnostic] { &self.diagnostics }

    /// Back-compat with air's current API. Collapses diagnostics to one error.
    pub fn error(&self) -> Option<ParseError> { /* first error, formatted */ }
    /// Signature unchanged for air's callers; the body now formats the first
    /// error diagnostic instead of reading a stored `ParseError`.
    pub fn into_result(self) -> Result<RSyntaxNode, ParseError> { ... }
}

impl From<Parse> for AnyParse {
    fn from(parse: Parse) -> Self {
        AnyParse::new(parse.root, parse.diagnostics)  // now genuinely useful
    }
}

pub fn parse(text: &str, options: RParserOptions) -> Parse {
    let mut cache = NodeCache::default();
    parse_with_cache(text, options, &mut cache)
}

pub fn parse_with_cache(text: &str, options: RParserOptions, cache: &mut NodeCache) -> Parse {
    tracing::debug_span!("parse").in_scope(move || {
        let mut parser = RParser::new(text, options);
        crate::syntax::parse_root(&mut parser);
        let (events, trivia, diagnostics) = parser.finish();

        let mut tree_sink = RLosslessTreeSink::with_cache(text, &trivia, cache);
        biome_parser::event::process(&mut tree_sink, events, diagnostics);
        let (green, diagnostics) = tree_sink.finish();
        Parse::new(green, diagnostics)
    })
}
```

Note this now *uses* the diagnostics channel through `process()`, which air's
current code deliberately discards with a comment saying they "go into the
tree-sink but come right back out." That comment is correct for the tree-sitter
path; for us they are real.

Also add `parse_with_cache` — air's LSP will want it, and it costs nothing now.

### 9.4 Diagnostics

```rust
// crates/aether_r_parser/src/diagnostics.rs
use biome_parser::diagnostic::{expected_any, expected_node, ParseDiagnostic};
use biome_rowan::TextRange;

pub(crate) fn expected_expression(p: &RParser, range: TextRange) -> ParseDiagnostic {
    expected_node("expression", range, p)
}

/// The selector diagnostic names the operator it is complaining about, and points
/// a secondary label at that operator. `$`, `@`, `::` and `:::` all funnel through
/// here, and a message that lists all four tells the reader nothing about which one
/// they wrote.
///
/// `operator_range` must be captured BEFORE the operator is bumped.
pub(crate) fn expected_selector(
    p: &RParser,
    operator: RSyntaxKind,
    operator_range: TextRange,
    range: TextRange,
) -> ParseDiagnostic {
    // `RSyntaxKind::to_string()` yields the literal text for punctuation kinds,
    // so this stays correct if the token set changes.
    let operator = operator.to_string().unwrap_or("operator");
    expected_any(&["identifier", "string", "`...`", "`..i`"], range, p)
        .with_detail(operator_range, markup! {
            "`"{operator}"` requires a name on the right"
        })
}

/// The whole `(…)` group is missing — `function 1`.
pub(crate) fn expected_parameters(p: &RParser, range: TextRange) -> ParseDiagnostic { ... }
/// A hole in the parameter list — `function(a,,b)`, which R rejects even though
/// the same shape is legal in an argument list (§11.3).
pub(crate) fn expected_parameter(p: &RParser, range: TextRange) -> ParseDiagnostic { ... }
pub(crate) fn expected_argument(p: &RParser, range: TextRange) -> ParseDiagnostic { ... }
/// Two expressions with nothing between them: `1 2`. R separates statements by a
/// newline or a `;` and accepts nothing else. `previous` labels the expression
/// that already ended, so the message can say where the separator was expected.
///
/// The argument-list analogue (`f(a b)`) is a DIFFERENT rule enforced elsewhere,
/// by `expect(T![,])` in `parse_argument_list` (§11.3) — same shape of mistake,
/// different separator, different wording.
pub(crate) fn expected_statement_separator(
    p: &RParser,
    previous: TextRange,
    range: TextRange,
) -> ParseDiagnostic { ... }
pub(crate) fn expected_condition(p: &RParser, range: TextRange) -> ParseDiagnostic { ... }
```

`diagnostics.rs` already exists — Phase 4 landed the six the lexer raises
(`unterminated_backtick_identifier`, `unterminated_special`,
`unterminated_hexadecimal`, `unterminated_hexadecimal_binary_exponent`,
`unterminated_exponent`, `unexpected_character`). This phase adds the parser's
`expected_*` family alongside them and drops the `#[allow(dead_code)]` that
`lib.rs` carries until something drives them.

Every diagnostic constructor should be a named function here, never an inline
`err_builder` in a syntax module. It keeps wording consistent and makes it trivial
to review the full diagnostic surface in one file.

**An unterminated string gets no constructor of its own for now.** The three
unclosed-quote corpus tests surface through `expect(STRING_CLOSE)` in §11.1,
which is `ParseDiagnostic` enough to start. Add one only if Phase 7's snapshots
read badly — and if so, give it the two-range shape
`unterminated_backtick_identifier` already uses, pointing at the opening
delimiter with EOF as a secondary label, since a string runs to EOF for the same
reason a backticked name does (§8.4).

**Done when:** `parse("")` produces `R_ROOT` containing an empty
`R_STATEMENT_LIST` and an `EOF` token, `parse("   \n# hi\n")` attaches the
whitespace and comment as trivia on `EOF` with no diagnostics, and air's eight
`test_parse_trivia_*` unit tests are ported and passing against `RTokenSource`
(§9.1). Do not defer that last one to a later phase — every snapshot in the
project is downstream of it.

---

## 10. Phase 6 — Test harness and corpus import

The largest infrastructure phase, and the one that determines how pleasant the
next eight phases are. Build it carefully.

### 10.1 The markdown format

One **pair** of files per concept (§10.3.1), one test per `##` heading. Files are
fine-grained — one test in a file is fine.

**Two files, never one.** The input file is human territory and no tool ever writes
to it; the snapshot file is entirely machine-generated and no human ever edits it.
Keeping them separate means a reviewer can see at a glance which half of a diff was
authored and which half was derived, and it removes the awkwardness of a test
runner rewriting the file that defines the tests.

```
tests/corpus/functions/definition.md         ← human writes this
tests/corpus/functions/definition.snap.md    ← `xtask accept` writes this
```

The `.snap.md` double extension (rather than air's `calls.R.snap` convention) is
deliberate: GitHub still renders it as markdown, which is most of the point of
using markdown for snapshots at all. Mark them as derived so they collapse in
diffs and stay out of language stats:

```gitattributes
# .gitattributes
*.snap.md linguist-generated=true
```

#### The input file

````markdown
## Functions

```r
function() 1
function(arg1, arg2) {
  arg2
}
```

## Function No Body With Assignment [error]

```r
function() <- 1
```

## Dollar No RHS [pending]

```r
foo$
```
````

That is the whole input format: a flat sequence of `## Name [markers]` headings,
each followed by exactly one ` ```r ` cell. No title, no prose, nothing before the
first heading — the file path already says what the file is about, and provenance
lives in `MANIFEST.toml` rather than in a paragraph that goes stale.

#### The snapshot file

````markdown
<!-- Generated by `xtask accept`. Do not edit.
     Source: functions/definition.md -->

## Functions

```r
function() 1
function(arg1, arg2) {
  arg2
}
```

```cst
RRoot {
    bom_token: missing (optional),
    statements: RStatementList [
        RStatement {
            expression: RFunctionDefinition {
                name_token: FUNCTION_KW@0..8 "function" [] [],
                parameters: RParameters {
                    l_paren_token: L_PAREN@8..9 "(" [] [],
                    items: RParameterList [],
                    r_paren_token: R_PAREN@9..10 ")" [] [],
                },
                body: RDoubleValue {
                    value_token: R_DOUBLE_LITERAL@10..12 "1" [Whitespace(" ")] [],
                },
            },
            semicolon_token: missing (optional),
        },
    ],
    eof_token: EOF@...,
}
```

## Function No Body With Assignment [error]

```r
function() <- 1
```

```cst
...
```

```diagnostics
× Expected an expression but instead found `<-`.
  ...
```

## Dollar No RHS [pending]

*Not yet supported.*
````

The snapshot **re-emits the heading and the ` ```r ` cell** rather than referring
back to the input file. That redundancy is the feature: a `cst` block is
unreadable without the source right above it, and a reviewer approving a snapshot
diff should never have to open a second file to know what they are approving.

**Cell contract:**

| Cell | File | Contents |
|---|---|---|
| ` ```r ` | input (authored), snapshot (echoed) | the R source |
| ` ```cst ` | snapshot only | `format!("{:#?}", parse.tree())` — the *typed AST* view, which is what air's existing snapshots show and what makes tree shape reviewable |
| ` ```diagnostics ` | snapshot only | rendered `ParseDiagnostic`s; the cell is omitted entirely when there are none |

Emit the AST view (`parse.tree()`), not the raw `SyntaxNode` view. Air's current
snapshots show both; the AST view is far more readable and the raw view adds little
once the round-trip assertion is in place. If a specific test needs the raw view,
support an opt-in ` ```cst-raw ` cell.

**Markers** go in the heading, in `[brackets]` after the name, in the **input**
file. They are echoed into the snapshot so it reads standalone, but the input file
is the only place they are edited.

| Marker | Meaning |
|---|---|
| *(none)* | Must parse with **zero** diagnostics. Fails if any diagnostic is emitted. |
| `[pending]` | Feature not implemented yet. Harness asserts it does **not** parse cleanly. **An unexpected clean parse is a hard failure.** The snapshot carries only `*Not yet supported.*` in place of the generated cells. Round-trip still applies (§10.2) — a pending test may fail to parse, but must never lose a token. |
| `[error]` | Must produce at least one error diagnostic. Fails if it parses cleanly. |
| `[deviation]` | Excluded from the tree-sitter oracle comparison. Orthogonal to the others — combine as `[error, deviation]`. |

**The pairing invariant.** The snapshot's heading sequence must match the input's
exactly — same names, same order, same markers. Check it explicitly rather than
letting a mismatch surface as a confusing content diff:

```rust
assert_eq!(
    input.tests.iter().map(|t| (&t.name, t.markers)).collect::<Vec<_>>(),
    snapshot.tests.iter().map(|t| (&t.name, t.markers)).collect::<Vec<_>>(),
    "{} and its snapshot have drifted apart — run `just accept`", path.display(),
);
```

This is what makes the split safe: adding a test to the input file fails loudly
until the snapshot is regenerated, rather than being silently untested.

### 10.2 Harness implementation

```rust
// crates/aether_r_parser_tests/src/document.rs
#[derive(Debug)]
pub struct Document {
    pub tests: Vec<TestCase>,
}

#[derive(Debug)]
pub struct TestCase {
    pub name: String,
    pub markers: Markers,
    /// The ```r cell. Authored in the input file, echoed into the snapshot.
    pub source: String,
    /// ```cst / ```diagnostics. Empty in an input document, always populated
    /// (or `Pending`) in a snapshot document.
    pub generated: Vec<Cell>,
}

bitflags::bitflags! {
    #[derive(Debug, Clone, Copy, PartialEq, Eq)]
    pub struct Markers: u8 {
        const PENDING   = 1 << 0;
        const ERROR     = 1 << 1;
        const DEVIATION = 1 << 2;
    }
}

impl Document {
    /// Parses either flavour; `generated` is simply empty for input files.
    pub fn parse(text: &str) -> Result<Document, DocumentError> { /* … */ }
    /// Renders the snapshot flavour: the banner, then per test its heading, its
    /// ```r cell, and its generated cells.
    pub fn render_snapshot(&self, source_path: &str) -> String { /* … */ }
}
```

Write the markdown parser by hand — it is ~150 lines of line-oriented state machine
(`## `, ` ```lang `, ` ``` `, and the snapshot banner). Anything before the first
`## ` in an input file is an error, not prose to be preserved. Do **not** pull in a
markdown crate; you need
byte-exact round-tripping of headings and cells, which general markdown parsers do
not give you.

`Document`, `check`, and `run_file` live in `crates/aether_r_parser_tests/src/`,
together with the corpus itself in that crate's `tests/`. The crate depends on
`aether_r_parser`; nothing depends on it except `xtask`, so the graph stays
acyclic and `aether_r_parser` ships to air with no test-only dependencies.

The harness is library code rather than living in `tests/`, because `xtask
accept` needs `Document` and `check` too and cannot reach into another crate's
`tests/` directory.

**This harness is for the parser only.** The lexer keeps its own `#[cfg(test)]`
suite inside `aether_r_parser` (§8.6): those tests read private fields, and a
token stream is small enough to assert exactly rather than snapshot. There is no
lexer corpus and `xtask accept` never looks at `src/`.

**Tests never write to disk.** nextest runs each test in its own process, in
parallel, so anything in the test path that writes files is a latent race. The
input/snapshot split makes this natural rather than a rule to remember: tests
*compare*, `xtask accept` *writes*, and the two never touch the same file. There is
no `AETHER_UPDATE_SNAPSHOTS` env var.

```rust
// crates/aether_r_parser_tests/src/runner.rs
//
// READ-ONLY. Never writes. Regeneration lives in `xtask accept`.
//
// Panics on mismatch rather than returning an error: it is called from a
// `#[test]`, and a panic is what nextest reports.
pub fn run_file(input_path: &Path) {
    let snapshot_path = input_path.with_extension("snap.md");

    let input = Document::parse(&fs::read_to_string(input_path).unwrap()).unwrap();
    let committed = fs::read_to_string(&snapshot_path).unwrap_or_else(|_| {
        panic!("{} has no snapshot. Run `just accept`.", input_path.display())
    });

    // 1. Run every test. Marker violations fail here, before any diffing, so a
    //    genuine parser bug is not reported as a confusing snapshot mismatch.
    let mut generated = input.clone();
    let mut failures = Vec::new();
    for test in &mut generated.tests {
        match check(test) {
            Ok(cells) => test.generated = cells,
            Err(e) => failures.push((test.name.clone(), e)),
        }
    }
    if !failures.is_empty() { panic!("{}", render_failures(&failures)); }

    // 2. Compare against the committed snapshot.
    let rendered = generated.render_snapshot(&relative(input_path));
    if rendered != committed {
        panic!(
            "{}\n\n{} is out of date. Run `just accept`.",
            similar::TextDiff::from_lines(&committed, &rendered)
                .unified_diff().header("committed", "generated"),
            snapshot_path.display(),
        );
    }
}
```

```rust
// xtask/src/accept.rs
//
// Single process, sequential. The ONLY thing that writes snapshot files, and it
// never writes an input file.
pub fn accept() -> Result<()> {
    for input_path in corpus_input_files()? {          // *.md, excluding *.snap.md
        let mut doc = Document::parse(&fs::read_to_string(&input_path)?)?;
        for test in &mut doc.tests {
            // On failure, still write what we can and report at the end — a
            // regeneration run should be maximally informative.
            test.generated = check(test).unwrap_or_default();
        }
        let snapshot_path = input_path.with_extension("snap.md");
        let rendered = doc.render_snapshot(&relative(&input_path));
        if fs::read_to_string(&snapshot_path).ok().as_deref() != Some(&rendered) {
            fs::write(&snapshot_path, rendered)?;
            println!("updated {}", snapshot_path.display());
        }
    }
    // An input file that was deleted leaves an orphan snapshot behind.
    remove_orphan_snapshots()?;
    Ok(())
}
```

```rust
fn check(test: &TestCase) -> Result<Vec<Cell>, CheckError> {
    let parsed = aether_r_parser::parse(&test.source, RParserOptions::default());

    // 1. Round-trip. Non-negotiable, EVERY test — including `[error]` tests and
    //    including `[pending]` ones. Pending inputs exercise the recovery paths
    //    hardest, so they are where dropped tokens are most likely; a parser that
    //    cannot yet parse a construct must still reproduce it verbatim. This is
    //    invariant 1 in `CLAUDE.md`, and "for ALL inputs" is meant literally.
    let round_tripped = parsed.syntax().to_string();
    if round_tripped != test.source {
        return Err(CheckError::RoundTrip { expected: test.source.clone(), got: round_tripped });
    }

    let has_error = parsed.has_error();
    let has_bogus = has_bogus_nodes_or_empty_slots(&parsed.syntax());
    let clean = !has_error && !has_bogus;

    // 2. Marker contract
    if test.markers.contains(Markers::PENDING) {
        if clean {
            return Err(CheckError::UnexpectedlyPasses);   // ← flip the marker off!
        }
        return Ok(vec![Cell::pending()]);   // renders as *Not yet supported.*
    }
    if test.markers.contains(Markers::ERROR) && !has_error {
        return Err(CheckError::ExpectedError);
    }
    if !test.markers.contains(Markers::ERROR) && has_error {
        return Err(CheckError::UnexpectedError(render_diagnostics(&parsed, &test.source)));
    }

    // 3. Structural sanity for clean tests
    if !test.markers.contains(Markers::ERROR) {
        let ast = format!("{:#?}", parsed.tree());
        if ast.contains("missing (required)") {
            return Err(CheckError::MissingRequired(ast));
        }
        if has_bogus {
            return Err(CheckError::BogusNodes(ast));
        }
    }

    // 4. Every bogus node is explained by a diagnostic that POINTS AT IT.
    //    Invariant 2 in `CLAUDE.md`. A recovery path that produces a bogus node
    //    without calling `p.error(...)` yields a tree that round-trips and
    //    satisfies its `[error]` marker via some *unrelated* diagnostic — which
    //    is why the test has to be per-node overlap and not "are there any
    //    diagnostics at all". The whole-parse form would be satisfied by the
    //    very diagnostic that masks the problem, and is in fact unreachable:
    //    step 3 has already rejected bogus nodes on every non-`[error]` test,
    //    and an `[error]` test always has a non-empty diagnostic list.
    //
    //    So in practice this only bites on `[error]` tests, which is exactly
    //    where it is needed. It matters most in phases 7-13 while recovery is
    //    being written — not in phase 14, by which point the bad snapshots are
    //    already committed.
    //
    //    Sits after the `[pending]` early return above: a pending test may
    //    legitimately produce undiagnosed bogus nodes before its feature exists.
    let unexplained = undiagnosed_bogus_nodes(&parsed.syntax(), parsed.diagnostics());
    if !unexplained.is_empty() {
        return Err(CheckError::UndiagnosedBogusNode {
            ranges: unexplained.iter().map(|n| n.text_range()).collect(),
            ast: format!("{:#?}", parsed.tree()),
        });
    }

    // 5. Oracle — compiled out unless the `oracle` feature is on, so a default
    //    `cargo nextest run` (including CI) never builds tree-sitter-r.
    #[cfg(feature = "oracle")]
    if !test.markers.contains(Markers::DEVIATION) {
        match aether_r_oracle::compare(&test.source, &parsed.syntax()) {
            OracleResult::Match => {}
            // tree-sitter bails on any error input, so this is a skip, not a
            // failure — see §10.4.
            OracleResult::OracleUnavailable(_) => {}
            OracleResult::Mismatch { path, expected, actual } => {
                return Err(CheckError::OracleMismatch { path, expected, actual });
            }
        }
    }

    // 6. Generate cells
    let mut cells = vec![Cell::new("cst", format!("{:#?}", parsed.tree()))];
    if !parsed.diagnostics().is_empty() {
        cells.push(Cell::new("diagnostics", render_diagnostics(&parsed, &test.source)));
    }
    Ok(cells)
}
```

`has_bogus_nodes_or_empty_slots` is copied verbatim from air's
`crates/air_r_parser/tests/spec_test.rs`. **Check what it actually covers before
relying on the split above.** Step 3 greps the AST dump for `missing (required)`
*in addition to* calling it, which suggests air's helper may not inspect empty
slots the way its name implies. If it does, the string-grep is redundant and
should go; if it does not, both checks stay and the name is misleading.

The step 4 helper:

```rust
/// Bogus nodes that no diagnostic points at.
///
/// Overlap, not equality: a recovery that consumes three tokens may legitimately
/// be explained by a diagnostic anchored on only the first of them.
fn undiagnosed_bogus_nodes(
    root: &RSyntaxNode,
    diagnostics: &[ParseDiagnostic],
) -> Vec<RSyntaxNode> {
    let spans: Vec<TextRange> = diagnostics
        .iter()
        .filter_map(|d| d.location().span)
        .collect();

    root.descendants()
        .filter(|node| node.kind().is_bogus())
        .filter(|node| {
            let range = node.text_range();
            !spans.iter().any(|span| span.intersect(range).is_some())
        })
        .collect()
}
```

Three things to know about it:

- **Empty slots are deliberately out of scope.** A missing required slot is not a
  node and has no range to overlap, so it cannot participate in this check. Step 3
  rejects it on every clean test, and the designed error paths always emit a
  diagnostic alongside it — `a$` produces a missing `right` slot *and*
  `expected_selector` (§11.4). Nothing is being let through here that step 3 or the
  `[error]` marker does not already cover.
- **Only primary ranges count.** `with_detail` labels live in the diagnostic's
  advices, not in `location()`, so `expected_selector`'s secondary label on the
  operator does not contribute to overlap — only its primary range does. That is
  the conservative reading and it is intentional; do not "fix" it by walking
  advices, or a diagnostic that merely gestures at a bogus node will start counting
  as an explanation of it.
- **`d.location().span` is the one API call here to verify at the pinned Biome
  rev** rather than trust. `ParseDiagnostic` implements `Diagnostic`, so
  `location()` should yield a `Location` carrying `span: Option<TextRange>`, but
  confirm the shape before writing the rest.

Diagnostic rendering:

```rust
fn render_diagnostics(parse: &Parse, source: &str) -> String {
    use biome_console::{fmt::{Formatter, Termcolor}, markup};
    use biome_diagnostics::{termcolor::NoColor, DiagnosticExt, PrintDiagnostic};
    let mut out = Vec::new();
    for diagnostic in parse.diagnostics() {
        let d = diagnostic.clone().with_file_path("test.R").with_file_source_code(source);
        Formatter::new(&mut Termcolor(NoColor::new(&mut out)))
            .write_markup(markup! { {PrintDiagnostic::verbose(&d)} })
            .unwrap();
    }
    String::from_utf8(out).unwrap()
}
```

Test registration — one `#[test]` per corpus **input** file, generated by globbing
the tree, so adding a file needs no registration edit. This needs a small
proc-macro crate, `crates/aether_r_test_macros/` (`proc-macro = true`,
`proc-macro2` + `quote`), built as part of Phase 6. Model it on air's
`tests_macros::gen_tests!`, which already does this for `.R` files. Note the glob
must exclude `*.snap.md`, or every snapshot gets its own bogus test:

```rust
// crates/aether_r_parser_tests/tests/main.rs
//
// The whole test target: no harness module, since `run_file` is library code in
// this same crate (§3).
mod corpus {
    // Expands to one `#[test] fn` per matched file, with the module path
    // mirroring the directory, e.g. `corpus::expressions::extract`.
    // The macro skips any path ending in `.snap.md`.
    aether_r_test_macros::gen_corpus_tests! {
        "tests/corpus/**/*.md",
        crate::run_file
    }
}
```

That naming makes nextest filters read naturally:

```bash
cargo nextest run -E 'test(corpus::expressions::extract)'
cargo nextest run -E 'test(/corpus::control/)'          # all control-flow files
cargo nextest run -p aether_r_parser_tests --status-level all # pending count
```

Per-file is the right granularity to start. Because the tests are read-only,
upgrading to one `#[test]` per `##` heading later is safe — extend the macro to
parse headings at expansion time.

### 10.3 Importing the tree-sitter corpus

Write `xtask/src/import_corpus.rs`, run it **once**, commit the output, then delete
or ignore the tool. Do not keep a live dependency on tree-sitter-r's test files.

The tree-sitter corpus format is:

```
================================================================================
Test Name
================================================================================
<R source>
--------------------------------------------------------------------------------

(program
  (function_definition ...))
```

with an optional attribute line (`:error`, `:skip`) between the name and the second
`===` fence. There are exactly **two** such attribute lines, both in
`expressions-errors.txt`.

The importer:
1. Splits on `={10,}` fences to get `(name, attributes, body)`.
2. Splits body on the `-{10,}` fence; keeps **only the R source** and discards the
   s-expression. We regenerate our own expectations; mechanically translating
   tree-sitter s-expressions into Rowan AST dumps would be both hard and pointless.
3. Emits `## {name} [pending]` + an ` ```r ` cell into the **input** file given by
   the mapping table below. It does not write snapshots — run
   `cargo run -p xtask -- accept` afterwards to generate every `.snap.md`.
4. Adds `[error]` for anything that came from a `*-errors.txt` file.
5. Writes `MANIFEST.toml` (§10.3.3).

#### 10.3.1 One file per concept

tree-sitter-r has only four corpus files because its test runner needs it that
way. That is an artifact of its infrastructure, not a meaningful grouping, and it
is the reason `expressions.txt` is a 49-test grab bag. **Aether splits by concept
instead**, roughly mirroring `src/syntax/`, and files may be as small as one test.

Two consequences worth stating plainly:

- **The `-errors` split disappears.** Error expectations are carried by the
  `[error]` marker on each test, not by which file it lives in, so
  `Not A Parenthesized Expression 1` sits directly beside `Parenthesized Expression`
  in `expressions/parenthesized.md`. That is strictly better for review: you see the
  accepted and rejected forms of a construct together.
- **Turning on a feature becomes one file, usually.** Phase 11 (functions) flips
  the markers in `functions/*.md` and nothing else. The phase-to-file correspondence
  in the mapping below is deliberate.

#### 10.3.2 The complete mapping — all 89 tests

`ts:e` = `expressions.txt`, `ts:ee` = `expressions-errors.txt`,
`ts:l` = `literals.txt`, `ts:le` = `literals-errors.txt`.
Every one of the 89 appears exactly once. **Do not drop or merge any of them, and
do not split a single tree-sitter test's body across two files** — an individual
test often bundles a dozen related snippets and its value is in that grouping.

| Destination | Tests | Source | Phase |
|---|---|---|---|
| `literals/identifiers.md` | Identifiers · Unicode Identifiers · Invalid Identifiers · Dot Dot I · **Unclosed Backtick** `[error]` | ts:l ×4, ts:le ×1 | 7 |
| `literals/numbers.md` | Integers · Floats · Hexadecimal · Scientific Notation Floats · Complex | ts:l ×5 | 7 |
| `literals/strings.md` | Strings · Strings Without Content · Strings With Escape Sequences · Raw Strings · **Unclosed Double Quote** · **Unclosed Single Quote** · **Unclosed Raw String** `[error]` | ts:l ×4, ts:le ×3 | 7 |
| `literals/comments.md` | Comments | ts:l ×1 | 7 |
| `literals/constants.md` | Constants | ts:l ×1 | 7 |
| `expressions/unary.md` | Unary Operators | ts:e ×1 | 8 |
| `expressions/binary.md` | Relational · Arithmetic Operator · Not An Arithmetic Operator | ts:e ×3 | 8 |
| `expressions/colon.md` | Colon | ts:e ×1 | 8 |
| `expressions/specials.md` | Specials `[deviation]` · Pipe · Pipe Placeholder | ts:e ×3 | 8 |
| `expressions/formula.md` | Formulas | ts:e ×1 | 8 |
| `expressions/help.md` | Help Operator | ts:e ×1 | 8 |
| `expressions/assignment.md` | Assignment | ts:e ×1 | 8 |
| `expressions/precedence.md` | Operator precedence · Precedence Tests · Complex Expressions · Subset and Subset2 Precedence | ts:e ×4 | 8, last one 9 |
| `expressions/call.md` | Calls · Calls With NULL As Argument Name · Calls With `return` As Argument Name · `return` Is Just An Identifier · Switch · **Calls With NA As Argument Name** · **Not A Call, Subset, Or Subset2** · **… Due To Sequential Arguments** `[error]` | ts:e ×5, ts:ee ×3 | 9 |
| `expressions/subset.md` | Subset | ts:e ×1 | 9 |
| `expressions/subset2.md` | Subset2 · **Unsupported Subset2 With `]]` Over Multiple Lines (#177)** `[error]` | ts:e ×1, ts:ee ×1 | 9 |
| `expressions/parenthesized.md` | Parenthesized Expression · **Not A Parenthesized Expression 1** · **2** · **3** `[error]` | ts:e ×1, ts:ee ×3 | 9 |
| `expressions/extract.md` | Dollar · Slot · Dollar, At, Namespace, Namespace Internal With Expression RHS · **Dollar No RHS** · **Slot No RHS** `[error, deviation]` · **… With `if` RHS** · **… With Newline Before Operator** `[error]` | ts:e ×5, ts:ee ×2 | 10 |
| `expressions/namespace.md` | Namespace get · Namespace get internal · **Namespace With Missing RHS** `[error, deviation]` | ts:e ×3 | 10 |
| `functions/definition.md` | Functions · Function No Body · **Function No Body With Assignment** · **Function No Body Inside Another Function** `[error]` | ts:e ×2, ts:ee ×2 | 11 |
| `functions/lambda.md` | Lambda Function | ts:e ×1 | 11 |
| `functions/parameters.md` | Functions With `return` As Parameter Name · Function Dots And Dot Dot I | ts:e ×2 | 11 |
| `control/if.md` | If · If-else · Complex If Expression · Identifiers starting with `else` that are not the `else` keyword (#200) · … - 2 (#200) | ts:e ×5 | 12 |
| `control/for.md` | For · For No Body | ts:e ×2 | 12 |
| `control/while.md` | While · While No Body | ts:e ×2 | 12 |
| `control/repeat.md` | Repeat · Repeat No Body | ts:e ×2 | 12 |
| `statements/braces.md` | Braced Expression | ts:e ×1 | 13 |
| `statements/newlines.md` | Newlines | ts:e ×1 | 8 |
| `statements/semicolons.md` | *(none from tree-sitter — air's semicolon snapshots, all `[deviation]`)* | — | 13 |
| `recovery/unmatched-delimiters.md` | **Closing Brace** · **Closing Parenthesis** · **Closing Bracket** · **Opening Brace, Closing Parenthesis** · **Opening Parenthesis, Closing Brace** · **Opening Parenthesis, Closing Bracket** · **Opening Bracket2, Unmatched Closing Bracket** · **Opening Bracket and Bracket2, Unmatched Closing Bracket** · **Opening Bracket2 and Bracket, Matched Closing Bracket** · **Opening Bracket2 and Bracket, Unmatched Closing Bracket** `[error]` | ts:ee ×10 | 9, finalised 14 |

Bold = imported from an `-errors.txt` file, so it carries `[error]`.

**Tally:** literals 19 (= 15 + 4), expressions 40, functions 7, control 11,
statements 2, recovery 10. Expression-family total 70 (= 49 + 21). **Grand total
89.** Reproduce this arithmetic after importing; if it does not come out to 89 you
have dropped something.

Three placements are judgement calls, noted so a reviewer is not surprised:
- **`Subset and Subset2 Precedence`** goes in `precedence.md` rather than
  `subset2.md`, since it is about binding order.
- **The two `#200` `else`-prefix identifier tests** go in `control/if.md`, not
  `literals/identifiers.md`. They are lexer tests in substance, but their R source
  is an `if`/`else` block and they only fail in that context.
- **`Dollar, At, Namespace, Namespace Internal With …`** (three tests) exercise all
  four operators at once. They live in `extract.md`, so `namespace.md` looks thinner
  than it is. `MANIFEST.toml` records where every test landed, which is where to
  look rather than guessing from file names.

#### 10.3.3 `MANIFEST.toml` — the anti-drop guard

The importer also writes a manifest, and a test asserts against it. This is what
makes "do not drop tests" mechanical rather than aspirational.

Every imported test is listed, whatever it was imported *from* — the 89 from
tree-sitter-r and air's ~35 snapshots (§10.3.4) alike. `source` carries the
provenance, so no naming convention inside the `.md` files has to.

```toml
# crates/aether_r_parser_tests/tests/corpus/MANIFEST.toml
# Generated once by `xtask import-corpus`. Every test imported from another
# project, and where it now lives. Do not hand-edit.
[[imported]]
source = "tree-sitter-r/test/corpus/expressions.txt"
name   = "Dollar No RHS"
file   = "expressions/extract.md"

[[imported]]
source = "tree-sitter-r/test/corpus/expressions-errors.txt"
name   = "Closing Brace"
file   = "recovery/unmatched-delimiters.md"

[[imported]]
source = "air/crates/air_r_parser/tests/snapshots/ok/calls.R"
name   = "Calls"
file   = "expressions/call.md"
# … 89 tree-sitter entries + air's snapshots
```

```rust
const TREE_SITTER: &str = "tree-sitter-r/";

#[test]
fn every_imported_test_still_exists() {
    let manifest = Manifest::load();
    let from_tree_sitter =
        manifest.imported.iter().filter(|e| e.source.starts_with(TREE_SITTER)).count();
    assert_eq!(from_tree_sitter, 89, "manifest lost tree-sitter entries");

    for entry in &manifest.imported {
        let doc = Document::parse(&fs::read_to_string(corpus_dir().join(&entry.file))?)?;
        assert!(
            doc.tests.iter().any(|t| t.name == entry.name),
            "test {:?} was imported from {} into {} but is no longer there",
            entry.name, entry.source, entry.file,
        );
    }
}

#[test]
fn no_imported_test_is_duplicated() {
    // Each imported name appears in exactly one corpus file, and it is the file the
    // manifest records. Copying a test into a second file forks it silently
    // otherwise — one copy then drifts while the manifest check stays green.
    // …
}
```

Renaming an imported test or moving it between files is then a deliberate two-file
change: the `.md` and the manifest. That is the point.

Tests aether writes itself in later phases are simply absent from the manifest and
need no bookkeeping — they are new, so there is no prior version of them to lose.

#### 10.3.4 Air's own snapshots

Air's `crates/air_r_parser/tests/snapshots/{ok,error,undefined}/**/*.R` (~35 files)
merge into the **same concept files**, not a separate `air-regressions.md` — a
regression test about calls belongs next to the other call tests. Suggested
placements:

| Air snapshot | Destination |
|---|---|
| `ok/binary_expressions.R`, `ok/unary_expressions.R` | `expressions/binary.md`, `expressions/unary.md` |
| `ok/calls.R`, `error/call/side_by_side_arguments.R` | `expressions/call.md` |
| `ok/subset.R`, `ok/subset2.R` | `expressions/subset.md`, `expressions/subset2.md` |
| `ok/extract_expression.R`, `undefined/extract_expression_error.R` | `expressions/extract.md` |
| `ok/namespace_expression.R`, `undefined/namespace_expression_error.R`, `error/namespace_expression/*.R` | `expressions/namespace.md` |
| `ok/parenthesized_expression.R`, `error/parenthesized_expression/*.R` | `expressions/parenthesized.md` |
| `ok/function_definition.R` | `functions/definition.md` |
| `ok/if_statement.R`, `ok/for_statement.R`, `ok/while_statement.R`, `ok/repeat_statement.R` | `control/*.md` |
| `ok/braced_expressions.R` | `statements/braces.md` |
| `ok/semicolons/*.R` | `statements/semicolons.md` |
| `ok/comments.R`, `ok/crlf/*.R` | `literals/comments.md` |
| `ok/value/*.R` | `literals/numbers.md`, `literals/strings.md` |
| `ok/dots.R`, `ok/dot_dot_i.R`, `ok/keyword.R` | `literals/identifiers.md`, `literals/constants.md` |

**A merged snapshot may lag its file's nominal phase.** Air's files were written
to exercise a feature, not to stay within one, so `ok/comments.R` is
`1 + 1 # Trailing` and `ok/dots.R` contains `{ ... }` and `...()`. Landing them in
`literals/` is still right — a comment test belongs with the other comment tests —
but it means the phase that owns a *directory* does not necessarily turn every
heading in it green. Leave the stragglers `[pending]` and let the phase that
implements what they need flip them. The per-phase counts in §11 are of imported
tree-sitter tests for this reason.

These headings get no marker of where they came from — no `air:` prefix, nothing.
A test's name should describe the R it exercises, and a reader scanning
`expressions/call.md` has no reason to care which upstream project a given case
arrived from. Provenance is recorded once, in `MANIFEST.toml` (§10.3.3), where it
is queryable and cannot rot. The importer writes these entries too, with an
`air/`-rooted `source`.

**Names must be unique within a file, and dropping the prefix is what makes that
bite.** Air's `ok/calls.R` lands in `expressions/call.md` next to tree-sitter's
`Calls`, and the obvious name for it is also `Calls`. Since the harness and both
manifest checks address tests by `(file, name)`, a collision is not a cosmetic
problem — it makes `every_imported_test_still_exists` satisfiable by the wrong test.
So:

- `Document::parse` rejects a duplicate heading within a file outright, with the
  offending name in the message. Cheap, and it turns the whole class of problem into
  a parse error at the point of authoring.

- Where a name is taken, the importer disambiguates by what the test actually
  covers, not by its origin — `Calls With Trailing Comma`, not `Calls (air)`. There
  are only a handful; name them by hand rather than auto-suffixing `_2`.

The `undefined/*` ones become `[error]` in aether — that directory exists in air
precisely because nobody was happy with the current silent behavior (deviation 2 in
`docs/deviations.md`).

### 10.4 The oracle

```rust
// crates/aether_r_oracle/src/lib.rs
//! Differential oracle: parse with tree-sitter-r + air's tree-sitter→Rowan
//! walker, and compare the resulting Rowan tree against aether's.
//!
//! `treesitter.rs` and `walk.rs` are vendored VERBATIM from
//! posit-dev/air `crates/air_r_parser/src/{treesitter.rs,parse.rs}` at rev <SHA>.
//! Do not "improve" them — their whole value is being an independent
//! implementation. Re-vendor if air's copy changes.

pub enum OracleResult {
    Match,
    Mismatch { path: String, expected: String, actual: String },
    /// tree-sitter itself failed; comparison is meaningless.
    OracleUnavailable(String),
}

pub fn compare(source: &str, actual: &RSyntaxNode) -> OracleResult {
    let expected = match parse_with_treesitter(source) {
        Ok(tree) => tree,
        Err(e) => return OracleResult::OracleUnavailable(e),
    };
    compare_nodes(&expected, actual, &mut String::from("RRoot"))
}
```

Compare **kind, text range, and slot structure**, walking both trees in parallel
by slot index (`node.slots()`), so the failure message can name the exact path:

```
mismatch at RRoot > RStatementList > [0] > RBinaryExpression > right
  expected: R_DOUBLE_VALUE @ 4..5
  actual:   R_BOGUS_EXPRESSION @ 4..5
```

**Handling the deviations.** The oracle cannot compare trees that intentionally
differ. Two mechanisms:

1. `[deviation]` marker on the test — skip entirely. Use for semicolon tests and
   extract/namespace-missing-RHS tests.
2. A **normalization pass** applied to *both* trees before comparison, for
   deviations that are mechanical. Specifically, for semicolons: unwrap
   `R_STATEMENT` nodes, drop `SEMICOLON` tokens, and treat aether's
   `R_STATEMENT_LIST` as equivalent to tree-sitter's `R_EXPRESSION_LIST` before
   comparing. This is worth building — it lets ~85 of the 89 tests keep
   oracle coverage instead of blanket-skipping everything containing a `;`.

**Where tree-sitter bails.** `parse_from_treesitter` returns a whole-file bogus
node whenever `root_node().has_error()`. So for every `[error]` test the oracle is
useless. That is fine and expected: mark them `[deviation]` or have `compare`
return `OracleUnavailable` when tree-sitter reports an error, and treat that as a
skip rather than a failure. Error-recovery behavior is exactly where aether is
*supposed* to differ.

#### 10.4.1 Scope: local only, and temporary

**Settled with the owner: the oracle does not run in CI, and is deleted once the
conversion is complete.** It is a development aid for catching drift while the
parser is being written — most valuably trivia attachment (§13) — not a permanent
correctness gate.

Two things follow.

**Feature-gate it.** The oracle is invoked from `check()`, which every corpus
test calls, so without a gate `cargo nextest run --workspace` would build
tree-sitter-r and its C toolchain on every run including CI.

One feature, on the crate whose code carries the `#[cfg]`. In
`aether_r_parser_tests/Cargo.toml`:

```toml
[features]
default = []
oracle  = ["dep:aether_r_oracle"]

[dependencies]
aether_r_parser = { workspace = true }
aether_r_oracle = { workspace = true, optional = true }
```

Note `aether_r_oracle` is a plain dependency, not a dev-dependency: `check()` is
library code in `src/`, and dev-dependencies are not in scope for it. Nothing
forwards this feature, and `aether_r_parser` never sees it — which is the point
of the harness living in its own crate (§3).

`just test` and CI run without it; `just oracle` runs with it, and the phase
checklist (§4) calls for that at the end of every parser phase.

**Do not defend it.** Because it is scheduled for deletion, the oracle does not
earn much maintenance. If a Biome rev bump or a tree-sitter-r change breaks it,
the reasonable responses are to fix it cheaply or to stop using it — not to spend
a day restoring a tool whose remaining life is a handful of phases. What must
survive its removal is the *reviewed snapshots*, which is why §10.1 requires
eyeballing every changed cell rather than trusting the oracle to catch everything.

The counterpart risk — that a local-only tool quietly stops being run — is real
and is why the phase checklist names it as an explicit step rather than leaving it
to habit.

### 10.5 `quick_test.rs`

Keep air's scratchpad, since the owner uses it:

Run it with `just quick` (nextest needs `--run-ignored all` and `--no-capture`
to show `println!` output):

```rust
// crates/aether_r_parser_tests/tests/quick_test.rs
#[ignore]
#[test]
fn quick_test() {
    let code = r#"
    1$foo
    1$2
    "#;
    let parsed = aether_r_parser::parse(code, RParserOptions::default());
    println!("{:#?}", parsed.tree());
    for d in parsed.diagnostics() { println!("{d:?}"); }
}
```

**Done when:** all 89 tree-sitter tests plus air's ~35 snapshots exist as
`[pending]`, distributed across the concept input files per §10.3.2, each with a
generated `.snap.md` sibling;
`MANIFEST.toml` lists every one of them, of which exactly 89 have a
`tree-sitter-r/` source, and `every_imported_test_still_exists` passes;
`cargo nextest run --workspace` is green — pending tests assert only that the
parse is *not* clean, which everything satisfies at this point, **and** that the
source round-trips, which requires the lexer to already tokenise every corpus
input losslessly (§8). That second half is the real bar for closing this phase;
if a corpus file does not round-trip, the lexer is dropping bytes and no amount
of later parser work will fix it. Finally, `cargo run -p xtask -- accept`
produces no diff.

---

## 11. Phases 7–13 — The parser, one feature at a time

From here on each phase is small enough to review in one sitting. The shape is
always the same: add syntax functions, add diagnostics, flip `[pending]` markers
off the tests the feature enables, regenerate, review the diff.

### 11.1 Phase 7 — Literals, identifiers, program

`src/syntax/value.rs`, `src/syntax.rs`.

```rust
pub(crate) fn parse_root(p: &mut RParser) {
    let m = p.start();
    p.eat(UNICODE_BOM);
    // No `with_flags` here: empty flags *are* top-level behaviour — newlines are
    // significant and `else` may not follow one. See §9.2 on polarity.
    parse_statement_list(p, StatementListEnd::Eof);
    p.expect(T![EOF]);
    m.complete(p, R_ROOT);
}
```

**Phase 7 writes the first `parse_statement_list`, not Phase 13.** §11.7 shows the
finished version, but `parse_root` calls it from this phase onward, so a working
version has to exist now. What Phase 13 *adds* is the `;` slot and the
empty-statement handling; what Phase 7 must already get right is the loop, the
recovery branch, and — importantly — the **statement separator rule**, since R
separates statements by a newline or a `;` and by nothing else. Deferring that to
Phase 13 would let `1 2` parse clean for six phases, including Phase 8, where
`statements/newlines.md` is supposed to go green. Write it here against
§11.7's body with the semicolon lines omitted.

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

### 11.2 Phase 8 — Binary and unary operators (the Pratt core)

`src/syntax/expr.rs`. This is the heart of the parser. Take the time to get it
right; everything else hangs off it.

#### The precedence table

Transcribed from `tree-sitter-r/grammar.js`'s `PREC`, which is itself transcribed
from R's `src/main/gram.y`. Higher binds tighter.

| Rank | Name | Operators | Assoc |
|---|---|---|---|
| 1 | HELP | `?` (binary) | left |
| 2 | FUNCTION_OR_LOOP | `function`, `while`, `for`, `repeat` bodies | (not an operator — see below) |
| 3 | IF | `if` | (not an operator — see below) |
| 4 | LEFT_ASSIGN | `<-` `<<-` `:=` | **right** |
| 5 | EQUALS_ASSIGN | `=` | **right** |
| 6 | RIGHT_ASSIGN | `->` `->>` | left |
| 7 | TILDE | `~` | left |
| 8 | OR | `\|` `\|\|` | left |
| 9 | AND | `&` `&&` | left |
| 10 | UNARY_NOT | `!` (unary) | left |
| 11 | COMPARISON | `<` `<=` `>` `>=` `==` `!=` | left (nonassoc in R) |
| 12 | PLUS_MINUS | `+` `-` (binary) | left |
| 13 | MULTIPLY_DIVIDE | `*` `/` | left |
| 14 | SPECIAL_OR_PIPE | `%...%` `\|>` | left |
| 15 | COLON | `:` | left |
| 16 | UNARY_PLUS_MINUS | `+` `-` (unary) | left |
| 17 | EXPONENTIATE | `^` `**` | **right** |
| 18 | EXTRACT | `$` `@` | left (see below) |
| 19 | NAMESPACE | `::` `:::` | left (see below) |
| 20 | CALL | `f(…)` `x[…]` `x[[…]]` | — (postfix) |

**Ranks 2, 3, and 20 are deliberately absent from the binding-power functions
below.** Ranks 2 and 3 are not operator precedences: in tree-sitter-r they wrap
entire productions —

```js
function_definition: $ => withPrec(PREC.FUNCTION_OR_LOOP, seq(
if_statement:        $ => withPrec(PREC.IF, seq(
```

— and their only job is to make a body greedy, i.e. to say how far right
`function(x) …` or `if (c) …` extends. A recursive-descent parser answers that by
construction: the body is a plain `parse_expression` call that runs to its natural
terminator (§11.5, §11.6). The two ranks are kept in the table so the numbering
lines up with tree-sitter-r's `PREC` when comparing, and for nothing else.

Rank 20 is absent for a different reason: postfix is handled structurally in the
Pratt loop and applies unconditionally, so it is never compared against a
`min_bp`. The number survives in code only as `POSTFIX_BINDING_POWER`, which
exists purely to give the `debug_assert!` in `parse_expression_with_lhs` something
to assert *against* — that 20 really is the ceiling. Adding an infix operator at
rank 20 or above would break that and is what the assertion catches; the fix in
that case is to renumber so postfix stays tightest, not to relax the assertion.

The one case where a greedy body and a body parsed at `min_bp = 2` would differ is
the binary `?` at rank 1 — `function() a ? b` could group either way. R's grammar
makes a function body `expr_or_assign_or_help`, which includes help and so points
at greedy, but do not take that on faith: let the oracle settle it the first time
it runs against `functions/definition.md`, and record the answer here if it is
not greedy.

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

/// Rank 20 — `f(…)`, `x[…]`, `x[[…]]`. Appears in no lookup table because
/// postfix is handled structurally in the Pratt loop, but it is named so the
/// `debug_assert!` guarding "postfix is the tightest-binding construct" reads as
/// a claim about the precedence table rather than a bare `20`.
const POSTFIX_BINDING_POWER: BindingPower = BindingPower(20);

fn infix_binding_power(kind: RSyntaxKind) -> Option<(BindingPower, Assoc)> {
    use RSyntaxKind::*;
    Some(match kind {
        T![?]                                    => (BindingPower(1),  Assoc::Left),
        T![<-] | T![<<-] | T![:=]                => (BindingPower(4),  Assoc::Right),
        T![=]                                    => (BindingPower(5),  Assoc::Right),
        T![->] | T![->>]                         => (BindingPower(6),  Assoc::Left),
        T![~]                                    => (BindingPower(7),  Assoc::Left),
        T![|] | T![||]                           => (BindingPower(8),  Assoc::Left),
        T![&] | T![&&]                           => (BindingPower(9),  Assoc::Left),
        T![<] | T![<=] | T![>] | T![>=]
        | T![==] | T![!=]                        => (BindingPower(11), Assoc::Left),
        T![+] | T![-]                            => (BindingPower(12), Assoc::Left),
        T![*] | T![/]                            => (BindingPower(13), Assoc::Left),
        SPECIAL | T![|>]                         => (BindingPower(14), Assoc::Left),
        T![:]                                    => (BindingPower(15), Assoc::Left),
        T![^] | T![**]                           => (BindingPower(17), Assoc::Right),
        T![$] | T![@]                            => (BindingPower(18), Assoc::Left),
        T![::] | T![:::]                         => (BindingPower(19), Assoc::Left),
        _ => return None,
    })
}

fn prefix_binding_power(kind: RSyntaxKind) -> Option<BindingPower> {
    Some(match kind {
        T![?]         => BindingPower(1),
        T![~]         => BindingPower(7),
        T![!]         => BindingPower(10),
        T![+] | T![-] => BindingPower(16),
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
    // Postfix is rank 20 and applies unconditionally, so there is no `min_bp`
    // guard around it below. That is only sound while 20 is genuinely the
    // ceiling: `min_bp` arrives as 0 from `parse_expression` and
    // `parse_argument`, as a prefix bp (max 16) from `parse_prefix_expression`,
    // and otherwise as this loop's own `right_bp` — which is `bp + 1` for
    // left-assoc, topping out at 20 because the highest infix rank is `:::` at
    // 19.
    //
    // Asserted rather than branched on. A branch here would be dead code that
    // silently starts mattering if someone adds an infix operator at rank 20 or
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
        if p.at_statement_break() { break }

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
            // `{` pushes LBRACE, not `(`, so braces do NOT get this.
            // `at_statement_break()` already encodes exactly that distinction,
            // so it is the whole of the check.
            T![::] | T![:::] if p.at_statement_break() => {
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
```

**Precedence facts worth writing explicit tests for**, all of which the
`Operator precedence` and `Precedence Tests` corpus tests cover:

- `-1^2` → `-(1^2)` — unary minus (16) binds looser than `^` (17).
- `!a & b` → `(!a) & b` — `!` (10) binds tighter than `&` (9).
- `-2:3` → `-(2:3)`? **No** — unary `-` is 16 and `:` is 15, so it is `(-2):3`,
  the sequence from -2 to 3. Contrast `-1^2` above: `^` (17) outranks unary `-`,
  `:` does not.
- `a <- b <- c` → `a <- (b <- c)` — right assoc.
- `a -> b -> c` → `(a -> b) -> c` — left assoc.
- `2^3^2` → `2^(3^2)` — right assoc.
- `a %in% b + c` → `a %in% (b + c)`? **No** — `%in%` is 14, `+` is 12, so
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
`IGNORE_NEWLINES` (§11.7). All three are aether-authored, so they are absent from
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

### 11.3 Phase 9 — Calls, subset, subset2

`src/syntax/call.rs`.

```rust
/// Postfix: `f(x)`, `x[i]`, `x[[i]]`. Returns `None` if no postfix applies.
fn parse_postfix(p: &mut RParser, lhs: CompletedMarker) -> Option<CompletedMarker> {
    // `f\n(x)` at statement level is two statements, not a call. Already
    // guaranteed by the `at_statement_break()` check in the caller, but the
    // errors corpus tests this directly ("Not A Call, Subset, Or Subset2").
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
```

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
    // Nothing to clear: `ALLOW_NEWLINE_BEFORE_ELSE` may stay set from an
    // enclosing `{`, and inside brackets it is harmless — see `parse_else_clause`.
    p.with_flags(RContextFlags::IGNORE_NEWLINES, RContextFlags::empty(), |p| {
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

`at_statement_break()` encodes exactly that distinction, so the check in §11.2 is
one guard clause. Add both as aether-authored tests in `expressions/namespace.md`
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
| `a$1(x)` | `(a$«1»)(x)` | mirrors `a$f(x)`, which is `(a$f)(x)` in R — call is rank 20, `$` is 18 |
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

**The newline subtlety.** `p.at_statement_break()` would normally stop us. Here it
must not: after `function`, after `)`, and after the parameter list, newlines carry
no meaning. The clean way is to *not consult* `at_statement_break()` in these
positions at all — it is only consulted in the Pratt loop and in `parse_postfix`.
Since `parse_function_definition` calls `parse_expression` unconditionally for the
body, newlines are naturally skipped. Verify against the `Functions` corpus test,
which contains all four spacing variants.

Parameters use `IGNORE_NEWLINES` (newlines eaten) and a comma-separated list where each
`RParameter` is `name` + optional `RParameterDefault`. Names may be `IDENT`,
`DOTS`, or `DOTDOTI` — note `return` is a plain identifier in R and is a valid
parameter name (`Functions With return As Parameter Name`).

**Enable:** the 7 imported tests across `corpus/functions/` — `definition.md`,
`lambda.md`, `parameters.md` — two of them `[error]`. Air's merged
`ok/function_definition.R` comes with them only if it stays within function
syntax; check it rather than assuming (§10.3.4).

### 11.6 Phase 12 — Control flow

`src/syntax/control.rs`. `for`, `while`, `repeat` are mechanical. `if`/`else` is
not.

```rust
fn parse_if_statement(p: &mut RParser) -> ParsedSyntax {
    if !p.at(T![if]) { return Absent }
    let m = p.start();
    p.bump(T![if]);
    p.expect(T!['(']);
    p.with_flags(RContextFlags::IGNORE_NEWLINES, RContextFlags::empty(), |p| {
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

There is a **third** case, and it is the one the flag collapse in §9.2 exposes:

```r
# legal — inside `(`, newlines are eaten entirely, braces or no braces
f(if (a) 1
  else 2)
```

So the rule is not "inside braces" but "a newline before `else` is fatal only
where newlines are significant *and* we are not in braces". Test the first
condition with `at_statement_break()` rather than `has_preceding_line_break()`,
which handles the paren case for free and keeps both of the language's newline
rules funnelling through one predicate:

```rust
fn parse_else_clause(p: &mut RParser) -> ParsedSyntax {
    if !p.at(T![else]) { return Absent }

    // A newline before `else` is only tolerated inside `{}` — or anywhere
    // newlines are insignificant, i.e. inside `(`/`[`/`[[`.
    if p.at_statement_break() && !p.allows_newline_before_else() {
        return Absent;   // caller finishes the `if`; `else` becomes a bogus statement
    }

    let m = p.start();
    p.bump(T![else]);
    parse_expression(p).or_add_diagnostic(p, diagnostics::expected_expression);
    Present(m.complete(p, R_ELSE_CLAUSE))
}
```

Note that `ALLOW_NEWLINE_BEFORE_ELSE` is therefore *not* cleared on entering
`(`/`[`/`[[` — leaving it set is harmless because `at_statement_break()` is already
false there, and clearing it would mean re-deriving brace context on the way back
out. Only `parse_braced_statements` sets it, and only `parse_else_clause` reads it.

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

### 11.7 Phase 13 — Blocks, statement lists, semicolons

`src/syntax/block.rs`. This lands the semicolon deviation.

```rust
fn parse_braced_statements(p: &mut RParser) -> ParsedSyntax {
    if !p.at(T!['{']) { return Absent }
    let m = p.start();
    p.bump(T!['{']);
    // The only site that still needs the `clear` argument: `f({\n  a\n  b\n})`
    // re-enters newline-significant territory from inside a paren.
    p.with_flags(
        RContextFlags::ALLOW_NEWLINE_BEFORE_ELSE,
        RContextFlags::IGNORE_NEWLINES,
        |p| parse_statement_list(p, StatementListEnd::RCurly),
    );
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
        let ate_semicolon = p.eat(T![;]);
        if had_expression
            && !ate_semicolon
            && !p.at(T![EOF])
            && !end.matches(p.cur())
            && !p.at_statement_break()
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
reallocation — pre-size it. The parser itself is single-pass: §11.3's argument
names are parsed optimistically and reshaped with `precede`, so no construct
backtracks and no token is lexed twice.

### 12.3 The swap into air

A single PR against `posit-dev/air`. Aether is now the source of truth for the
parser, the syntax layer, the factory, and `r.ungram`; air deletes all four and
becomes a consumer.

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
3. **Verify the rename is invisible.** `cargo build --workspace` should pass with
   zero changes to any `.rs` file outside the formatter. If it does not, something
   in air was reaching into a crate-private path.
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
   the reference for anything: remove `crates/aether_r_oracle/`, the `oracle`
   feature and its `#[cfg]` in `check()`, the `just oracle` recipe, and the
   `tree-sitter` / `tree-sitter-r` workspace deps from **aether** as well as air.
   The `[deviation]` marker stays — it still documents which tests intentionally
   differ, and `docs/deviations.md` still cross-references it — but it becomes
   inert in the harness. Say so in its row in §10.1's marker table when you do.
7. **Final acceptance gate:** air's entire formatter snapshot suite is unchanged
   except for semicolon-related files.

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
| **Trivia attachment drift** | A one-piece difference in whitespace attachment changes every snapshot and silently changes formatter output. | Port air's rules literally; the oracle compares ranges, so it catches this. Do the token source before any grammar work. |
| **`[[` / `]]` joining** | Genuinely ambiguous; tree-sitter needs an external scanner and still has open bugs. The lexer half landed in Phase 4; the parser half is Phase 9. | One join site, in the argument list's terminator check (§11.3). Six corpus tests exercise it. Do not invent a scope stack, and do not make the lexer glue closers. |
| **Codegen drift** | If aether's generated code diverges from air's, the swap-in fails in confusing ways. | Retired: Phase 2 asserted byte-identity, and CI asserts `just codegen` idempotence on every PR. |
| **Oracle unavailable on error tests** | tree-sitter bails on any error, so ~25 tests get no oracle coverage. | Accept it; those are exactly the tests where we *intend* to differ. Rely on reviewed snapshots there. |
| **Oracle skipped or rotted** | It is local-only and feature-gated (§10.4.1), so nothing forces anyone to run it, and nothing catches it breaking. | Named as an explicit step in every phase's checklist (§4). If it does break, fix cheaply or abandon it — it is deleted at swap-in regardless. Reviewed snapshots, not the oracle, are what carry forward. |
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
  §10.2's `check()` enforces the first three mechanically once Phase 6 lands.
