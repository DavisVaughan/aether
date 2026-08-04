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
| 3 | Vendor `tree-sitter-r` **and** air's tree-sitter→Rowan conversion as a **differential oracle running in CI**. |
| 4 | Grammar deviations land **first**, so the parser targets the final tree shape from day one. |
| 5 | Full Biome-style error recovery from the start: bogus nodes, `ParseRecoveryTokenSet`, rich `ParseDiagnostic`s. |
| 6 | **Markdown snapshot harness for everything.** No Biome-style `tests/specs/**` + `.rast`. |
| 7 | Imported-but-unsupported tests stay inert via a per-test `[pending]` marker; an unexpected pass is a hard failure. |
| 8 | Lossless round-trip (`source == tree.to_string()`) asserted on every test. No fuzzing, no CRAN corpus run for now. |
| 9 | Feature order: lexer → literals → binary/unary ops → calls & subsetting → functions → control flow → braces/semicolons → specials/pipes/raw strings → recovery polish → integration. |
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

```toml
biome_formatter    = { git = "https://github.com/lionel-/biome", rev = "a1296ea6ba363d8b8d8f02181b2a4ce9315c5ef9" }
biome_line_index   = { git = "https://github.com/lionel-/biome", rev = "a1296ea6..." }
biome_parser       = { git = "https://github.com/lionel-/biome", rev = "a1296ea6..." }
biome_rowan        = { git = "https://github.com/lionel-/biome", rev = "a1296ea6..." }
biome_string_case  = { git = "https://github.com/lionel-/biome", rev = "a1296ea6..." }
biome_text_size    = { git = "https://github.com/lionel-/biome", rev = "a1296ea6..." }
biome_unicode_table= { git = "https://github.com/lionel-/biome", rev = "a1296ea6..." }
biome_ungrammar    = { path = "./crates/biome_ungrammar" }
tree-sitter        = "0.24.7"
tree-sitter-r      = { git = "https://github.com/r-lib/tree-sitter-r", rev = "d459b97a9a11dd81643ab674938bede2874663b0" }
```

**Aether must pin the exact same Biome rev.** If aether and air disagree on the
Biome rev, `RSyntaxNode` types are nominally different and the swap-in at the end
becomes an ordeal. Additionally aether needs two crates air does not currently
depend on, from the same rev:

```toml
biome_console      = { git = "https://github.com/lionel-/biome", rev = "a1296ea6..." }
biome_diagnostics  = { git = "https://github.com/lionel-/biome", rev = "a1296ea6..." }
```

These are required for `ParseDiagnostic` rendering (`biome_diagnostics::print_diagnostic_to_string`
via `biome_console::fmt::Formatter`), which the markdown harness needs to emit the
`diagnostics` cell.

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

**What this costs at swap-in.** Air currently has ~hundreds of
`use air_r_syntax::…` lines across `air_r_formatter`, `workspace`, `lsp`, and
`comments`. Two ways to handle it, decided in Phase 14:

1. **Cargo package rename (recommended, zero-diff).** Air keeps writing
   `air_r_syntax` everywhere; Cargo maps the name:
   ```toml
   air_r_syntax = { package = "aether_r_syntax", git = "https://github.com/posit-dev/aether", rev = "..." }
   air_r_factory = { package = "aether_r_factory", git = "...", rev = "..." }
   air_r_parser = { package = "aether_r_parser", git = "...", rev = "..." }
   ```
   The swap becomes a handful of `Cargo.toml` lines plus the formatter changes
   the semicolon deviation forces, and nothing else.
2. **Mechanical find-and-replace in air.** Honest but noisy; it buries the
   substantive formatter diff under a thousand import changes. Prefer option 1
   for the swap PR, and do the rename as a separate follow-up in air if the owner
   wants the names to match.

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
    ├── aether_r_corpus/           # markdown harness: Document, check (§10.2)
    ├── aether_r_syntax/           # generated + hand-written ext traits
    ├── aether_r_factory/          # generated
    ├── aether_r_parser/           # THE NEW PARSER
    │   ├── src/
    │   │   ├── lib.rs
    │   │   ├── parse.rs           # public `parse()` entry point
    │   │   ├── options.rs
    │   │   ├── diagnostics.rs     # expected_* diagnostic constructors
    │   │   ├── token_source.rs    # RTokenSource
    │   │   ├── parser.rs          # RParser + context flags
    │   │   ├── lexer/
    │   │   │   ├── mod.rs         # RLexer
    │   │   │   └── tests.rs
    │   │   └── syntax/
    │   │       ├── mod.rs
    │   │       ├── expr.rs        # Pratt loop, primary exprs
    │   │       ├── call.rs        # call / subset / subset2 / arg lists
    │   │       ├── function.rs    # function defs, lambdas, parameters
    │   │       ├── control.rs     # if/else, for, while, repeat
    │   │       ├── block.rs       # braced exprs, statement lists, semicolons
    │   │       └── value.rs       # literals, strings, keywords-as-values
    │   └── tests/
    │       ├── main.rs            # harness entry
    │       ├── harness/           # markdown snapshot machinery
    │       │   ├── mod.rs
    │       │   ├── document.rs    # parse/render the .md format
    │       │   └── runner.rs
    │       ├── quick_test.rs      # scratchpad, #[ignore]d
    │       └── corpus/            # THE TESTS — see §10.3 for the full mapping
    │           ├── MANIFEST.toml   # every imported test name → its home file
    │           ├── literals/         # each X.md has a sibling X.snap.md
    │           │   ├── identifiers.md
    │           │   ├── identifiers.snap.md
    │           │   ├── numbers.md
    │           │   ├── numbers.snap.md
    │           │   ├── strings.md
    │           │   ├── strings.snap.md
    │           │   ├── comments.md
    │           │   ├── comments.snap.md
    │           │   ├── constants.md
    │           │   └── constants.snap.md
    │           ├── expressions/      # …and so on: X.md + X.snap.md throughout
    │           │   ├── unary.md
    │           │   ├── binary.md
    │           │   ├── colon.md
    │           │   ├── specials.md
    │           │   ├── formula.md
    │           │   ├── help.md
    │           │   ├── assignment.md
    │           │   ├── precedence.md
    │           │   ├── extract.md
    │           │   ├── namespace.md
    │           │   ├── call.md
    │           │   ├── subset.md
    │           │   ├── subset2.md
    │           │   └── parenthesized.md
    │           ├── functions/
    │           │   ├── definition.md
    │           │   ├── lambda.md
    │           │   └── parameters.md
    │           ├── control/
    │           │   ├── if.md
    │           │   ├── for.md
    │           │   ├── while.md
    │           │   └── repeat.md
    │           ├── statements/
    │           │   ├── braces.md
    │           │   ├── newlines.md
    │           │   └── semicolons.md
    │           └── recovery/
    │               └── unmatched-delimiters.md
    └── aether_r_oracle/           # dev-only differential oracle
        ├── Cargo.toml             # depends on tree-sitter, tree-sitter-r
        └── src/
            ├── lib.rs
            └── treesitter.rs      # vendored air parse.rs walker
```

---

## 4. Phase plan

Fourteen phases. Each is one reviewable PR. Phases 1–6 are infrastructure and
should land quickly; the owner wants to review the *parser* in small pieces, not
the scaffolding.

**Every phase from 7 onward ends with the same three steps:**
1. Flip the `[pending]` markers off the corpus tests the feature enables.
2. Regenerate snapshots, eyeball every changed cell.
3. Confirm the oracle diff is clean for all non-`[deviation]` tests.

| Phase | Title | Deliverable |
|---|---|---|
| 1 | Repo bootstrap | Workspace builds, CI green, nothing else |
| 2 | Vendor + rename syntax layer | `just codegen` reproduces `aether_r_syntax` + `aether_r_factory`, identical to air's modulo the rename |
| 3 | Grammar deviations | `.ungram` + kinds updated, regenerated, documented |
| 4 | Lexer | Standalone `RLexer` with its own token-stream tests |
| 5 | Parser scaffolding | `RParser`, `RTokenSource`, diagnostics, `parse()` returns an empty `R_ROOT` |
| 6 | Test harness + corpus import | All 89 tree-sitter tests imported as `[pending]`; oracle wired up |
| 7 | Literals, identifiers, program | `1`, `"a"`, `foo`, `TRUE`, `NA_integer_`, `..1`, `...` |
| 8 | Binary & unary operators | The Pratt core, precedence table, newline sensitivity |
| 9 | Calls, subset, subset2 | `f(a, b=1)`, `x[i]`, `x[[i]]`, hole arguments |
| 10 | Extract & namespace | `$`, `@`, `::`, `:::` and the deviation from tree-sitter |
| 11 | Functions | `function(x) y`, `\(x) y`, parameters, defaults |
| 12 | Control flow | `if`/`else` (incl. the newline rule), `for`, `while`, `repeat` |
| 13 | Blocks, statements, semicolons | `{}`, `R_STATEMENT_LIST`, real semicolon tokens |
| 14 | Recovery polish + air integration | Errors corpus fully green; swap into air |

---

## 5. Phase 1 — Repo bootstrap

Create the workspace. Nothing R-specific yet.

```toml
# Cargo.toml
[workspace]
members = ["crates/*", "xtask/codegen"]
resolver = "2"

[workspace.package]
authors      = ["Posit Software, PBC"]
edition      = "2024"
rust-version = "1.86"
license      = "MIT"
repository   = "https://github.com/posit-dev/aether"
publish      = false

[workspace.dependencies]
# --- pinned to the same fork+rev as posit-dev/air ---
biome_console       = { git = "https://github.com/lionel-/biome", rev = "a1296ea6ba363d8b8d8f02181b2a4ce9315c5ef9" }
biome_diagnostics   = { git = "https://github.com/lionel-/biome", rev = "a1296ea6ba363d8b8d8f02181b2a4ce9315c5ef9" }
biome_parser        = { git = "https://github.com/lionel-/biome", rev = "a1296ea6ba363d8b8d8f02181b2a4ce9315c5ef9" }
biome_rowan         = { git = "https://github.com/lionel-/biome", rev = "a1296ea6ba363d8b8d8f02181b2a4ce9315c5ef9" }
biome_string_case   = { git = "https://github.com/lionel-/biome", rev = "a1296ea6ba363d8b8d8f02181b2a4ce9315c5ef9" }
biome_text_size     = { git = "https://github.com/lionel-/biome", rev = "a1296ea6ba363d8b8d8f02181b2a4ce9315c5ef9" }
biome_unicode_table = { git = "https://github.com/lionel-/biome", rev = "a1296ea6ba363d8b8d8f02181b2a4ce9315c5ef9" }
biome_ungrammar     = { path = "./crates/biome_ungrammar" }

aether_r_corpus  = { path = "./crates/aether_r_corpus" }
aether_r_factory = { path = "./crates/aether_r_factory" }
aether_r_oracle  = { path = "./crates/aether_r_oracle" }
aether_r_parser  = { path = "./crates/aether_r_parser" }
aether_r_syntax  = { path = "./crates/aether_r_syntax" }

bitflags      = "2.6"
enumflags2    = "0.7"
proc-macro2   = "1.0"
quote         = "1.0"
rustc-hash    = "2.0"
serde         = { version = "1.0", features = ["derive"] }
similar       = "2.6"          # unified diffs in harness failure output
tracing       = "0.1"
tree-sitter   = "0.24.7"
tree-sitter-r = { git = "https://github.com/r-lib/tree-sitter-r", rev = "d459b97a9a11dd81643ab674938bede2874663b0" }
unicode-ident = "1.0"
xtask         = { path = "./xtask" }

[workspace.lints.rust]
# copy air's [lints] block verbatim
```

`justfile`:

```make
# Regenerate aether_r_syntax and aether_r_factory from r.ungram
codegen:
    cargo run -p xtask_codegen -- grammar
    cargo fmt

# Run the full test suite
test:
    cargo nextest run --workspace
    cargo test --doc --workspace          # nextest does not run doctests

# Regenerate all markdown snapshots, then verify they are stable
accept:
    cargo run -p xtask -- accept
    cargo nextest run --workspace

# Run the scratchpad in tests/quick_test.rs
quick:
    cargo nextest run -p aether_r_parser --run-ignored all -E 'test(quick_test)' --no-capture
```

### 5.1 nextest

**The test runner is [`cargo-nextest`](https://nexte.st), not `cargo test`.**
This is not cosmetic — three of its properties matter directly for a parser project.

**Process-per-test isolation.** Each test binary invocation runs one test in its
own process. A panic in one corpus file cannot take down the others, and a stack
overflow from runaway recursion (very possible while developing a recursive
descent parser) is reported as a clean failure rather than killing the run.

**Timeouts.** `cargo test` hangs forever on an infinite loop. nextest terminates
and reports it. Given that "the parser never loops forever" is one of our stated
invariants (§14), this turns a class of bug from "CI hangs for six hours" into a
normal red test.

**Filter expressions.** `-E 'test(/Dollar/)'` is far better than substring matching
once there are ~90 named corpus tests.

`.config/nextest.toml`:

```toml
[profile.default]
# Report every failing corpus file in one run instead of stopping at the first.
fail-fast = false
# A parser test that takes 10s is looping. Warn, then kill.
slow-timeout = { period = "10s", terminate-after = 3 }
failure-output = "immediate-final"
status-level = "fail"

[profile.ci]
inherits = "default"
fail-fast = false
retries = 0
failure-output = "immediate-final"
final-status-level = "slow"

[profile.ci.junit]
path = "junit.xml"
```

Note `retries = 0` deliberately: parser tests are deterministic, and a retry that
turns a real failure green is strictly harmful here.

**Two caveats to remember:**
1. nextest does **not** run doctests. `just test` runs `cargo test --doc`
   separately. If aether ends up with no doctests worth running, drop that line
   rather than letting it silently do nothing.
2. `#[ignore]` tests need `--run-ignored all` (see the `quick` recipe above).

### 5.2 CI

`.github/workflows/ci.yml` jobs:

```yaml
- uses: taiki-e/install-action@nextest
- run: cargo fmt --all --check
- run: cargo clippy --workspace --all-targets -- -D warnings
- run: cargo nextest run --workspace --profile ci
- run: cargo test --doc --workspace
# codegen freshness
- run: just codegen && git diff --exit-code
# snapshot freshness: `accept` must be a no-op on a clean tree
- run: cargo run -p xtask -- accept && git diff --exit-code
```

That last job is worth calling out: it means a PR can never land with stale
snapshot cells, because regenerating them on CI must produce no diff.

**Done when:** the empty workspace builds, `cargo nextest run --workspace` runs
(zero tests, exit 0), and CI is green.

---

## 6. Phase 2 — Vendor the syntax layer

Copy from air, unchanged:

```
air/xtask/codegen/            → aether/xtask/codegen/
air/xtask/codegen/r.ungram    → aether/xtask/codegen/r.ungram
air/crates/biome_ungrammar/   → aether/crates/biome_ungrammar/
air/crates/air_r_syntax/      → aether/crates/aether_r_syntax/
air/crates/air_r_factory/     → aether/crates/aether_r_factory/
```

Then apply **the rename**, which is three lines in
`xtask/codegen/src/language_kind.rs`:

```diff
-    pub fn formatter_crate_name(&self) -> String { format!("air_{self}_formatter") }
-    pub fn syntax_crate_name(&self)    -> String { format!("air_{self}_syntax") }
-    pub fn factory_crate_name(&self)   -> String { format!("air_{self}_factory") }
+    pub fn syntax_crate_name(&self)    -> String { format!("aether_{self}_syntax") }
+    pub fn factory_crate_name(&self)   -> String { format!("aether_{self}_factory") }
```

Because `ast.rs` derives its output paths from these functions, codegen now writes
to `crates/aether_r_syntax/src/generated/` and `crates/aether_r_factory/src/generated/`
without any further changes, and the generated factory emits
`use aether_r_syntax::…`. Delete `formatter_crate_name` and
`xtask/codegen/src/formatter.rs` along with it — aether has no formatter.

The hand-written files in the syntax crate (`lib.rs`, `syntax_node.rs`,
`call_ext.rs`, `argument_ext.rs`) and the factory's `lib.rs` are *not* generated,
so rename their crate references by hand. There are only a handful.

Air's codegen is otherwise multi-language-aware (`language_kind.rs` is built around
a `LanguageKind` enum with a single `R` variant); trim it further only if it drags
in unrelated deps. A smaller diff against upstream air is worth more than tidiness.

**Acceptance test for this phase — do not skip it.** Two checks:

1. `just codegen` on a clean checkout leaves `git diff --exit-code` empty
   (codegen is idempotent).
2. Aether's `crates/aether_r_syntax/src/generated/` is **byte-identical to air's
   `crates/air_r_syntax/src/generated/` after applying `s/air_r_/aether_r_/g` to
   air's copy**. Script this as a one-off comparison and paste the result in the
   PR. If it does not match, you have codegen drift that will silently corrupt
   every tree comparison for the rest of the project.

```bash
# scratch check, not committed
diff <(sed 's/air_r_/aether_r_/g' ../air/crates/air_r_syntax/src/generated/nodes.rs) \
     crates/aether_r_syntax/src/generated/nodes.rs
```

Also vendor air's hand-written syntax extensions: `syntax_node.rs`, `call_ext.rs`,
`argument_ext.rs`, and `lib.rs`. These are not generated and are used by air's
formatter, so they must come along for the swap-in to work.

**Done when:** `just codegen` is idempotent and output matches air byte-for-byte.

---

## 7. Phase 3 — Grammar deviations

This is where aether's tree intentionally diverges. Landing it now means every
later phase targets the final shape.

### 7.1 Semicolons

**Current behavior.** `SEMICOLON` exists in `r_kinds_src.rs` but never appears in a
tree. `parse.rs` maps `;` to `TriviaPieceKind::Whitespace`
(there are four `TODO(semicolon)` comments about this). So `1;2` snapshots as:

```
RDoubleValue { value_token: R_DOUBLE_LITERAL@0..1 "1" [] [] },
RDoubleValue { value_token: R_DOUBLE_LITERAL@1..3 "2" [Whitespace(";")] [] },
```

The `;` is literally inside a whitespace trivia piece. The formatter can never
`format_removed()` it because it is not a token.

**Why the obvious fix does not work.** You cannot put `SEMICOLON` tokens directly
into air's existing `R_EXPRESSION_LIST`. Biome's `AstNodeListIterator::slot_to_node`
panics on a token slot:

```rust
SyntaxSlot::Token(token) => panic!(
    "Expected node of type `{:?}` but found token `{:?}` instead.", ...
),
```

Node lists are homogeneous. A separated list (`(A (';' A)* ';'?)`) is also wrong,
because R separates statements by newline far more often than by `;`, and every
missing separator would become an error slot.

**The design: `RStatementList` of `RStatement`.** Air's `RExpressionList` is
replaced outright — it is not kept alongside — because once each element carries an
optional semicolon it is a list of *statements*, not of expressions, and the name
should say so.

```ungram
// ----------------------------------------------------------------
// Statements

RStatementList = RStatement*

// A single statement.
//
// Both slots are optional, using the same trick as `RArgument`:
// - `1`      → expression, no semicolon
// - `1;`     → expression + semicolon
// - `;`      → no expression, semicolon only (R allows `;;` and a leading `;`)
//
// A statement with neither slot filled is never produced by the parser.
RStatement =
	expression: AnyRExpression?
	semicolon: ';'?
```

The `RArgument` precedent is already in the file, with a comment explaining the
same technique:

```ungram
RArgument =
	name_clause: RArgumentNameClause?
	value: AnyRExpression?
```

The two nodes that held an `RExpressionList` are updated in place:

```diff
 RRoot =
 	bom: 'UNICODE_BOM'?
-	expressions: RExpressionList
+	statements: RStatementList
 	eof: 'EOF'

-RBracedExpressions =
+RBracedStatements =
 	'{'
-	expressions: RExpressionList
+	statements: RStatementList
 	'}'
```

**The field is renamed too**, so the generated accessors read the way the tree
actually is. Consumers walk `.statements()`, then reach through each statement:

```rust
for statement in root.statements() {
    // `expression` is `Option<AnyRExpression>` — absent for a bare `;`
    if let Some(expression) = statement.expression() {
        // …
    }
    // `semicolon_token` is `Option<SyntaxToken>` — present only when written
    if let Some(semicolon) = statement.semicolon_token() {
        // the formatter's `format_removed(&semicolon)` hook
    }
}
```

The exact generated signatures, confirmed against air's current
`crates/air_r_syntax/src/generated/nodes.rs`:

```rust
impl RRoot {
    pub fn bom_token(&self) -> Option<SyntaxToken> { ... }
    pub fn statements(&self) -> RStatementList { ... }   // list slots are never Option
    pub fn eof_token(&self) -> SyntaxResult<SyntaxToken> { ... }
}

impl RStatement {
    pub fn as_fields(&self) -> RStatementFields { ... }
    pub fn expression(&self) -> Option<AnyRExpression> { ... }
    pub fn semicolon_token(&self) -> Option<SyntaxToken> { ... }
}
```

Three codegen rules are doing the work here, and they are worth internalising
because they determine what the formatter code will look like: an optional node
slot becomes `Option<T>` (exactly like today's `RArgument::value()`), an optional
token slot becomes `Option<SyntaxToken>` *and* gets a `_token` suffix (exactly like
today's `RRoot::bom_token()`), and a list slot is returned bare because it is
always present even when empty. So `root.statements()` needs no unwrapping and
iterates directly.

`RBracedExpressions` is renamed to **`RBracedStatements`** in the same pass, since
it now holds statements rather than expressions. Two more mechanical edits follow
from it — the union that lists it:

```diff
 AnyRExpression =
 	AnyRValue
 	| RIdentifier
 	…
-	| RBracedExpressions
+	| RBracedStatements
```

and `r_kinds_src.rs`:

```diff
-        "R_BRACED_STATEMENTS",
+        "R_BRACED_STATEMENTS",
```

At swap time this renames air's `AnyRExpression::RBracedExpressions` variant and
its formatter rule file
`crates/air_r_formatter/src/r/expressions/braced_expressions.rs`
(→ `braced_statements.rs`). Air's formatter codegen derives those paths from the
node names, so re-running air's formatter codegen after the swap generates the new
stub; expect to move the hand-written body across by hand and retarget its `impl`.

In `r_kinds_src.rs`, the `nodes` array changes:

```diff
-        "R_EXPRESSION_LIST",
+        "R_STATEMENT_LIST",
+        "R_STATEMENT",
```

`R_STATEMENT_LIST` must be recognised as a **list** kind so
`SyntaxKind::is_list()` returns true for it — air's codegen derives that from the
`*` in the ungram rule, so it happens automatically, but the harness's
`has_bogus_nodes_or_empty_slots` check depends on it, so verify it after
regenerating.

**Consequences to write down in the PR description**, because air's formatter will
need updating at swap time:
- `RStatementList` iteration yields `RStatement`, not `AnyRExpression`.
- `RRoot::statements()` and `RBracedStatements::statements()` replace
  `.expressions()`; every walk gains a `.expression()` hop to reach the payload.
- `RStatement::semicolon_token()` is the new hook the formatter uses to drop or
  rewrite semicolons.
- Air's formatter rules that walk expressions directly gain one `.expression()`
  hop and a `format_removed(semicolon)` (or `format_replaced` with a hard line,
  per the owner's preference — flag it, don't decide it unilaterally).

**Alternative if the owner rejects the wrapper:** keep semicolons as trivia but
introduce a dedicated `TriviaPieceKind` — not possible, `TriviaPieceKind` is a
Biome enum. Or emit `;` as a real token but keep a list of bare expressions and
attach the `;` as a slot on each expression node — requires touching every
expression node in the grammar. The wrapper is cleaner. Present it, note the
formatter cost, and get a thumbs-up before writing code.

### 7.2 Extract operator RHS

**Current behavior.** tree-sitter-r's `extract_operator` has `optional(field("rhs", ...))`
deliberately — the grammar comment says this is so `dplyr::` yields a namespace
node with a known package name for completions. Air's `.ungram` already declares
the RHS **required** (`right: AnyRSelector`), so the mismatch shows up as
`right: missing (required)` with **no diagnostic**, and the test lives in
`tests/snapshots/undefined/` precisely because nobody was happy with it. Its header
comment says:

> tree-sitter-r currently allows an `optional()` RHS, but our grammar requires a
> RHS, so we get `missing (required)`. These should eventually be parse errors.

**Aether's behavior.** No `.ungram` change is needed — the slot is already
required. The deviation is purely in the parser:

- `a$b`, `a$"b"`, `a$...`, `a$..1` → `R_EXTRACT_EXPRESSION`, clean.
- `a$` → `R_EXTRACT_EXPRESSION` with a missing `right` slot **plus** an
  `expected_selector` diagnostic pointing at the position after `$`, with a
  secondary label on the `$` itself. The diagnostic names the specific operator,
  so `a$`, `a@`, `a::` and `a:::` each read differently:

  ```
  × Expected an identifier, a string, `...` or `..i` but instead found the end of the file.

    1 │ dplyr::
      │      ── `::` requires a name on the right
      │        ▲
      │        ╰── Expected an identifier, a string, `...` or `..i` here.
  ```
- `a$1`, `a$NA`, `a$NULL`, `a$TRUE`, `a$(b)`, `a${b}` → the RHS is present but not
  a valid selector: wrap it in `R_BOGUS` and emit `expected_selector` with the
  offending range labelled.

Same treatment for `@`, `::`, `:::`.

This means `tests/snapshots/undefined/extract_expression_error.R` and
`namespace_expression_error.R` become *error* tests in aether, and their corpus
counterparts (`Dollar No RHS`, `Slot No RHS`, `Namespace With Missing RHS`) get
marked `[deviation]` so the oracle skips them.

### 7.3 The deviation ledger

Create `docs/deviations.md` in aether and keep it current. Every intentional
difference from tree-sitter-r goes here with an example, the tree-sitter output,
the aether output, and the rationale. The oracle reads nothing from this file, but
reviewers will, and so will whoever updates air's formatter.

Give it **two** headings. The first is aether's own deviations. The second,
*"tree-sitter-r vs. R"*, records places where tree-sitter-r already differs from R
and aether follows tree-sitter for oracle agreement — see §8.4 for two concrete
ones (`%foo` at EOF, and backslashes inside `%…%`). Keeping the categories apart
matters: the first list is a set of choices we made and can revisit, the second is
inherited behaviour we are deliberately not fixing yet.

Seed the first with:

| # | Deviation | tree-sitter-r | aether |
|---|---|---|---|
| 1 | Semicolons | trivia, invisible | `SEMICOLON` token in `RStatement` |
| 1b | Statement list | `R_EXPRESSION_LIST` of expressions | `R_STATEMENT_LIST` of `R_STATEMENT` |
| 1c | Braces | `R_BRACED_EXPRESSIONS` | `R_BRACED_STATEMENTS` |
| 2 | Extract RHS | `optional()`, silent | required, diagnostic + bogus |
| 3 | Namespace RHS | `optional()`, silent | required, diagnostic + bogus |
| 4 | Error recovery | whole-file bogus node | localized bogus nodes, tree still walkable |

**Done when:** `.ungram` and `r_kinds_src.rs` updated, `just codegen` run, generated
code compiles, `docs/deviations.md` seeded, and the delta vs air's generated code is
*only* the expected statement-list changes: `R_EXPRESSION_LIST` replaced by
`R_STATEMENT_LIST`, `R_STATEMENT` added, and `RRoot`/`RBracedStatements` pointing
at the new list type.

---

## 8. Phase 4 — The lexer

`crates/aether_r_parser/src/lexer/mod.rs`. This phase produces a **standalone,
independently testable** lexer. No parser yet.

### 8.1 The trait we implement

From `biome_parser::lexer::Lexer` at the pinned rev:

```rust
pub trait Lexer<'src> {
    const NEWLINE: Self::Kind;
    const WHITESPACE: Self::Kind;
    type Kind: SyntaxKind;
    type LexContext: LexContext + Default;
    type ReLexContext;

    fn source(&self) -> &'src str;
    fn current(&self) -> Self::Kind;
    fn current_start(&self) -> TextSize;
    fn next_token(&mut self, context: Self::LexContext) -> Self::Kind;
    fn has_preceding_line_break(&self) -> bool;
    fn has_unicode_escape(&self) -> bool;
    fn rewind(&mut self, checkpoint: LexerCheckpoint<Self::Kind>);
    fn finish(self) -> Vec<ParseDiagnostic>;
    fn position(&self) -> usize;
    fn push_diagnostic(&mut self, diagnostic: ParseDiagnostic);
    // + provided: current_range, current_flags, consume_newline_or_whitespaces, ...
}

pub trait LexerWithCheckpoint<'src>: Lexer<'src> {
    fn checkpoint(&self) -> LexerCheckpoint<Self::Kind>;
}
```

Implement both. `LexerWithCheckpoint` is what gives us `NthToken` lookahead for
free via `BufferedLexer`, and we need lookahead in at least three places (`[[`
disambiguation, `else` after newlines, argument-name detection).

### 8.2 Skeleton

```rust
// crates/aether_r_parser/src/lexer/mod.rs
use aether_r_syntax::{RSyntaxKind, RSyntaxKind::*, T};
use biome_parser::diagnostic::ParseDiagnostic;
use biome_parser::lexer::{
    Lexer, LexerCheckpoint, LexerWithCheckpoint, LexContext, TokenFlags,
};
use biome_rowan::{TextRange, TextSize};

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub(crate) enum RLexContext {
    #[default]
    Regular,
    /// Lex the body of a string opened with this delimiter.
    /// Emits `STRING_CONTENT` (possibly empty → we skip straight to close).
    StringContent(RStringDelimiter),
    /// Lex the closing delimiter of a string.
    StringEnd(RStringDelimiter),
}

impl LexContext for RLexContext {
    fn is_regular(&self) -> bool {
        matches!(self, RLexContext::Regular)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum RStringDelimiter {
    Single,
    Double,
    /// `r"---[` … `]---"`. Records the quote char, hyphen count, bracket pair.
    Raw { quote: u8, hyphens: u8, bracket: RawBracket },
}
// NOTE: there is deliberately no `Backtick` variant. See §8.4 — a backtick-quoted
// name is an IDENT, not a string.

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum RawBracket { Paren, Bracket, Brace }

pub(crate) struct RLexer<'src> {
    source: &'src str,
    position: usize,
    current_kind: RSyntaxKind,
    current_start: TextSize,
    current_flags: TokenFlags,
    after_newline: bool,
    diagnostics: Vec<ParseDiagnostic>,
    /// Set when we emit `STRING_OPEN`; cleared after `STRING_CLOSE`.
    string_delimiter: Option<RStringDelimiter>,
}

impl<'src> Lexer<'src> for RLexer<'src> {
    const NEWLINE: RSyntaxKind = NEWLINE;
    const WHITESPACE: RSyntaxKind = WHITESPACE;
    type Kind = RSyntaxKind;
    type LexContext = RLexContext;
    type ReLexContext = RReLexContext;

    fn next_token(&mut self, context: RLexContext) -> RSyntaxKind {
        self.current_flags = TokenFlags::empty();
        self.current_start = TextSize::from(self.position as u32);

        let kind = if self.is_eof() {
            EOF
        } else {
            match context {
                RLexContext::Regular => self.lex_token(),
                RLexContext::StringContent(d) => self.lex_string_content(d),
                RLexContext::StringEnd(d) => self.lex_string_end(d),
            }
        };

        self.current_kind = kind;
        debug_assert!(self.current_start <= TextSize::from(self.position as u32));
        kind
    }

    fn has_preceding_line_break(&self) -> bool {
        self.current_flags.contains(TokenFlags::PRECEDING_LINE_BREAK)
    }

    fn has_unicode_escape(&self) -> bool { false } // R has none at token level
    // ... trivial accessors
}
```

### 8.3 `lex_token` dispatch

Order matters. Copy the ordering decisions from tree-sitter-r's `grammar.js`
comments — several of them exist specifically to fix filed bugs.

```rust
fn lex_token(&mut self) -> RSyntaxKind {
    let byte = self.current_byte();
    match byte {
        b'\n' | b'\r'          => self.consume_newline(),
        b if is_r_whitespace(b)=> self.consume_whitespace(),
        b'#'                   => self.consume_comment(),

        b'"'  => self.consume_string_open(RStringDelimiter::Double),
        b'\'' => self.consume_string_open(RStringDelimiter::Single),
        b'`'  => self.consume_backtick_identifier(),

        // `r"(...)"` / `R'---[...]---'`. Falls through to identifier if the
        // raw-string prefix does not actually match.
        b'r' | b'R' if self.peek_is_raw_string_open() => self.consume_raw_string_open(),

        b'0'..=b'9' => self.consume_number(),

        // `.` is overloaded: `...`, `..1`, `.5` (a number!), `.foo` (identifier).
        // tree-sitter-r has an explicit note: an identifier may start with `.`
        // only if the `.` is NOT followed by a digit, otherwise `.1i` and `.1L`
        // get misparsed as identifiers (their issue #190).
        b'.' => self.consume_dot_dispatch(),

        b'%' => self.consume_special(),

        b'_' | b'a'..=b'z' | b'A'..=b'Z' => self.consume_identifier_or_keyword(),
        b if b >= 128 => self.consume_identifier_or_keyword(), // unicode XID_Start

        _ => self.consume_punct(),
    }
}
```

### 8.4 The token inventory

Everything the lexer must be able to produce, taken from
`xtask/codegen/src/r_kinds_src.rs`:

**Puncts.** `; , { } [ ] [[ ]] ( ) ? ~ <- <<- := -> ->> = | & || && < <= > >= == != + - * / ^ ** |> : :: ::: $ @ ! ... \`

Maximal munch matters and is fiddly. Concretely: `<` → `<-` → `<<-` (note `<<-`
is three chars and `<<` alone is not a token); `-` → `->` → `->>`; `:` → `::` →
`:::` → `:=`; `*` → `**`; `|` → `||` → `|>`; `&` → `&&`; `=` → `==`; `!` → `!=`;
`<` → `<=`; `>` → `>=`. Write a table-driven test for every prefix.

**`[` and `[[`.** tree-sitter-r resolves these in the external scanner with a scope
stack, because `x[[1]]` vs `x[ [1] ]` vs `x[[1], 2]` are genuinely ambiguous and
issue #177 (`Unsupported Subset2 With ]] Over Multiple Lines`) is in the errors
corpus. **Recommendation: lex `[` and `]` as single tokens only**, and let the
*parser* decide whether two adjacent `[` form a `[[`, using a re-lex step. See
§10.4 — this is the single most important design call in the lexer and the one
most likely to be got wrong.

**Keywords.** `function for in while repeat if else next break TRUE FALSE NULL Inf
NaN NA NA_integer_ NA_real_ NA_complex_ NA_character_`.
Lex as `IDENT` first, then map the text to a keyword kind — the standard Biome
approach, and it automatically gets the `else_idx` case right (tree-sitter-r
issue #200: `elseμ <- 1` must not see an `else` keyword).

**Literals.**
```
_hex_literal    = 0[xX](([0-9a-fA-F]+(\.[0-9a-fA-F]*)?)|(\.[0-9a-fA-F]*))([pP][+-]?[0-9]+)?
_number_literal = (?:(?:\d+(?:\.\d*)?)|(?:\.\d+))(?:[eE][+-]?\d*)?
integer         = _float_literal "L"   → R_INTEGER_LITERAL
complex         = _float_literal "i"   → R_COMPLEX_LITERAL
float           = _float_literal       → R_DOUBLE_LITERAL
```
Note `[eE][+-]?\d*` permits a **trailing empty exponent** (`1e`) — that is
deliberate in tree-sitter-r; match it.

**Identifiers.**
```
unquoted            = [\p{XID_Start}_][\p{XID_Continue}.]*
unquoted_leading_dot= \.(?:[\p{XID_Start}._][\p{XID_Continue}.]*)?
quoted              = `((?:\\(.|\n))|[^`\\])*`
```
`_` alone is a valid identifier (the native-pipe placeholder). `_foo` lexes as a
single identifier and is *not* rejected here — tree-sitter-r's comment says
downstream consumers do that check. Match that.

**Backticks produce an identifier, not a string.** `` `my var` `` is one `IDENT`
token covering the backticks and everything between them, because tree-sitter-r
puts the quoted form inside the `identifier` rule (the `quoted` line above), and
air's walker maps it straight to `R_IDENTIFIER`. It never becomes an
`R_STRING_VALUE`, so there is no `STRING_OPEN`/`STRING_CONTENT`/`STRING_CLOSE`
triple and no `Backtick` variant in `RStringDelimiter`.

The two places this matters:
- **Escapes still apply inside backticks** (`` `a\`b` ``), per the `quoted` regex
  above, so `consume_backtick_identifier` needs the same backslash handling as a
  string body even though the result is one token.
- **An unterminated backtick is an identifier error, not a string error.** The
  `Unclosed Backtick` corpus test needs its own diagnostic
  (`unterminated_backtick_identifier`), emitted by the *lexer*, whereas the three
  unclosed-quote tests surface through the parser's `expect(STRING_CLOSE)`.

**Backticked names may span newlines**, so scan to the matching backtick, not to
end of line:

```r
`my
var` <- 1     # one identifier whose name contains a newline. Legal R.
```

The `quoted` regex above permits this — its `[^`\\]` class includes `\n`, and the
escape branch `\\(.|\n)` explicitly allows an escaped newline.

R agrees, and its source says so out loud. Backticks are lexed by the *same*
function as strings — `token()` dispatches `` ` `` to `StringValue(c, forSymbol=true)`
— and that function contains:

```c
while ((c = xxgetc()) != R_EOF && c != quote) {
    CTEXT_PUSH(c);
    if (c == '\n') {
        xxungetc(c); CTEXT_POP();
        /* Fix suggested by Mark Bravington to allow multiline strings
         * by pretending we've seen a backslash. Was:
         * return ERROR;
         */
        c = '\\';
    }
```

So multiline support here is deliberate and was added on purpose. It also means
**escapes and multiline behaviour are identical for backticks and strings**, since
they share one implementation — which is the cleanest justification for
`consume_backtick_identifier` and the string-body scanner sharing their escape
handling. tree-sitter-r's `Strings` corpus test contains `"foo\nbar"` and
`'foo\nbar'` as literal two-line strings, so this is covered from Phase 7 onward.

So there is no line-based early stop available for any of these tokens, and an
unterminated one genuinely does run to EOF. That is also what tree-sitter does — its
`Unclosed Backtick` expectation is `(ERROR (UNEXPECTED '\0'))`, i.e. it consumed to
the end. Do not try to be cleverer than the language here; the way to keep the
error useful is **where you point the diagnostic**, not where you stop scanning:

```rust
// Point at the OPENING delimiter, which is the actionable location, and label
// the end of input separately. Highlighting the whole rest of the file is useless.
fn unterminated_backtick_identifier(open: TextRange, eof: TextRange) -> ParseDiagnostic {
    ParseDiagnostic::new("Unterminated backtick-quoted name.", open)
        .with_detail(eof, "reached end of file looking for a closing `` ` ``")
}
```

Because these can span lines, `enable_recovery_on_line_break()` will not help the
parser resynchronise after one, and it should not be expected to. One unterminated
delimiter is a single unrecoverable error for the remainder of the file, in aether
exactly as in R.

Add an aether-authored test for the multi-line backtick to
`literals/identifiers.md` — tree-sitter-r's corpus covers multi-line *strings* but
never a multi-line backticked name, so nothing else would catch a regression here.

**Dots.** `...` → `DOTS`; `/[.][.]\d+/` → `DOTDOTI`. Both must be tried *before*
the identifier rule, or `identifier` swallows them.

**Special.** `/%[^%\\\n]*%/` → `SPECIAL`. Covers `%%`, `%/%`, `%in%`, `%>%`.

Unlike strings and backticks, **`%…%` may not span a newline**, confirmed in R's
own lexer (`SpecialValue` in `src/main/gram.y`):

```c
static int SpecialValue(int c)
{
    YYTEXT_PUSH(c, yyp);
    while ((c = xxgetc()) != R_EOF && c != '%') {
        if (c == '\n') {
            xxungetc(c);
            return ERROR;      // newline terminates with an error
        }
        YYTEXT_PUSH(c, yyp);
    }
    if (c == '%') YYTEXT_PUSH(c, yyp);
    ...
    return SPECIAL;
}
```

Two details in there are worth copying rather than inventing:

- **R ungets the newline.** So the error stays local: the newline is still
  available to terminate the statement, and the next line parses normally. Aether
  should do the same — on hitting a newline, stop *before* consuming it, emit
  `unterminated_special`, and emit the partial `%foo` as a bogus token. Do not
  scan to EOF the way an unterminated string must (§8.4); here the language gives
  us a resynchronisation point, so use it.
- **R accepts `%foo` at EOF.** The loop exits on `R_EOF`, the closing `%` is never
  pushed, and it still returns `SPECIAL`. tree-sitter-r's regex requires the
  closing `%`, so it errors. **Follow tree-sitter**, since it is the oracle — but
  record the divergence.

There is one more R-vs-tree-sitter gap here: tree-sitter's character class excludes
backslash (`[^%\\\n]`) while R's `SpecialValue` does not, so `%a\b%` is a valid
`SPECIAL` in R but not in tree-sitter. Their own comment concedes the regex is
approximate ("This could probably be fine tuned to disallow more things"). Follow
tree-sitter for oracle agreement and log both gaps in `docs/deviations.md` under a
separate **"tree-sitter-r vs. R"** heading — these are not aether's deviations, they
are inherited, and conflating the two categories will confuse whoever reads that
doc next.

**Strings.** Three tokens: `STRING_OPEN`, `STRING_CONTENT` (optional),
`STRING_CLOSE`. Content is one token spanning everything between delimiters,
including escapes — air's tree walker does not model `escape_sequence` as a
separate node, and neither does `r.ungram` (`RStringValue` has exactly
`open`/`content?`/`close`).

**Raw strings.** `r`/`R`, then `"` or `'`, then 0–255 `-`, then one of `([{`. The
close is the matching `)]}` + the same hyphen count + the same quote. Port
`scan_raw_string_open` and `scan_raw_string_content_or_close` from
`tree-sitter-r/src/scanner.c` (lines ~527–700) more or less directly.

### 8.5 Newlines: the crucial decision

**Newlines are lexed as `NEWLINE` tokens, then converted to trivia by the token
source — but the parser can still ask whether one occurred.**

Do *not* replicate tree-sitter's scope-stack approach of conditionally swallowing
newlines in the lexer. In a hand-written recursive-descent parser the equivalent
information is available structurally: the parser knows whether it is inside
`(`/`[`/`[[`, so it can simply ignore `has_preceding_line_break()` there. Carrying a
scope stack in the lexer would duplicate that knowledge and is the classic source
of desync bugs.

So:
- `RLexer` emits `NEWLINE` as its own kind.
- `RTokenSource` reports `NEWLINE` (and `WHITESPACE`, `COMMENT`) as trivia and
  sets `PRECEDING_LINE_BREAK` on the next real token.
- `RParser` consults `p.has_preceding_line_break()` at the exactly four places R
  cares (§10.3).

`Trivia` kinds map as follows (matching air's current `parse.rs` behavior so the
snapshots line up):

| Lexer kind | `TriviaPieceKind` |
|---|---|
| `WHITESPACE` | `Whitespace` |
| `NEWLINE` | `Newline` |
| `COMMENT` | `SingleLineComment` |

**A `\r\n` is one `Newline` trivia piece, not two.** Air has CRLF tests
(`tests/snapshots/ok/crlf/*`); port them.

### 8.6 Testing this phase

`crates/aether_r_parser/src/lexer/tests.rs`. A macro-driven table asserting the exact
token stream, in Biome's style:

```rust
macro_rules! assert_lex {
    ($src:expr, $($kind:ident:$len:expr),* $(,)?) => {{
        let mut lexer = RLexer::from_str($src);
        let mut idx = 0usize;
        let mut tokens = vec![];
        loop {
            let kind = lexer.next_token(RLexContext::Regular);
            if kind == EOF { break }
            tokens.push((kind, lexer.current_range()));
        }
        let expected: Vec<(RSyntaxKind, usize)> = vec![$(($kind, $len)),*];
        // assert kinds and lengths, and that lengths sum to src.len()
    }};
}

#[test] fn lex_number_suffixes() {
    assert_lex!("1L 1i 1 0x1p3 .5 1e 1e+ 1e-3",
        R_INTEGER_LITERAL:2, WHITESPACE:1,
        R_COMPLEX_LITERAL:2, WHITESPACE:1,
        R_DOUBLE_LITERAL:1,  WHITESPACE:1,
        R_DOUBLE_LITERAL:5,  /* ... */);
}

#[test] fn lex_dot_dispatch() {
    assert_lex!("... ..1 .5 .foo . ..",
        DOTS:3, WHITESPACE:1, DOTDOTI:3, WHITESPACE:1,
        R_DOUBLE_LITERAL:2, WHITESPACE:1, IDENT:4, WHITESPACE:1,
        IDENT:1, WHITESPACE:1, IDENT:2);
}

#[test] fn lex_backtick_is_an_identifier() {
    // One IDENT token, backticks included. NOT a STRING_OPEN/CONTENT/CLOSE triple.
    assert_lex!("`my var` `a\\`b`",
        IDENT:8, WHITESPACE:1, IDENT:6);
}

#[test] fn lex_backtick_spans_newlines() {
    // Legal R: one identifier whose name contains a newline.
    assert_lex!("`my\nvar`", IDENT:8);
}

#[test] fn lex_special_stops_at_newline() {
    // R's `SpecialValue` ungets the newline and errors, so the NEWLINE token
    // survives to terminate the statement. `%foo` becomes a bogus token.
    assert_lex!("%foo\n1",
        R_BOGUS:4, NEWLINE:1, R_DOUBLE_LITERAL:1);
    // …and a well-formed one is a single SPECIAL.
    assert_lex!("a %in% b",
        IDENT:1, WHITESPACE:1, SPECIAL:4, WHITESPACE:1, IDENT:1);
}

#[test] fn lex_string_spans_newlines() {
    // Same rule for strings — see tree-sitter-r's `Strings` corpus test.
    assert_lex!("\"foo\nbar\"",
        STRING_OPEN:1, STRING_CONTENT:7, STRING_CLOSE:1);
}

#[test] fn lex_else_is_not_a_prefix() {
    assert_lex!("else elseif else_idx elseμ",
        ELSE_KW:4, WHITESPACE:1, IDENT:6, WHITESPACE:1,
        IDENT:8, WHITESPACE:1, IDENT:6);
}
```

**Always assert that token lengths sum to the source length.** That single
invariant catches the majority of lexer bugs and is the lexer-level analogue of
the round-trip check.

**Done when:** every token kind in `r_kinds_src.rs` has lexer coverage, all
`literals.md` inputs lex without diagnostics (they will not *parse* yet), and the
sum-of-lengths invariant holds on every test.

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
    preceding_line_break: bool,
}

impl<'src> RTokenSource<'src> {
    pub fn from_str(source: &'src str) -> Self {
        let lexer = BufferedLexer::new(RLexer::from_str(source));
        let mut source = Self { lexer, trivia: Vec::new(), preceding_line_break: false };
        source.next_non_trivia_token(RLexContext::default(), true);
        source
    }

    fn next_non_trivia_token(&mut self, context: RLexContext, first: bool) {
        let mut trailing = !first;
        self.preceding_line_break = false;

        loop {
            let kind = self.lexer.next_token(context);
            let trivia_kind = match kind {
                WHITESPACE => TriviaPieceKind::Whitespace,
                NEWLINE    => TriviaPieceKind::Newline,
                COMMENT    => TriviaPieceKind::SingleLineComment,
                _ => break,
            };

            // A comment or newline ends the "trailing trivia of the previous
            // token" run; everything after belongs to the next token as leading.
            if trivia_kind.is_newline() {
                trailing = false;
                self.preceding_line_break = true;
            }

            self.trivia.push(Trivia::new(trivia_kind, self.lexer.current_range(), trailing));

            if trivia_kind.is_single_line_comment() {
                trailing = false;
            }
        }
    }
}
```

**Trivia attachment must match air's existing rules exactly**, because a mismatch
shows up in every snapshot. Air's `parse.rs` (see the comment block around line
1139) states them as:

> Between two tokens, all trivia is leading unless we see a newline. Everything up
> to and including the last token on the same line is trailing of that token; the
> newline and everything after is leading of the next token.

Port that logic literally, then let the oracle confirm it. Expect to spend real
time here — trivia attachment is the highest-risk area for silent snapshot churn,
and it is exactly the kind of thing the differential oracle exists to catch.

Implement `BumpWithContext` (for string lexing) and `TokenSourceWithBufferedLexer`
(which gives `NthToken` lookahead automatically).

### 9.2 `RParser`

```rust
// crates/aether_r_parser/src/parser.rs
use crate::token_source::RTokenSource;
use aether_r_syntax::RSyntaxKind;
use biome_parser::diagnostic::merge_diagnostics;
use biome_parser::event::Event;
use biome_parser::prelude::*;
use biome_parser::{ParserContext, ParserContextCheckpoint};

pub(crate) struct RParser<'src> {
    context: ParserContext<RSyntaxKind>,
    source: RTokenSource<'src>,
    options: RParserOptions,
    /// Stack-discipline flags. See `RContextFlag`.
    state: RParserState,
}

bitflags::bitflags! {
    /// Context that changes how newlines and certain tokens are interpreted.
    #[derive(Debug, Clone, Copy, PartialEq, Eq)]
    pub(crate) struct RContextFlags: u8 {
        /// Inside `(`, `[`, or `[[`. Newlines carry no meaning here — R's parser
        /// eats them, and so does tree-sitter-r's external scanner
        /// (`consume_whitespace_and_ignored_newlines`).
        const IN_PARENS       = 1 << 0;
        /// Inside `{` or at top level. A newline here can terminate a statement.
        const NEWLINE_SENSITIVE= 1 << 1;
        /// Inside `{`. `else` on a following line is legal here but not at top level.
        const IN_BRACES       = 1 << 2;
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
    /// Run `f` with `flags` set, restoring the previous flags afterwards.
    pub(crate) fn with_flags<F, R>(&mut self, set: RContextFlags, clear: RContextFlags, f: F) -> R
    where F: FnOnce(&mut Self) -> R {
        let saved = self.state.flags;
        self.state.flags.insert(set);
        self.state.flags.remove(clear);
        let result = f(self);
        self.state.flags = saved;
        result
    }

    /// True when a newline before the current token should end the current
    /// expression. False inside `(`/`[`/`[[`.
    pub(crate) fn at_statement_break(&self) -> bool {
        self.state.flags.contains(RContextFlags::NEWLINE_SENSITIVE)
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

**Recommendation:** keep `Parse` with the same method names and add diagnostics:

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
    pub fn into_result(self) -> Result<RSyntaxNode, ParseError> { /* unchanged */ }
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

pub(crate) fn expected_parameter(p: &RParser, range: TextRange) -> ParseDiagnostic { ... }
pub(crate) fn expected_argument(p: &RParser, range: TextRange) -> ParseDiagnostic { ... }
pub(crate) fn expected_condition(p: &RParser, range: TextRange) -> ParseDiagnostic { ... }
// Backtick-quoted names are identifiers, not strings — separate wording.
// Takes two ranges: the opening delimiter (primary) and EOF (detail). See §8.4.
pub(crate) fn unterminated_backtick_identifier(open: TextRange, eof: TextRange) -> ParseDiagnostic { ... }
pub(crate) fn unterminated_string(open: TextRange, eof: TextRange) -> ParseDiagnostic { ... }
// `%…%` cannot span a newline; points at the `%` that opened it. See §8.4.
pub(crate) fn unterminated_special(open: TextRange) -> ParseDiagnostic { ... }
```

Every diagnostic constructor should be a named function here, never an inline
`err_builder` in a syntax module. It keeps wording consistent and makes it trivial
to review the full diagnostic surface in one file.

**Done when:** `parse("")` produces `R_ROOT` containing an empty
`R_STATEMENT_LIST` and an `EOF` token, and `parse("   \n# hi\n")` attaches the
whitespace and comment as trivia on `EOF` with no diagnostics.

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
| `[pending]` | Feature not implemented yet. Harness asserts it does **not** parse cleanly. **An unexpected clean parse is a hard failure.** The snapshot carries only `*Not yet supported.*` in place of the generated cells. |
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
// crates/aether_r_corpus/src/document.rs
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

`Document` and `check` live in `crates/aether_r_corpus/`, a small library that both
the test harness and `xtask` depend on. Keeping it out of `aether_r_parser`'s
`tests/` means `xtask` does not have to pull in that crate's dev-dependencies.

**Tests never write to disk.** nextest runs each test in its own process, in
parallel, so anything in the test path that writes files is a latent race. The
input/snapshot split makes this natural rather than a rule to remember: tests
*compare*, `xtask accept` *writes*, and the two never touch the same file. There is
no `AETHER_UPDATE_SNAPSHOTS` env var.

```rust
// crates/aether_r_parser/tests/harness/runner.rs
//
// READ-ONLY. Never writes. Regeneration lives in `xtask accept`.
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
    let has_error = parsed.has_error();
    let clean = !has_error && !has_bogus_nodes_or_empty_slots(&parsed.syntax());

    // 1. Marker contract
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

    // 2. Round-trip. Non-negotiable, every test, including error tests.
    let round_tripped = parsed.syntax().to_string();
    if round_tripped != test.source {
        return Err(CheckError::RoundTrip { expected: test.source.clone(), got: round_tripped });
    }

    // 3. Structural sanity for clean tests
    if !test.markers.contains(Markers::ERROR) {
        let ast = format!("{:#?}", parsed.tree());
        if ast.contains("missing (required)") {
            return Err(CheckError::MissingRequired(ast));
        }
        if has_bogus_nodes_or_empty_slots(&parsed.syntax()) {
            return Err(CheckError::BogusNodes(ast));
        }
    }

    // 4. Oracle
    if !test.markers.contains(Markers::DEVIATION) {
        aether_r_oracle::assert_matches(&test.source, &parsed.syntax())?;
    }

    // 5. Generate cells
    let mut cells = vec![Cell::new("cst", format!("{:#?}", parsed.tree()))];
    if !parsed.diagnostics().is_empty() {
        cells.push(Cell::new("diagnostics", render_diagnostics(&parsed, &test.source)));
    }
    Ok(cells)
}
```

`has_bogus_nodes_or_empty_slots` is copied verbatim from air's
`crates/air_r_parser/tests/spec_test.rs`.

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
the tree, so adding a file needs no registration edit. Model the macro on air's
`tests_macros::gen_tests!`, which already does this for `.R` files. Note the glob
must exclude `*.snap.md`, or every snapshot gets its own bogus test:

```rust
// crates/aether_r_parser/tests/main.rs
mod harness;

mod corpus {
    // Expands to one `#[test] fn` per matched file, with the module path
    // mirroring the directory, e.g. `corpus::expressions::extract`.
    // The macro skips any path ending in `.snap.md`.
    aether_test_macros::gen_corpus_tests! { "tests/corpus/**/*.md", crate::harness::run_file }
}
```

That naming makes nextest filters read naturally:

```bash
cargo nextest run -E 'test(corpus::expressions::extract)'
cargo nextest run -E 'test(/corpus::control/)'          # all control-flow files
cargo nextest run -p aether_r_parser --status-level all # see the pending count
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
| `expressions/specials.md` | Specials · Pipe · Pipe Placeholder | ts:e ×3 | 8 |
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
# crates/aether_r_parser/tests/corpus/MANIFEST.toml
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
precisely because nobody was happy with the current silent behavior (§7.2).

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

### 10.5 `quick_test.rs`

Keep air's scratchpad, since the owner uses it:

Run it with `just quick` (nextest needs `--run-ignored all` and `--no-capture`
to show `println!` output):

```rust
// crates/aether_r_parser/tests/quick_test.rs
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
`cargo nextest run --workspace` is green (pending tests only assert
*failure*, and everything fails at this point); and `cargo run -p xtask -- accept`
produces no diff.

---

## 11. Phases 7–13 — The parser, one feature at a time

From here on each phase is small enough to review in one sitting. The shape is
always the same: add syntax functions, add diagnostics, flip `[pending]` markers
off the tests the feature enables, regenerate, review the diff.

### 11.1 Phase 7 — Literals, identifiers, program

`src/syntax/value.rs`, `src/syntax/mod.rs`.

```rust
pub(crate) fn parse_root(p: &mut RParser) {
    let m = p.start();
    p.eat(UNICODE_BOM);
    p.with_flags(RContextFlags::NEWLINE_SENSITIVE, RContextFlags::IN_PARENS, |p| {
        parse_statement_list(p, StatementListEnd::Eof);
    });
    p.expect(T![EOF]);
    m.complete(p, R_ROOT);
}

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

/// `R_STRING_VALUE` is three tokens; the middle one needs a lex context.
fn parse_string_value(p: &mut RParser) -> ParsedSyntax {
    if !p.at(STRING_OPEN) { return Absent }
    let m = p.start();
    let delimiter = p.string_delimiter();          // from the lexer's current flags
    p.bump_with_context(STRING_OPEN, RLexContext::StringContent(delimiter));
    if p.at(STRING_CONTENT) {
        p.bump_with_context(STRING_CONTENT, RLexContext::StringEnd(delimiter));
    }
    p.expect(STRING_CLOSE);                        // diagnostic if unterminated
    Present(m.complete(p, R_STRING_VALUE))
}
```

**Enable: the whole of `corpus/literals/`** — all five files, all 19 tests. This is
the cleanest phase-to-directory mapping in the project; `cargo nextest run -E
'test(/corpus::literals/)'` should go fully green.

Three of the four error tests (Unclosed Double Quote, Unclosed Single Quote,
Unclosed Raw String) exercise the parser's `expect(STRING_CLOSE)` and its recovery.
Unclosed Backtick takes a different path entirely and exercises the *lexer's*
`unterminated_backtick_identifier` (§8.4), since a backtick-quoted name is an
`IDENT`, not a string.

That is **19 of 89 tests green** at the end of the very first parser phase, which
is a good confidence signal.

### 11.2 Phase 8 — Binary and unary operators (the Pratt core)

`src/syntax/expr.rs`. This is the heart of the parser. Take the time to get it
right; everything else hangs off it.

#### The precedence table

Transcribed from `tree-sitter-r/grammar.js`'s `PREC`, which is itself transcribed
from R's `src/main/gram.y`. Higher binds tighter.

| Rank | Name | Operators | Assoc |
|---|---|---|---|
| 1 | HELP | `?` (binary) | left |
| 2 | FUNCTION_OR_LOOP | `function`, `while`, `for`, `repeat` bodies | left |
| 3 | IF | `if` | right |
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

**Associativity of `$` `@` `::` `:::`.** tree-sitter-r forces these **right**
associative and explains why in a long comment: it is a workaround for the
`optional()` RHS, so that `foo::bar` prefers the longer match `[foo::bar]` over
`[foo::][bar]`. Since aether makes the RHS **required** (§7.2), that workaround is
unnecessary and we use **left** associativity, matching R's `gram.y`. The
resulting trees should be identical — the RHS is restricted to a selector, so
`a$b$c` can only nest one way — but say so in the PR and let the oracle confirm it.

```rust
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub(crate) struct BindingPower(u8);

#[derive(Debug, Clone, Copy)]
enum Assoc { Left, Right }

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
    let mut lhs = match parse_prefix_expression(p) {
        Present(m) => m,
        Absent => return Absent,
    };

    loop {
        // R's statement-termination rule: at top level or inside `{`, a newline
        // before an infix operator ends the expression. `1\n+2` is two
        // statements; `1 +\n2` is one. Inside `(`/`[`/`[[` newlines are eaten
        // and this check is disabled.
        if p.at_statement_break() { break }

        // Postfix `(`, `[`, `[[` bind tightest (rank 20) and are handled here,
        // not via `infix_binding_power`.
        if BindingPower(20) >= min_bp {
            if let Some(next) = parse_postfix(p, lhs) { lhs = next; continue }
        }

        let Some((bp, assoc)) = infix_binding_power(p.cur()) else { break };
        if bp < min_bp { break }

        let m = lhs.precede(p);
        let operator = p.cur();
        // Captured before the bump: `expected_selector` labels the operator
        // itself, so it needs the range while it is still current.
        let operator_range = p.cur_range();
        p.bump_any();

        // Newlines AFTER an infix operator are always insignificant — except
        // for `::` and `:::`, where tree-sitter-r explicitly notes
        // "No newlines are allowed."
        let right_bp = match assoc {
            Assoc::Left  => BindingPower(bp.0 + 1),
            Assoc::Right => bp,
        };

        let node_kind = match operator {
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

    Present(lhs)
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
- `-2:3` → `(-2):3`? **No** — `:` is 15, unary `-` is 16, so `-2:3` is `(-2):3`.
- `a <- b <- c` → `a <- (b <- c)` — right assoc.
- `a -> b -> c` → `(a -> b) -> c` — left assoc.
- `2^3^2` → `2^(3^2)` — right assoc.
- `a %in% b + c` → `a %in% (b + c)`? **No** — `%in%` is 14, `+` is 12, so
  `(a %in% b) + c`. This one trips people up; test it.

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

**Approach: lex greedily, split via re-lex.** The lexer always produces `[[` and
`]]` when it sees two adjacent brackets. When the parser is inside a single-`[`
subset and encounters `]]`, it re-lexes to split it into two `]` tokens. This is
exactly what Biome's JS parser does for `>>` inside nested generics.

```rust
// crates/aether_r_parser/src/lexer/mod.rs
pub(crate) enum RReLexContext {
    /// Split a `]]` into `]` `]`, or a `[[` into `[` `[`.
    SplitBrackets,
}

// crates/aether_r_parser/src/syntax/call.rs
fn parse_subset_arguments(p: &mut RParser) {
    let m = p.start();
    p.bump(T!['[']);
    p.with_flags(RContextFlags::IN_PARENS, RContextFlags::NEWLINE_SENSITIVE, |p| {
        parse_argument_list(p, T![']']);
    });
    if p.at(T!["]]"]) {
        // `x[a[b]]` — the inner subset must consume only one `]`.
        p.re_lex(RReLexContext::SplitBrackets);
    }
    p.expect(T![']']);
    m.complete(p, R_SUBSET_ARGUMENTS);
}
```

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
fn parse_argument_list(p: &mut RParser, end: RSyntaxKind) {
    let m = p.start();
    let mut first = true;
    loop {
        if p.at(end) || p.at(T![EOF]) { break }
        if !first { p.expect(T![,]); }
        first = false;

        // An empty argument: open and immediately close, pushing no children.
        // This is the documented technique in `r.ungram`.
        let arg = p.start();
        parse_argument_name_clause(p).ok();
        parse_expression(p).ok();
        arg.complete(p, R_ARGUMENT);
    }
    m.complete(p, R_ARGUMENT_LIST);
}

/// `name =` where name is identifier | string | `...` | `..i` | `NULL`.
///
/// `NULL` is allowed here and nowhere else — tree-sitter-r's comment notes this
/// has been true since the beginning of R (`fn(NULL = 1)` parses) and is
/// surprising because `NA`/`Inf` are not allowed, and `NULL` is not special in
/// `x$NULL` or `function(NULL) {}`. See their issue #164.
fn parse_argument_name_clause(p: &mut RParser) -> ParsedSyntax {
    let is_name = matches!(p.cur(), IDENT | STRING_OPEN | DOTS | DOTDOTI | T![null]);
    if !is_name { return Absent }
    // Two-token lookahead: only a name clause if followed by `=` (and not `==`).
    if p.nth(1) != T![=] { return Absent }
    let m = p.start();
    parse_argument_name(p).unwrap();
    p.bump(T![=]);
    Present(m.complete(p, R_ARGUMENT_NAME_CLAUSE))
}
```

The lookahead for `=` needs care with strings, which are three tokens — `nth(1)`
after `STRING_OPEN` is `STRING_CONTENT`, not `=`. Handle strings by parsing the
string first and then checking for `=`, using a parser checkpoint to rewind if it
turns out not to be a name clause. `ParserContextCheckpoint` + the token source's
checkpoint support both exist for this.

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
| `a$1` | rhs absent, `1` becomes a sibling statement, **no error** | `R_BOGUS` rhs + `expected_selector` |
| `a$NULL`, `a$TRUE`, `a$(b)`, `a${b}` | same as above | `R_BOGUS` rhs + `expected_selector` |
| `foo::` | `namespace_operator`, rhs absent, no error | missing slot + `expected_selector` |
| `foo::\nbar` | error (grammar says no newlines allowed) | error, matched |

Note the `a$1` case carefully: today air produces `right: missing (required)` *and*
a separate `RDoubleValue` sibling for the `1`, silently. Aether should consume the
`1` into a bogus RHS so it does not leak out as a phantom statement.

**Enable:** `expressions/extract.md` and `expressions/namespace.md` in full — 10
tests, of which three carry `[error, deviation]` (Dollar No RHS, Slot No RHS,
Namespace With Missing RHS) and two carry `[error]` (the `if` RHS and
Newline Before Operator cases). Air's `undefined/extract_expression_error.R` and
`undefined/namespace_expression_error.R` live in these two files as well and
become `[error]` tests.

### 11.5 Phase 11 — Function definitions

`src/syntax/function.rs`.

```rust
fn parse_function_definition(p: &mut RParser) -> ParsedSyntax {
    if !p.at(T![function]) && !p.at(BACKSLASH) { return Absent }
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

Parameters use `IN_PARENS` (newlines eaten) and a comma-separated list where each
`RParameter` is `name` + optional `RParameterDefault`. Names may be `IDENT`,
`DOTS`, or `DOTDOTI` — note `return` is a plain identifier in R and is a valid
parameter name (`Functions With return As Parameter Name`).

**Enable: the whole of `corpus/functions/`** — `definition.md`, `lambda.md`,
`parameters.md`. 7 tests, two of them `[error]`.

### 11.6 Phase 12 — Control flow

`src/syntax/control.rs`. `for`, `while`, `repeat` are mechanical. `if`/`else` is
not.

```rust
fn parse_if_statement(p: &mut RParser) -> ParsedSyntax {
    if !p.at(T![if]) { return Absent }
    let m = p.start();
    p.bump(T![if]);
    p.expect(T!['(']);
    p.with_flags(RContextFlags::IN_PARENS, RContextFlags::NEWLINE_SENSITIVE, |p| {
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

In aether this is one line, because the parser already tracks brace context:

```rust
fn parse_else_clause(p: &mut RParser) -> ParsedSyntax {
    if !p.at(T![else]) { return Absent }

    // A newline before `else` is only tolerated inside `{}`.
    if p.has_preceding_line_break() && !p.state.flags.contains(RContextFlags::IN_BRACES) {
        return Absent;   // caller finishes the `if`; `else` becomes a bogus statement
    }

    let m = p.start();
    p.bump(T![else]);
    parse_expression(p).or_add_diagnostic(p, diagnostics::expected_expression);
    Present(m.complete(p, R_ELSE_CLAUSE))
}
```

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

**Enable: the whole of `corpus/control/`** — `if.md`, `for.md`, `while.md`,
`repeat.md`. 11 tests. Note the two `#200` `else`-prefix identifier tests live in
`if.md` rather than `literals/identifiers.md`; see §10.3.2.

### 11.7 Phase 13 — Blocks, statement lists, semicolons

`src/syntax/block.rs`. This lands the semicolon deviation.

```rust
fn parse_braced_statements(p: &mut RParser) -> ParsedSyntax {
    if !p.at(T!['{']) { return Absent }
    let m = p.start();
    p.bump(T!['{']);
    p.with_flags(
        RContextFlags::NEWLINE_SENSITIVE | RContextFlags::IN_BRACES,
        RContextFlags::IN_PARENS,
        |p| parse_statement_list(p, StatementListEnd::RCurly),
    );
    p.expect(T!['}']);
    Present(m.complete(p, R_BRACED_STATEMENTS))
}

fn parse_statement_list(p: &mut RParser, end: StatementListEnd) {
    let m = p.start();
    while !p.at(T![EOF]) && !end.matches(p.cur()) {
        let progress_before = p.source().position();

        let stmt = p.start();
        let had_expression = parse_expression(p).is_present();
        let had_semicolon = p.eat(T![;]);
        stmt.complete(p, R_STATEMENT);

        if !had_expression && !had_semicolon {
            // Nothing consumed — recover so we cannot loop forever.
            let recovered = ParseRecoveryTokenSet::new(
                R_BOGUS_EXPRESSION,
                token_set![T![;], T!['}'], T![EOF]],
            )
            .enable_recovery_on_line_break()
            .recover(p);
            if recovered.is_err() { break }
            p.error(diagnostics::expected_expression(p, p.cur_range()));
        }

        debug_assert!(p.source().position() > progress_before, "parser made no progress");
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

**Enable: the whole of `corpus/statements/`** — `braces.md` and `semicolons.md`
(`newlines.md` went green back in Phase 8). Every test in `semicolons.md` comes
from air's snapshots rather than tree-sitter and carries `[deviation]`.

---

## 12. Phase 14 — Recovery polish and air integration

### 12.1 Recovery review pass

By this point every feature is in, and recovery has been added ad hoc. Do a
dedicated pass:

- Every `parse_*` that can fail has a recovery set that includes the enclosing
  construct's closing token, plus `T![;]`, plus `enable_recovery_on_line_break()`
  wherever a newline genuinely bounds the construct.
- Every bogus node has at least one diagnostic. Add a debug assertion in the
  harness: if the tree contains a bogus node and `diagnostics` is empty, fail.
  Air's `has_bogus_nodes_or_empty_slots` exists precisely for this and its comment
  says so: *"used in the parser test to ensure it doesn't emit bogus nodes without
  diagnostics."*
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
the number in the PR. If it is *slower*, the likely culprits are (a) `Vec<Trivia>`
reallocation — pre-size it, and (b) excessive checkpointing in the argument-name
lookahead.

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
5. **Update the LSP** to surface per-diagnostic ranges instead of one whole-file
   error. This is the most user-visible win of the whole project — call it out in
   the changelog.
6. **Final acceptance gate:** air's entire formatter snapshot suite is unchanged
   except for semicolon-related files.

**Optional follow-up, separate PR.** If the owner wants air's imports to say
`aether_r_syntax` rather than relying on the Cargo rename, do a mechanical
find-and-replace across air *after* the swap lands. Keeping it separate means the
swap PR's diff stays reviewable.

---

## 13. Risk register

| Risk | Why it bites | Mitigation |
|---|---|---|
| **Trivia attachment drift** | A one-piece difference in whitespace attachment changes every snapshot and silently changes formatter output. | Port air's rules literally; the oracle compares ranges, so it catches this. Do the token source before any grammar work. |
| **`[[` / `]]` splitting** | Genuinely ambiguous; tree-sitter needs an external scanner and still has open bugs. | Greedy lex + re-lex split (§11.3). Six corpus tests exercise it. Do not invent a scope stack. |
| **The statement wrapper** | Replaces `R_EXPRESSION_LIST` with `R_STATEMENT_LIST` of `R_STATEMENT`, so air's formatter must change. | Land in Phase 3, before any parser code. Get explicit sign-off on the ungram shape. |
| **Codegen drift** | If aether's generated code diverges from air's, the swap-in fails in confusing ways. | Phase 2 asserts byte-identity, and CI asserts `just codegen` idempotence. |
| **Oracle unavailable on error tests** | tree-sitter bails on any error, so ~25 tests get no oracle coverage. | Accept it; those are exactly the tests where we *intend* to differ. Rely on reviewed snapshots there. |
| **`[pending]` rot** | A large batch of inert tests invites ignoring them. | Unexpected pass is a *hard failure*, so the count only moves deliberately. Report `N/89 enabled` in CI output. |
| **Biome fork rev skew** | Air's fork could move mid-project. | Pin, and treat a rev bump as its own PR with a full snapshot regeneration. |

---

## 14. `CLAUDE.md` for the aether repo

Create this in Phase 1 so every subsequent session has the same context:

```markdown
# Aether

A hand-written Rust parser for R, built on Biome's parser infrastructure,
producing the same Rowan tree as `posit-dev/air`'s tree-sitter-based parser.

## Commands
Tests run under **cargo-nextest**, not `cargo test`.
- `just codegen` — regenerate `aether_r_syntax` / `aether_r_factory` from `r.ungram`
- `just test` — `cargo nextest run --workspace` (+ `cargo test --doc`)
- `just accept` — regenerate markdown snapshots via `xtask accept`, then verify
- `just quick` — run the `tests/quick_test.rs` scratchpad
- `cargo nextest run -E 'test(/Dollar/)'` — filter by name

Tests are **read-only**. Only `xtask accept` writes corpus files; nextest runs
tests in parallel processes and they would race otherwise.

## Invariants — do not break these
1. `parse(src).syntax().to_string() == src` for ALL inputs. Always.
2. Bogus nodes always come with at least one diagnostic.
3. The parser never panics and never loops forever.
4. Tree shape matches tree-sitter-r except where `docs/deviations.md` says so.

## Test corpus
`crates/aether_r_parser/tests/corpus/**/*.md`, one file per concept, one test per
`##` heading.

**Two files per concept.** `X.md` is human-authored: headings, markers, and ```r
cells. `X.snap.md` is generated by `just accept` and must never be hand-edited —
it echoes the heading and source, then adds ```cst and ```diagnostics. If you
change a test, edit `X.md` and re-run `just accept`.

`MANIFEST.toml` records where each of the 89 tree-sitter-r tests lives; a test
enforces that none go missing. Moving or renaming a test means editing both the
input `.md` and the manifest.
The ` ```r ` cell is the input and is human-authored. Everything else is
generated — never hand-edit a `cst` or `diagnostics` cell.

Markers: `[pending]` (not yet supported; must NOT parse cleanly),
`[error]` (must produce an error), `[deviation]` (skip oracle comparison).

**Turning on a feature means deleting `[pending]` markers, not adding tests.**
The 89 tree-sitter-r tests were imported wholesale in one commit.

## Reference implementations
- `tree-sitter-r` (r-lib) — especially `src/scanner.c` and the long comments in
  `grammar.js`. Those comments record real bugs; read them before changing
  lexer behavior.
- Biome's JS/CSS parsers — for idiom. When in doubt, do what `biome_js_parser`
  does.
```

---

## 15. Open questions for the owner

The grammar shape is fully settled (§7), so nothing here blocks Phase 3. These are
decisions to make before the phase each one names — but do not let them go
unanswered until swap-in.

1. **Formatter semantics for semicolons.** At swap time, should the formatter
   `format_removed()` the semicolon and insert a hard line break (i.e. `1;2`
   becomes two lines), or preserve it? This does not block the parser but shapes
   the deviation doc.
2. **Cargo package rename vs. find-and-replace.** At swap-in, is the
   `air_r_syntax = { package = "aether_r_syntax", … }` rename acceptable
   (recommended — keeps the swap PR's diff to a few `Cargo.toml` lines plus the
   formatter changes), or do you want air's imports renamed outright in the same
   PR?
3. **`a$1` recovery shape.** Consume the `1` into a bogus RHS (recommended, no
   phantom sibling statement), or leave it as a sibling the way tree-sitter does?
4. **Repo visibility and CI budget.** The oracle requires building tree-sitter-r
   (a C dependency) in CI. Acceptable, or should the oracle be a separate
   scheduled job?
