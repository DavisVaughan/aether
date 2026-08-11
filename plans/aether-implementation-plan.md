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
    │       ├── parse.rs           # public `parse()` entry point
    │       ├── options.rs
    │       ├── diagnostics.rs     # expected_* diagnostic constructors
    │       ├── token_source.rs    # RTokenSource
    │       ├── parser.rs          # RParser + context flags
    │       ├── lexer.rs           # RLexer; ends with `#[cfg(test)] mod tests;`
    │       ├── lexer/
    │       │   ├── tests.rs       # the assert_lex! family + `mod` declarations
    │       │   └── tests/         # one file per dispatch arm — see §8.6
    │       │       ├── bom.rs
    │       │       ├── whitespace.rs
    │       │       ├── comments.rs
    │       │       ├── line_breaks.rs
    │       │       ├── puncts.rs
    │       │       ├── brackets.rs
    │       │       ├── keywords.rs
    │       │       ├── identifiers.rs
    │       │       ├── backticks.rs
    │       │       ├── numbers.rs
    │       │       ├── dots.rs
    │       │       ├── specials.rs
    │       │       ├── strings.rs
    │       │       ├── raw_strings.rs
    │       │       ├── checkpoint.rs
    │       │       └── coverage.rs
    │       ├── syntax.rs
    │       └── syntax/
    │           ├── expr.rs        # Pratt loop, primary exprs
    │           ├── call.rs        # call / subset / subset2 / arg lists
    │           ├── function.rs    # function defs, lambdas, parameters
    │           ├── control.rs     # if/else, for, while, repeat
    │           ├── block.rs       # braced exprs, statement lists, semicolons
    │           └── value.rs       # literals, strings, keywords-as-values
    ├── aether_r_test_macros/      # proc-macro: `gen_corpus_tests!` (§10.2)
    │   └── src/lib.rs             #   modelled on air's `tests_macros`
    ├── aether_r_parser_tests/     # every corpus test, plus the harness that
    │   ├── src/                   #   runs them. Depends on the parser; nothing
    │   │   ├── document.rs        #   depends on it except `xtask`.
    │   │   ├── check.rs           # run one test, produce its cells (§10.2)
    │   │   └── runner.rs          # `run_file`: read-only compare; panics on
    │   │                          #   mismatch, for use from a `#[test]`
    │   └── tests/
    │       ├── main.rs            # globs corpus/**/*.md → crate::run_file
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
3. Run `just oracle` and confirm the diff is clean for all non-`[deviation]`
   tests. This is a deliberate local step — the oracle is feature-gated and CI
   does not run it (§10.4.1).

| Phase | Title | Deliverable |
|---|---|---|
| 1 | Repo bootstrap | Workspace builds, CI green, nothing else |
| 2 | Vendor + rename syntax layer | `just codegen` reproduces `aether_r_syntax` + `aether_r_factory`, identical to air's modulo the rename |
| 3 | Grammar deviations | `.ungram` + kinds updated, regenerated, documented |
| 4 | Lexer | Standalone `RLexer`, plus sixteen `#[cfg(test)]` token-stream test files under `src/lexer/tests/` (§8.6) |
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
members = ["crates/*", "xtask", "xtask/codegen"]
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

aether_r_factory      = { path = "./crates/aether_r_factory" }
aether_r_oracle       = { path = "./crates/aether_r_oracle" }   # optional, see §10.4.1
aether_r_parser       = { path = "./crates/aether_r_parser" }
aether_r_parser_tests = { path = "./crates/aether_r_parser_tests" }
aether_r_syntax       = { path = "./crates/aether_r_syntax" }
aether_r_test_macros  = { path = "./crates/aether_r_test_macros" }

bitflags      = "2.6"
enumflags2    = "0.7"
proc-macro2   = "1.0"          # aether_r_test_macros only
quote         = "1.0"          # aether_r_test_macros only
rustc-hash    = "2.0"
serde         = { version = "1.0", features = ["derive"] }
similar       = "2.6"          # unified diffs in harness failure output
tracing       = "0.1"
# Only ever built under `--features oracle` (§10.4.1). Deleted at swap-in.
tree-sitter   = "0.24.7"
tree-sitter-r = { git = "https://github.com/r-lib/tree-sitter-r", rev = "d459b97a9a11dd81643ab674938bede2874663b0" }
unicode-ident = "1.0"

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

# Differential check against tree-sitter-r. Local only; CI does not run this,
# and the whole crate is deleted at swap-in.
oracle:
    cargo nextest run -p aether_r_parser_tests --features oracle

# Run the scratchpad in tests/quick_test.rs
quick:
    cargo nextest run -p aether_r_parser_tests --run-ignored all -E 'test(quick_test)' --no-capture
```

Note `just test` deliberately omits `--features oracle`: the default test run
builds no C dependency and stays fast, and the oracle is invoked on purpose at the
end of each phase.

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

**No oracle job, deliberately** (§10.4.1). `--features oracle` appears nowhere
above, so CI never builds tree-sitter-r or its C toolchain. The oracle is a local
development aid with a scheduled death; paying for it on every push, and keeping
it green as a merge gate, would be spending real budget on a crate we plan to
delete.

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
-        "R_BRACED_EXPRESSIONS",
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
  hop and a `format_removed(semicolon)`. **Settled with the owner:** the
  semicolon is removed and replaced by a hard line break, so `1;2` formats as two
  lines. See §12.3 step 4 for the two comment-placement rules this forces.

**Two alternatives were considered and rejected.** Keeping semicolons as trivia
under a dedicated `TriviaPieceKind` is not possible at all — `TriviaPieceKind` is
a Biome enum and aether does not fork it. Emitting `;` as a real token while
keeping a list of bare expressions, with the `;` as a slot on each expression
node, works but requires touching every expression node in the grammar. The
wrapper is cleaner and is what Phase 3 lands.

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
  a valid selector: wrap it in a bogus node and emit `expected_selector` with the
  offending range labelled. **Settled with the owner:** the bogus node consumes
  exactly one atom — no postfix, no infix — so `a$1 + 2` still parses the `+`
  normally. §11.4 has the full boundary table. The bogus node's kind is
  `R_BOGUS_SELECTOR`, a new node kind — see §7.2.1.

Same treatment for `@`, `::`, `:::`.

#### 7.2.1 `AnyRSelector` needs a bogus variant

Checked against air's generated code, and this does **not** work out of the box.
`AnyRSelector` has no bogus variant:

```rust
// air/crates/air_r_syntax/src/generated/nodes.rs
pub enum AnyRSelector {
    RDotDotI(RDotDotI),
    RDots(RDots),
    RIdentifier(RIdentifier),
    RStringValue(RStringValue),
}
```

and the generated factory does not panic when the `right` slot fails to cast — it
does something quieter and worse:

```rust
// air/crates/air_r_factory/src/generated/syntax_factory.rs, R_EXTRACT_EXPRESSION
if current_element.is_some() {
    return RawSyntaxNode::new(
        R_EXTRACT_EXPRESSION.to_bogus(),
        children.into_iter().map(Some),
    );
}
```

`to_bogus()` maps `R_EXTRACT_EXPRESSION` to `R_BOGUS_EXPRESSION`, so `a$1` would
collapse the **whole** extract expression into a single bogus node rather than
producing an `R_EXTRACT_EXPRESSION` with a bogus RHS. Round-trip still passes, so
nothing in the harness would catch it; it would only show up by reading the
snapshot.

The fix is a **new bogus node kind, `RBogusSelector`**, landed in Phase 3 with the
other grammar deviations. Two edits to `r.ungram`:

```diff
 RBogus = SyntaxElement*
 RBogusValue = SyntaxElement*
 RBogusExpression = SyntaxElement*
+RBogusSelector = SyntaxElement*

 AnyRSelector =
 	RIdentifier
 	| RStringValue
 	| RDots
 	| RDotDotI
+	| RBogusSelector
```

and one to `r_kinds_src.rs`:

```diff
         "R_BOGUS",
         "R_BOGUS_VALUE",
         "R_BOGUS_EXPRESSION",
+        "R_BOGUS_SELECTOR",
```

Adding a kind means the two hand-written `RSyntaxKind` matches in
`aether_r_syntax/src/lib.rs` need arms — they are exhaustive, so the compiler
demands it. `is_bogus()` returns `true`, and `to_bogus()` is idempotent, exactly
as `R_BOGUS_VALUE` is:

```rust
// Bogus selector
RSyntaxKind::R_BOGUS_SELECTOR => RSyntaxKind::R_BOGUS_SELECTOR,
```

Note what does *not* change: the four selector node kinds keep their existing
`to_bogus()` mappings. `to_bogus()` answers "this node kind failed to build, what
does it degrade to", which is about the node being built, not the slot it sits in
— and `R_IDENTIFIER` or `R_STRING_VALUE` appear in plenty of non-selector
positions. Only the parser decides to build an `R_BOGUS_SELECTOR`, and it only
does so in a selector slot.

**Why a new kind rather than reusing `RBogusValue`.** Biome's convention is one
bogus kind per restricted position, and `js.ungram` has ten of them —
`JsBogusBinding`, `JsBogusParameter`, `JsBogusMember`, `JsBogusAssignment`,
`JsBogusImportAssertionEntry`, `JsBogusNamedImportSpecifier`, `TsBogusType`, and
the rest. `AnyJsDecorator` is the one place Biome reuses a broader bogus kind
(`JsBogusExpression`) in a restricted union, and it is the exception. Two concrete
payoffs here:

- **The name does not lie.** The bogus RHS wraps one whole prefix atom (§11.4), so
  `a${b}` wraps `R_BRACED_STATEMENTS`, `a$(b)` wraps `R_PARENTHESIZED_EXPRESSION`,
  and `a$-1` wraps `R_UNARY_EXPRESSION`. None of those is a value. Snapshots are
  the artifact reviewers read most, and they should read truthfully.

- **The kind alone identifies the position.** A tree walker — air's LSP, a lint
  rule — seeing `R_BOGUS_SELECTOR` knows a selector was malformed without
  inspecting the parent. `R_BOGUS_VALUE` would have been ambiguous with a
  malformed literal.

`R_BOGUS_SELECTOR` is also disjoint from `AnyRExpression`, where `R_BOGUS_VALUE`
is a member via `AnyRValue`. That makes `RExtractExpression`'s `left`
(`AnyRExpression`) and `right` (`AnyRSelector`) slots unable to accept the same
bogus kind. Not load-bearing — the required operator token between them already
anchors the factory's positional scan, verified by driving `RSyntaxFactory`
directly for both orderings — but it costs nothing to keep them disjoint.

**Swap-in cost**, since this is one more kind than air has: air's `RSyntaxKind`,
`nodes.rs`, and both factories are generated, so they follow from air's own
codegen. The hand-written touch points are air's `is_bogus()` and `to_bogus()` in
`air_r_syntax/src/lib.rs`, and one formatter rule file. The latter is nearly free
— air's bogus rules are empty impls that its codegen stubs out:

```rust
impl FormatBogusNodeRule<RBogusSelector> for FormatRBogusSelector {}
```

`FormatAnyRSelector` in `air_r_formatter/src/r/any/selector.rs` gains a fifth arm
in the same generated pass.

This means `tests/snapshots/undefined/extract_expression_error.R` and
`namespace_expression_error.R` become *error* tests in aether, and their corpus
counterparts (`Dollar No RHS`, `Slot No RHS`, `Namespace With Missing RHS`) get
marked `[deviation]` so the oracle skips them.

### 7.3 The deviation ledger

Create `docs/deviations.md` in aether and keep it current. Every intentional
difference from tree-sitter-r goes here with an example, the tree-sitter output,
the aether output, and the rationale. The oracle reads nothing from this file, but
reviewers will, and so will whoever updates air's formatter.

Give it **three** headings. The first is aether's own deviations. The second,
*"tree-sitter-r vs. R"*, records places where tree-sitter-r already differs from R
and aether follows tree-sitter for oracle agreement — see §8.4 for two concrete
ones (`%foo` at EOF, and backslashes inside `%…%`). The third, *"Inherited from
air"*, records air conventions aether copies without endorsing, so they are
findable when the swap is done and snapshots can be regenerated in one deliberate
pass. Keeping the categories apart matters: the first is a set of choices we made
and can revisit, the second is upstream behaviour we are deliberately not fixing,
and the third is a to-do with a natural date.

Seed the third with the one entry we know of:

> **Trivia attachment polarity.** Air attaches whitespace between two same-line
> tokens as *leading* of the following token; Biome's own parsers attach it as
> *trailing* of the preceding one. Air also makes the same
> whitespace-before-a-newline trailing or leading depending on whether any token
> follows later in the file (`1 + \n1` vs `1  \n `). Aether copies both, because
> deviating would change every snapshot and every oracle comparison for no
> user-visible gain — the formatter regenerates whitespace regardless, and
> round-trip holds either way (§9.1).
>
> The JS-shaped simplification, if this is ever revisited, is `JsTokenSource`'s
> whole body: `let mut trailing = !first;` and, in a single streaming pass,
> `if kind.is_newline() { trailing = false; }` before pushing each piece. No gap
> buffer, no one-piece lookahead, no `between_two_tokens`, and the three special
> cases collapse — whitespace and comments are trailing of the preceding token
> until a newline is seen, then leading. That is roughly forty lines shorter than
> §9.1's loop and states in one sentence. The cost of adopting it is a
> whole-corpus snapshot regeneration, which is cheap *after* the swap and
> expensive before it, since it would mask exactly the drift the oracle is
> watching for.

Seed the first with:

| # | Deviation | tree-sitter-r | aether |
|---|---|---|---|
| 1 | Semicolons | trivia, invisible | `SEMICOLON` token in `RStatement`; formatter removes it and hard-breaks the line, so `1;2` → two lines |
| 1b | Statement list | `R_EXPRESSION_LIST` of expressions | `R_STATEMENT_LIST` of `R_STATEMENT` |
| 1c | Braces | `R_BRACED_EXPRESSIONS` | `R_BRACED_STATEMENTS` |
| 2 | Extract RHS | `optional()`, silent | required, diagnostic + bogus |
| 3 | Namespace RHS | `optional()`, silent | required, diagnostic + bogus |
| 4 | Error recovery | whole-file bogus node | localized bogus nodes, tree still walkable |
| 5 | `AnyRSelector` | 4 variants, no bogus | new `RBogusSelector` kind added as a fifth variant, so a malformed RHS is contained in the `right` slot instead of collapsing the whole expression (§7.2.1) |

**Done when:** `.ungram` and `r_kinds_src.rs` updated, `just codegen` run, generated
code compiles, `docs/deviations.md` seeded, and the delta vs air's generated code is
*only* these two sets of changes:

- the statement list — `R_EXPRESSION_LIST` replaced by `R_STATEMENT_LIST`,
  `R_STATEMENT` added, and `RRoot`/`RBracedStatements` pointing at the new list
  type

- the bogus selector — `R_BOGUS_SELECTOR` added as the last kind before `__LAST`
  so nothing renumbers, `AnyRSelector::can_cast` accepting it, and arms in
  `is_bogus()` and `to_bogus()`

Phase 4 adds a third set — `ERROR_TOKEN` (§8.4, deviation 6) — so this is the
delta as of *this* phase, not a standing invariant. Keep the list growing here as
kinds land.

---

## 8. Phase 4 — The lexer

`crates/aether_r_parser/src/lexer.rs`. This phase produces a **standalone,
independently testable** lexer. No parser yet.

The lexer is one module, `lexer.rs`, with a `#[cfg(test)] mod tests;` at the
bottom. Its tests live under `lexer/` and are therefore descendants of the module
they test, which is the whole reason they sit here rather than in a test crate:
they read private fields (`after_newline`, `string_delimiter`) and call private
helpers directly, and `#[cfg(test)]` keeps every byte of them out of the parser
air links against. §8.6 is the test plan and it is not an afterthought — the lexer
is the one component whose bugs every later phase inherits silently.

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
free via `BufferedLexer`. Note that `nth` lexes ahead in `RLexContext::default()`,
so it is only meaningful for tokens whose lexing is context-free — it cannot be
used to peek past a `STRING_OPEN`. That constraint is why §11.3 parses argument
names optimistically rather than looking ahead for the `=`.

### 8.2 Skeleton

```rust
// crates/aether_r_parser/src/lexer.rs
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
    Raw { quote: u8, hyphens: u8, bracket: RawStringBracket },
}
// NOTE: there is deliberately no `Backtick` variant. See §8.4 — a backtick-quoted
// name is an IDENT, not a string.

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum RawStringBracket { Paren, Bracket, Brace }

/// The re-lexing requests the parser can make.
///
/// Only one exists, and by §8.4 only one ever should: `[[` and `]]` are lexed
/// greedily, and the parser splits them back apart at its single split site
/// (§11.3). Modelled as an enum rather than a unit type because
/// [`biome_parser::lexer::ReLexer`] requires an associated `ReLexContext`, and
/// because a second variant would be a design change worth seeing in a diff.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum RReLexContext {
    /// Re-lex `[[` as `[`, or `]]` as `]`, leaving the second bracket to be
    /// lexed as the next token. A no-op on any other current token.
    SplitBrackets,
}

pub(crate) struct RLexer<'src> {
    source: &'src str,
    position: usize,
    current_kind: RSyntaxKind,
    current_start: TextSize,
    current_flags: TokenFlags,
    /// Sticky across `next_token` calls: set by `consume_newline`, cleared only
    /// once a *non-trivia* token has been stamped with it. This is what lets the
    /// lexer report a preceding line break even though `NEWLINE` is a token in
    /// its own right (§8.5), and it is exactly `biome_js_parser`'s mechanism.
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

        // Stamp the flag AFTER lexing, so `lex_token` may set other flags first,
        // and use `set` on the single bit rather than assigning the whole set.
        self.current_flags
            .set(TokenFlags::PRECEDING_LINE_BREAK, self.after_newline);
        self.current_kind = kind;

        // The bit survives across trivia. `consume_newline` sets it; the WHITESPACE
        // and COMMENT tokens that may follow leave it alone; the first non-trivia
        // token consumes it. So `1\n  # c\n  + 2` reports the line break on `+`.
        if !kind.is_trivia() {
            self.after_newline = false;
        }

        debug_assert!(self.current_start <= TextSize::from(self.position as u32));
        kind
    }

    fn has_preceding_line_break(&self) -> bool {
        self.current_flags.contains(TokenFlags::PRECEDING_LINE_BREAK)
    }

    /// Overridden solely to maintain `after_newline`; the provided body cannot,
    /// since it has no access to this type's fields. `biome_js_parser` overrides
    /// it for exactly the same reason and with the same body.
    ///
    /// Inheriting the provided `consume_newline` also gets `\r\n` treated as a
    /// single newline for free, which is what §8.5 requires.
    fn consume_newline_or_whitespaces(&mut self) -> RSyntaxKind {
        if self.consume_newline() {
            self.after_newline = true;
            NEWLINE
        } else {
            self.consume_whitespaces();
            WHITESPACE
        }
    }

    fn has_unicode_escape(&self) -> bool { false } // R has none at token level
    // ... trivial accessors
}
```

Two obligations follow from that field, both easy to miss:

- **`after_newline` is set by overriding `consume_newline_or_whitespaces`.** The
  trait provides that method, but the provided body cannot set a field on the
  concrete lexer, so JS overrides it to add one line and aether does the same.
  Dispatch newline and whitespace through this single method (§8.3) rather than
  splitting them: `consume_newline` is trait-provided, returns `bool` rather than
  a kind, and already handles `\r\n` as one unit. If R's whitespace set turns out
  to differ from the provided `consume_whitespaces`, override *that* — the
  dispatch stays one arm either way.
- **`checkpoint()` and `rewind()` must carry it.** `LexerCheckpoint` has a
  dedicated `after_line_break: bool` field for exactly this, alongside
  `current_flags`. The two are not redundant: `current_flags` records what was
  stamped on the checkpointed token, while `after_line_break` is the sticky state
  lexing resumes from. Restore both, or a rewind across a newline silently loses
  the break.

`LexerCheckpoint` is a concrete struct in `biome_parser`, not an associated type,
and its full field list at the pinned rev is:

```rust
pub struct LexerCheckpoint<Kind> {
    pub position: TextSize,
    pub current_start: TextSize,
    pub current_kind: Kind,
    pub current_flags: TokenFlags,
    pub after_line_break: bool,
    pub unicode_bom_length: usize,
    pub diagnostics_pos: u32,
}
```

So **`unicode_bom_length` is not optional for aether even though nothing in R
reads it.** In JS it exists for `read_shebang`, which checks `start != 0 && start
!= self.unicode_bom_length` so that a shebang after a BOM is still a shebang; R
has no shebang and no second reader. `RLexer` still needs the field, because
`checkpoint()` must construct the struct and `rewind()` destructures it — and
`JsLexer` sets it from `consume_potential_bom`'s returned length (§8.3.1). Set it
the same way rather than hardcoding `0`: it costs one line, and a `0` there is a
lie that the next person has to re-derive.

### 8.3 `lex_token` dispatch

Order matters. Copy the ordering decisions from tree-sitter-r's `grammar.js`
comments — several of them exist specifically to fix filed bugs.

```rust
fn lex_token(&mut self) -> RSyntaxKind {
    let byte = self.current_byte();
    match byte {
        // One arm for both, as in `biome_js_parser`. The trait-provided
        // `consume_newline` inside it already treats `\r\n` as a single newline
        // (§8.5), and the override in §8.2 is what sets `after_newline`.
        b'\n' | b'\r' | b'\t' | b' ' | b'\x0B' | b'\x0C'
                               => self.consume_newline_or_whitespaces(),
        b'#'                   => self.consume_comment(),

        b'"'  => self.consume_string_open(RStringDelimiter::Double),
        b'\'' => self.consume_string_open(RStringDelimiter::Single),
        b'`'  => self.consume_backtick_identifier(),

        b'0'..=b'9' => self.consume_number(),

        // `.` is overloaded: `...`, `..1`, `.5` (a number!), `.foo` (identifier).
        // tree-sitter-r has an explicit note: an identifier may start with `.`
        // only if the `.` is NOT followed by a digit, otherwise `.1i` and `.1L`
        // get misparsed as identifiers (their issue #190).
        b'.' => self.consume_dot_dispatch(),

        b'%' => self.consume_special(),

        // `r"(...)"` / `R'---[...]---'` share this arm with every other name,
        // falling through to the identifier rule when the prefix does not match
        // — see below for why they do not get an arm of their own.
        b'_' | b'a'..=b'z' | b'A'..=b'Z' => match self.scan_raw_string_open() {
            Some(open) => self.consume_raw_string_open(open),
            None => self.consume_identifier_or_keyword(),
        },

        // Maximal munch over the operators, inline in this same match rather
        // than behind a `consume_punct` — one arm per leading byte, nesting a
        // second match on `peek_byte` where the byte is a prefix of something
        // longer. The full table is in §8.4.
        b';' => self.eat_byte(T![;]),
        // …
        b'<' => match (self.peek_byte(), self.byte_at(2)) {
            (Some(b'<'), Some(b'-')) => self.eat_bytes(3, T![<<-]),
            (Some(b'-'), _)          => self.eat_bytes(2, T![<-]),
            (Some(b'='), _)          => self.eat_bytes(2, T![<=]),
            _                        => self.eat_byte(T![<]),
        },
        // …

        // The unicode arm, and also where the BOM is resolved — see §8.3.1
        b if b >= 128 => self.consume_unicode(),

        _ => self.consume_unexpected(),
    }
}
```

Keep this one match. The punct arms are a third of it and the temptation to
extract them is real, but the dispatch reads as a single decision on the leading
byte and splitting it means holding two orderings in your head — and the order
is load-bearing, per the note above.

**Do not give `r`/`R` an arm of its own.** It is the obvious way to write the
raw-string check and it costs the whole match its jump table. LLVM compiles this
`match` to one range check plus an indirect jump, but the range has to be
contiguous: a `b'r' | b'R'` arm ahead of `b'a'..=b'z'` splits the letters, the
table shrinks to `\t..=r`, and every byte from `s` up — eight common lowercase
letters, plus `{`, `|`, `}`, `~` — falls out to a comparison chain instead. In R
source that is a large fraction of all bytes. Measured on 2.3 MB of R with a
skeleton of this dispatch, the split arm costs about 14%, which is also more than
switching the whole match to `biome_unicode_table::lookup_byte` would win back
(that pays an extra dependent load, and measured ~4% *slower* than this version).

Verify with `cargo rustc --release -p aether_r_parser --lib -- --emit asm` and
look for the range check ahead of the `LJTI` reference in `next_token`: it should
read `sub #9` / `cmp #117`, covering `\t..=~`.

`scan_raw_string_open` therefore checks for `r`/`R` itself and answers `None` for
every other identifier start, rather than asserting the caller already looked.

#### 8.3.1 The BOM

`RRoot` has a `bom: 'UNICODE_BOM'?` slot, so `UNICODE_BOM` has to be produced or
that slot is unreachable. **Handle it exactly the way `biome_js_parser` does**, in
the `>= 128` arm rather than in an arm of its own:

```rust
/// The `byte >= 128` arm, which is also where the BOM is resolved.
fn consume_unicode(&mut self) -> RSyntaxKind {
    // A BOM is only a BOM at offset 0. Elsewhere U+FEFF is category Cf, so
    // neither XID_Start nor XID_Continue, and it falls to `consume_unexpected`
    // below like any other character that starts nothing.
    if self.position == 0 {
        if let Some((bom, length)) = self.consume_potential_bom(UNICODE_BOM) {
            self.unicode_bom_length = length;
            return bom;
        }
    }

    if is_xid_start(self.current_char_unchecked()) {
        self.consume_identifier_or_keyword()
    } else {
        self.consume_unexpected()
    }
}
```

Both this arm and the match's final `_` end at `consume_unexpected`, which is
§8.4's `ERROR_TOKEN` case.

Four things about `consume_potential_bom`, all verified against
`biome_parser/src/lexer.rs` at the pinned rev rather than assumed:

- **It is trait-provided.** Do not hand-roll a peek-then-consume pair. It takes
  the kind to return, so the call is `self.consume_potential_bom(UNICODE_BOM)`.

- **It is safe to call speculatively.** The body advances by `bom.len()` *before*
  matching, but `Bom::Null.len()` is `0`, so a non-match advances nothing. That is
  why no guard beyond `position == 0` is needed.

- **It works on `source().get(0..3)`, a `str` slice, not on raw bytes.** A source
  shorter than three bytes, or one whose first three bytes are not a char
  boundary, yields `None` and is skipped. This is why matching on `0xEF` yourself
  is both redundant and more fragile than it looks.

- **It returns the byte length, and JS stores it** in a `unicode_bom_length`
  field. Its only JS reader is `read_shebang`, which R has no analogue of — but
  see §8.2, because the field is not optional for us.

#### 8.3.2 Unicode whitespace is deliberately not handled here

`biome_js_parser`'s `>= 128` arm also routes unicode line breaks and unicode
spaces (NBSP, en quad, ideographic space) to `consume_newline_or_whitespaces`,
short-circuited by a `UNICODE_WHITESPACE_STARTS` table. Aether's arm above does
not, because tree-sitter-r's whitespace set is ASCII-only and the oracle is the
thing we are matching. If that turns out to be wrong, it is a change to this one
function — but do not add it speculatively, and if it is added, it belongs in
`docs/deviations.md` under "tree-sitter-r vs. R".

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
corpus. **Aether lexes greedily: two adjacent brackets always produce a single
`[[` or `]]` token.** The *parser* splits them back apart where the nesting
demands it, via `p.re_lex(RReLexContext::SplitBrackets)` — the same technique
Biome's JS parser uses for `>>` inside nested generics. Do not carry a scope
stack in the lexer. There is exactly **one** split site — the argument list's
terminator check, `ArgumentListEnd::split_and_check` — and §11.3 explains why it
has to be there and nowhere else, with the `x[a[b]]` worked example. This is the
single most important design call in the lexer and the one most likely to be got
wrong.

**Keywords.** `function for in while repeat if else next break TRUE FALSE NULL Inf
NaN NA NA_integer_ NA_real_ NA_complex_ NA_character_`.
Lex as `IDENT` first, then map the text to a keyword kind — the standard Biome
approach, and it automatically gets the `else_idx` case right (tree-sitter-r
issue #200: `elseμ <- 1` must not see an `else` keyword).

**Do not reach for the generated `RSyntaxKind::from_keyword()` to do that
mapping.** Checked against `aether_r_syntax/src/generated/kind.rs`: codegen
derives it from the `keywords` array in `r_kinds_src.rs`, which spells the R
constants in lowercase placeholder form, so `from_keyword` matches `"true"`,
`"na_integer"`, `"na_double"` — none of which is R source text. `from_keyword("TRUE")`
is `None`. The lexer needs its own literal table, and the two spellings do not
line up one-for-one:

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

`keywords.rs` (§8.6) pins every row of that table, `NA_real_ → NA_DOUBLE_KW`
above all, since it is the one an implementer will get wrong by pattern-matching
on the name.

One more generated-code landmine while you are here: the `T!` macro has a
`[#] => HASH` arm, but `RSyntaxKind` has no `HASH` variant — the arm only fails to
compile if something invokes it. Comments are `COMMENT`; there is no `#` token.
Do not write `T![#]`.

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

R agrees, and deliberately so. `token()` dispatches `` ` `` to
`StringValue(c, forSymbol=true)` — the *same* function that lexes strings — and
that function turns a newline into an escaped newline rather than an error, under
a comment crediting Mark Bravington with the fix and showing the `return ERROR` it
replaced (`src/main/gram.y`). Multiline support here was added on purpose.

It also means
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
own lexer (`SpecialValue`, `src/main/gram.y`). Two details there are worth copying
rather than inventing:

- **R ungets the newline** before returning its error, so the error stays local:
  the newline is still available to terminate the statement and the next line
  parses normally. Aether does the same — on hitting a newline, stop *before*
  consuming it and emit `unterminated_special`, keeping the partial `%foo` as a
  `SPECIAL` token with the diagnostic attached (§8.6). Do not scan to EOF the way
  an unterminated string must (§8.4); here the language gives us a
  resynchronisation point, so use it.
- **R accepts `%foo` at EOF.** Its loop exits on `R_EOF` without ever seeing the
  closing `%` and still returns `SPECIAL`. tree-sitter-r's regex requires the
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

**`ERROR_TOKEN`.** A character that starts no token at all. Consume exactly one
character, raise `unexpected_character`, and return `ERROR_TOKEN` — the same
shape `biome_js_parser` uses at both of its equivalent sites. The reachable cases
are narrow: every printable ASCII character starts some R token, so this is only
control characters and non-ASCII characters that are neither `XID_Start` nor
`XID_Continue`.

This kind is **added to `r_kinds_src.rs` in this phase**; air's kind list has no
error token. Deviation 6 in `docs/deviations.md` carries the full argument, but
the short version is that all eight other Biome grammars declare `ERROR_TOKEN`,
and air is the outlier only because tree-sitter did its lexing. Adding it is one
line in `r_kinds_src.rs`, one generated line in `kind.rs`, and one arm each in the
hand-written `is_bogus()` and `to_bogus()` (not bogus itself; degrades to
`R_BOGUS` like every other token).

Do **not** use `IDENT` for this. It round-trips and it carries a diagnostic, so
every invariant holds — but `IDENT` is accepted wherever a name is, so `x <- €`
would build a clean `R_BINARY_EXPRESSION` whose right slot claims `€` is a name,
and nothing in the harness would catch it. `ERROR_TOKEN` starts no production, so
the parser's only move is a bogus node. See §11.1 for that side.

### 8.5 Newlines: the crucial decision

**Newlines are lexed as `NEWLINE` tokens, then converted to trivia by the token
source — but the parser can still ask whether one occurred.**

Do *not* replicate tree-sitter's scope-stack approach of conditionally swallowing
newlines in the lexer. In a hand-written recursive-descent parser the equivalent
information is available structurally: the parser knows whether it is inside
`(`/`[`/`[[`, so it can simply ignore `has_preceding_line_break()` there. Carrying a
scope stack in the lexer would duplicate that knowledge and is the classic source
of desync bugs.

So, with ownership stated once:
- `RLexer` emits `NEWLINE` as its own kind, **and owns the preceding-line-break
  flag**: it carries `after_newline` across calls and stamps
  `PRECEDING_LINE_BREAK` onto the first non-trivia token that follows (§8.2).
  This is verbatim what `biome_js_parser`'s lexer does for ASI.
- `RTokenSource` reports `NEWLINE` (and `WHITESPACE`, `COMMENT`) as trivia. It
  keeps **no** line-break state of its own and delegates the question to the
  lexer, exactly as `JsTokenSource` does (§9.1).
- `RParser` reads the flag only through `at_statement_break()` (§9.2), which
  gates it on `IGNORE_NEWLINES`. That predicate has three callers: the Pratt loop
  (§11.2), `parse_postfix` (§11.3), and `parse_else_clause` (§11.6). Nothing else
  reads the raw flag.

The division of labour is the same as JS's: the *lexer* answers "was there a line
break here", which is a lexical fact, and the *parser* decides whether that fact
means anything, which in JS is ASI and restricted productions and in R is
statement termination and `else` attachment.

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

All of it is `#[cfg(test)]` inside `aether_r_parser`. The markdown corpus of §10.2
is the *parser's* harness and the lexer does not use it: the lexer's entire output
is a flat list of `(kind, range)`, and an exact assertion on that list is both more
precise and more readable than a snapshot of it would be. Snapshots earn their keep
when the expected value is too large to write by hand; a token stream is not.

#### Module layout

```rust
// crates/aether_r_parser/src/lexer.rs — the last lines in the file
#[cfg(test)]
mod tests;
```

```rust
// crates/aether_r_parser/src/lexer/tests.rs
//
// The driver and the assertion macros. Every assertion itself lives in a
// submodule, one per dispatch arm of `lex_token` (§8.3) plus one per piece of
// cross-cutting machinery.

mod backticks;
mod bom;
mod brackets;
mod checkpoint;
mod comments;
mod coverage;
mod dots;
mod identifiers;
mod keywords;
mod line_breaks;
mod numbers;
mod puncts;
mod raw_strings;
mod specials;
mod strings;
mod whitespace;

use super::*;
```

Because `tests` is a *descendant* of `lexer`, these files read `after_newline`
and `string_delimiter` directly, with no accessor and no `pub(crate)` widening.
That is the whole reason the tests sit here instead of in a test crate, and it is
worth not giving up: an accessor added only for tests is a permanent hole in the
lexer's encapsulation, and there is no version of this file layout that needs one.

One file per dispatch arm is not a filing convention, it is the coverage argument.
`lex_token` is a `match`, `coverage.rs` proves every kind is reachable, and the
file list makes an arm with no test file visible by inspection.

#### The driver

The lexer cannot be driven by `next_token(Regular)` alone: string bodies and
closing delimiters are lexed in a context the *parser* supplies (§8.1). The tests
supply it the same way `RTokenSource` will, derived from the previous token rather
than hand-written per call, so the tests exercise the real contract:

```rust
/// Lex `source` to completion, driving `RLexContext` the way §9.1 will.
pub(super) fn lex(source: &str) -> (Vec<(RSyntaxKind, TextRange)>, Vec<ParseDiagnostic>) {
    let mut lexer = RLexer::from_str(source);
    let mut tokens = Vec::new();
    let mut context = RLexContext::Regular;

    loop {
        let kind = lexer.next_token(context);
        if kind == EOF {
            break;
        }
        tokens.push((kind, lexer.current_range()));

        // Match on what came back, not on what was requested: a string with an
        // empty body answers a `StringContent` request with `STRING_CLOSE`, and
        // the next token after that is `Regular` again.
        context = match (kind, lexer.string_delimiter) {
            (STRING_OPEN, Some(delimiter)) => RLexContext::StringContent(delimiter),
            (STRING_CONTENT, Some(delimiter)) => RLexContext::StringEnd(delimiter),
            _ => RLexContext::Regular,
        };
    }

    (tokens, lexer.finish())
}
```

#### The macros

Three, each with one job. They are thin wrappers over one `#[track_caller]`
function so that the failure output is written once and is good.

```rust
/// Assert the exact token stream, and that the lexer emitted NO diagnostics.
///
/// The silence is half the assertion. A lexer that produces the right tokens and
/// also complains about them is broken in a way that a kinds-and-lengths table
/// alone will not catch, and `[error]` corpus tests will not catch it either
/// because they only require *some* diagnostic.
macro_rules! assert_lex {
    ($source:expr, $($kind:ident : $length:expr),* $(,)?) => {
        $crate::lexer::tests::check_lex($source, &[$(($kind, $length)),*], &[])
    };
}

/// [`assert_lex!`] plus the diagnostics expected, in order.
///
/// Messages match as substrings, so a test pins the wording it cares about
/// without re-encoding the full rendering.
macro_rules! assert_lex_err {
    ($source:expr, $($kind:ident : $length:expr),* $(,)? => $($message:literal),+ $(,)?) => {
        $crate::lexer::tests::check_lex($source, &[$(($kind, $length)),*], &[$($message),+])
    };
}

/// Assert `PRECEDING_LINE_BREAK` on each NON-TRIVIA token, in order.
///
/// Trivia is filtered out on purpose: the flag is sticky across it by design
/// (§8.2), and "which real token reports a preceding break" is the only question
/// the parser ever asks (§8.5).
macro_rules! assert_line_breaks {
    ($source:expr, $($kind:ident : $preceded:literal),* $(,)?) => {
        $crate::lexer::tests::check_line_breaks($source, &[$(($kind, $preceded)),*])
    };
}
```

```rust
#[track_caller]
pub(super) fn check_lex(source: &str, expected: &[(RSyntaxKind, u32)], messages: &[&str]) {
    let (tokens, diagnostics) = lex(source);

    // Render the WHOLE stream on any failure. A message naming only the offending
    // token sends you back to re-run with `--no-capture` and print it yourself.
    let render = || -> String {
        tokens
            .iter()
            .map(|(kind, range)| format!("  {kind:?}@{range:?} {:?}", &source[*range]))
            .collect::<Vec<_>>()
            .join("\n")
    };

    assert_eq!(
        tokens.len(), expected.len(),
        "expected {} tokens, lexed {}:\n{}", expected.len(), tokens.len(), render(),
    );

    // The ranges must TILE the source: start at 0, each adjacent to the last, the
    // final one ending at `source.len()`. This is strictly stronger than summing
    // lengths — it also rejects a lexer that overlaps or rewinds — and it makes
    // reconstructing the input to compare it redundant.
    let mut offset = TextSize::from(0);
    for (index, ((kind, range), (want_kind, want_length))) in
        tokens.iter().zip(expected).enumerate()
    {
        assert_eq!(*kind, *want_kind, "token {index}:\n{}", render());
        assert_eq!(range.len(), TextSize::from(*want_length), "token {index}:\n{}", render());
        assert_eq!(range.start(), offset, "token {index} leaves a gap:\n{}", render());
        offset = range.end();
    }
    assert_eq!(
        usize::from(offset), source.len(),
        "tokens stop short of the end of the source:\n{}", render(),
    );

    let actual: Vec<String> = diagnostics.iter().map(render_message).collect();
    assert_eq!(actual.len(), messages.len(), "diagnostics:\n{actual:#?}");
    for (actual, expected) in actual.iter().zip(messages) {
        assert!(actual.contains(expected), "expected {expected:?} in {actual:?}");
    }
}
```

That tiling check is the single highest-value assertion in the phase, and it is
free on every test because it lives in the shared function rather than being
something each test remembers to do.

#### The files

| File | Pins |
|---|---|
| `bom.rs` | `UNICODE_BOM:3` at offset 0 and nowhere else — a bare BOM file, a BOM before a token, and U+FEFF mid-file *not* becoming a second `UNICODE_BOM`. Plus the two short-source cases `consume_potential_bom` guards with its `get(0..3)` (§8.3.1): a one-byte and a two-byte file must not panic or mis-slice. The `RRoot::bom` slot is dead without this file. |
| `whitespace.rs` | Runs of space/tab/`\x0B`/`\x0C` coalescing into one `WHITESPACE`; `\n` and lone `\r` as `NEWLINE:1`; `\r\n` as **one** `NEWLINE:2`; `\n\n` as two tokens; `\r\r\n` as two. |
| `comments.rs` | `#` to end of line; a comment ending the file with no newline; `# x\r\n` leaving the `\r` **out** of the comment and inside the `NEWLINE`; the empty comment `#`; non-ASCII inside a comment. |
| `line_breaks.rs` | The sticky flag of §8.2: false on the first token; true after a newline; surviving `WHITESPACE` **and** `COMMENT`; cleared by the first non-trivia token. `1\n  # c\n  + 2` reports the break on `+` and not on `2`. |
| `puncts.rs` | Every punct kind at least once, and the whole maximal-munch table of §8.4 as a prefix sweep: `<`, `<-`, `<<-`, `<<` (**two** tokens), `<=`, `>`, `>=`, `-`, `->`, `->>`, `:`, `::`, `:::`, `::::`, `:=`, `*`, `**`, `\|`, `\|\|`, `\|>`, `&`, `&&`, `=`, `==`, `!`, `!=`. Adjacency cases too: `1<-2` vs `1 < -2`. Plus the ASCII half of `ERROR_TOKEN` (§8.4) — a control byte, which is where the dispatch's final `_` lands. |
| `brackets.rs` | Greedy `[[`/`]]`; `x[ [1] ]` staying four separate brackets; `[[[` as `L_BRACK2` + `L_BRACK`; and `re_lex(SplitBrackets)` — splitting `[[` into `L_BRACK:1` with the second bracket still to come, being a no-op on any other token, and being idempotent. |
| `keywords.rs` | Every row of the keyword table in §8.4, `NA_real_` → `NA_DOUBLE_KW` included. Case sensitivity in both directions: `If`/`Function` are `IDENT`, and so are `true`/`false`/`null`. The `else` prefixes: `elseif`, `else_idx`, `elseμ` (tree-sitter-r #200). |
| `identifiers.rs` | `foo`, `foo.bar`, `a1`, `_` alone, `_foo` as one `IDENT` (not rejected here — §8.4), `.foo`, `.`, `..`, and unicode XID starts and continues. Plus the non-ASCII half of `ERROR_TOKEN`: a character such as `€` that is neither `XID_Start` nor punctuation. |
| `backticks.rs` | One `IDENT` spanning the delimiters; `` `a\`b` `` with the escape; the empty `` `` ``; the multi-line name of §8.4; and an unterminated one running to EOF with `unterminated_backtick_identifier` pointing at the **opening** delimiter. |
| `numbers.rs` | `1`, `1L`, `1i`, `1.`, `.5`, `1e`, `1e+`, `1e-3`, `1e5L`, hex `0xFF`, `0x1p3`, `0x.8p1`, `0xFFL`. The trailing-empty-exponent case is deliberate (§8.4) and needs a comment saying so, or it reads as a bug. |
| `dots.rs` | The §8.3 dispatch: `...` → `DOTS`, `..1`/`..12` → `DOTDOTI`, `.5` → `R_DOUBLE_LITERAL`, `.foo`/`.`/`..` → `IDENT`, `...1` → `DOTS` then a literal. `.1L` and `.1i` are the regression for tree-sitter-r #190 and belong here rather than in `numbers.rs`, since the bug was in the dispatch. |
| `specials.rs` | `%%`, `%/%`, `%in%`, `%>%`, `%o%`, and the empty `%%`. The newline case of §8.4: `%foo\n1` keeps `%foo` as `SPECIAL:4`, does **not** consume the newline, and carries `unterminated_special`. Plus the two inherited tree-sitter divergences — `%foo` at EOF errors, and `%a\b%` is not a `SPECIAL` — each with a comment pointing at `docs/deviations.md`. |
| `strings.rs` | The `STRING_OPEN`/`STRING_CONTENT`/`STRING_CLOSE` triple for `'` and `"`; the empty string emitting **no** `STRING_CONTENT`; escapes staying inside one content token; the multi-line string; `#` and `%` inert inside a body; the other quote character inert inside; and an unterminated string running to EOF. |
| `raw_strings.rs` | All three bracket pairs; hyphen counts 0, 1, and several; a close that matches the bracket but not the hyphen count *not* terminating; `R'...'` uppercase; `rnorm` and `r + 1` falling through to the identifier and punct arms; an unterminated raw string. |
| `error_token.rs` | What holds of an `ERROR_TOKEN` however the dispatch reached it, the two arms themselves being covered by `puncts.rs` and `identifiers.rs`: the token spans the whole character at all four UTF-8 widths; a run produces one token *and one diagnostic naming that character* each, not one merged token; lexing continues afterwards; `a€b` is three tokens; and the token is non-trivia, so it takes the sticky break flag and clears it. |
| `checkpoint.rs` | `checkpoint()`/`rewind()` restoring every field of `LexerCheckpoint` (§8.2): position, kind, `current_flags`, `after_newline`, and `unicode_bom_length`. The §8.2 warning gets a test of its own: rewind to before a newline, lex forward again, and assert `has_preceding_line_break()` still reports it. That bug is invisible until Phase 12's `else` handling and silent even then. |
| `coverage.rs` | See below. |

Diagnostics are asserted in the file that owns the construct — the unterminated
backtick in `backticks.rs`, the unterminated special in `specials.rs` — rather
than collected into a `diagnostics.rs`. A diagnostic is part of how a construct
lexes, and splitting the two means changing one without noticing the other.

Two of the original tests, relocated, to fix the shape:

```rust
// lexer/tests/dots.rs
#[test]
fn dispatch() {
    assert_lex!("... ..1 .5 .foo . ..",
        DOTS:3, WHITESPACE:1, DOTDOTI:3, WHITESPACE:1,
        R_DOUBLE_LITERAL:2, WHITESPACE:1, IDENT:4, WHITESPACE:1,
        IDENT:1, WHITESPACE:1, IDENT:2);
}

// lexer/tests/specials.rs
#[test]
fn stops_at_newline() {
    // R's `SpecialValue` ungets the newline and errors, so the NEWLINE survives to
    // terminate the statement. `%foo` is still a SPECIAL with a diagnostic
    // attached, NOT an `ERROR_TOKEN`: the text is a well-formed operator that was
    // cut short, so the parser can still use it, and `ERROR_TOKEN` is reserved for
    // characters that start nothing at all (§8.4).
    assert_lex_err!("%foo\n1",
        SPECIAL:4, NEWLINE:1, R_DOUBLE_LITERAL:1
        => "Unterminated");
}
```

#### `coverage.rs` — the anti-gap guard

"Every token kind has lexer coverage" is the acceptance criterion for this phase,
and as prose it is unfalsifiable: nobody re-derives it by hand across sixteen
files. `MANIFEST.toml` does this job for the corpus (§10.3.3); this does it for
the token inventory.

`RSyntaxKind` is `#[repr(u16)]` and ends in `__LAST`, and the `SyntaxKind` trait
gives a safe `from_raw`, so the full inventory is enumerable:

```rust
/// Well-formed sources, which between them must produce every lexable kind but
/// `ERROR_TOKEN`. The test names the gaps for you.
const SOURCES: &[&str] = &[ /* one per construct */ ];

/// Sources for the kinds no well-formed source can reach. `ERROR_TOKEN` is the
/// only one, and it comes with a diagnostic by construction, so it needs a list
/// of its own rather than a hole in `SOURCES`. Both lists feed `produced`, and
/// each asserts its own diagnostic polarity — that is what stops a source from
/// drifting between the two lists unnoticed.
const ERROR_SOURCES: &[&str] = &["\u{1}"];

/// Kinds the lexer legitimately never produces, each with a reason. Adding a
/// node kind to `r.ungram` therefore does NOT fail this test, but adding a token
/// kind does, until something lexes it.
fn is_lexable(kind: RSyntaxKind) -> bool {
    !matches!(kind, TOMBSTONE | EOF | __LAST) && !kind.is_list() && !is_node(kind)
}

#[test]
fn every_lexable_kind_is_produced() {
    let mut produced = HashSet::new();
    for source in SOURCES {
        // ... assert the source lexed cleanly ...
        produced.extend(lex(source).0.into_iter().map(|(kind, _)| kind));
    }
    for source in ERROR_SOURCES {
        // ... assert the source did NOT lex cleanly ...
        produced.extend(lex(source).0.into_iter().map(|(kind, _)| kind));
    }

    let missing: Vec<_> = (0..RSyntaxKind::__LAST as u16)
        .map(|raw| RSyntaxKind::from_raw(RawSyntaxKind(raw)))
        .filter(|kind| is_lexable(*kind) && !produced.contains(kind))
        .collect();

    assert!(missing.is_empty(), "no test lexes: {missing:#?}");
}
```

The failure mode it is built for is not "someone forgot a test today" — it is
Phase 3-style grammar edits landing a new token kind in `r_kinds_src.rs` months
from now with nothing lexing it. That failure is otherwise discovered by the
parser, as a confusing missing-slot bug several phases downstream.

**Done when:** `coverage.rs` passes with an empty `missing` list; every file in
the table above exists and is non-trivial; every `assert_lex!` in the suite tiles
its source; and the R sources in the eventual `literals/*.md` corpus files all lex
with **zero** diagnostics (they will not *parse* yet — there is no parser).

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
"Inherited from air" section of `docs/deviations.md` (§7.3) — but aether copies it
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
  `expected_selector` (§7.2). Nothing is being let through here that step 3 or the
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
`[foo::][bar]`. Since aether makes the RHS **required** (§7.2), that workaround is
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
can regress silently. It only passes because §8.2's `after_newline` flag is sticky
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

**Approach: lex greedily, split via re-lex.** The lexer always produces `[[` and
`]]` when it sees two adjacent brackets. When the parser is inside a single-`[`
subset and encounters `]]`, it re-lexes to split it into two `]` tokens. This is
exactly what Biome's JS parser does for `>>` inside nested generics.

**The split must happen at the loop guard, not after the list returns.** This is
the detail the whole approach turns on. If `parse_argument_list` runs to
completion first and the caller splits afterwards, the list never terminates
correctly: at `]]` its guard `p.at(end)` is false, it is not at `,` either, so
recovery fires — and `]]` is in no recovery set, so recovery *consumes both
brackets* and runs to EOF. The caller then finds no `]]` to split, `expect(']')`
errors, and the enclosing subset has lost its terminator too. One valid nesting
becomes a cascade of errors.

So the list has to own its terminator, including the case where the lexer glued
that terminator to someone else's:

```rust
// `RReLexContext::SplitBrackets` is defined in §8.2 and lands in Phase 4; this is
// its one and only call site.

// crates/aether_r_parser/src/syntax/call.rs
//
/// What closes an argument list. A type rather than a bare `RSyntaxKind`,
/// for the same reason `StatementListEnd` (§11.7) is one: the recovery set is
/// derived from the terminator instead of hardcoded at each call site. Here
/// there is a second reason — `[` has a terminator the lexer may have glued to
/// its neighbour, and only the terminator itself knows that.
#[derive(Debug, Clone, Copy)]
pub(crate) enum ArgumentListEnd {
    RParen,   // `f(…)`
    RBrack,   // `x[…]`  — a `]]` here is this list's `]` plus someone else's
    RBrack2,  // `x[[…]]`
}

impl ArgumentListEnd {
    /// True when the current token closes this list.
    ///
    /// **Side-effecting by design.** For `RBrack`, a `]]` is split into two `]`
    /// before the answer is returned, so every caller downstream — the loop
    /// guard, the recovery set, and the final `expect` — sees the exact closing
    /// token and none of them has to know the split happened.
    fn split_and_check(self, p: &mut RParser) -> bool {
        match self {
            Self::RParen => p.at(T![')']),
            Self::RBrack => {
                if p.at(T!["]]"]) {
                    // `x[a[b]]` — this list owns only the first `]`.
                    p.re_lex(RReLexContext::SplitBrackets);
                }
                p.at(T![']'])
            }
            Self::RBrack2 => p.at(T!["]]"]),
        }
    }

    /// Deliberately uniform across all three variants, and deliberately wider
    /// than this list's own terminator: recovery must never consume a closing
    /// delimiter of ANY kind or arity. Eating one turns a single error into a
    /// cascade by stripping an enclosing construct of its terminator — the same
    /// rule as `a$)` in §11.4, where the `)` is left alone for exactly this
    /// reason.
    fn recovery_set(self) -> TokenSet<RSyntaxKind> {
        token_set![
            T![,],
            T![')'], T![']'], T!["]]"], T!['}'],
            T![EOF],
        ]
    }
}

fn parse_subset_arguments(p: &mut RParser) {
    let m = p.start();
    p.bump(T!['[']);
    // Nothing to clear: `ALLOW_NEWLINE_BEFORE_ELSE` may stay set from an
    // enclosing `{`, and inside brackets it is harmless — see `parse_else_clause`.
    p.with_flags(RContextFlags::IGNORE_NEWLINES, RContextFlags::empty(), |p| {
        parse_argument_list(p, ArgumentListEnd::RBrack);
    });
    // The list split any `]]` on its way out, so this is a plain expect.
    p.expect(T![']']);
    m.complete(p, R_SUBSET_ARGUMENTS);
}
```

**No merge-back is ever needed, and the ladder cases confirm it** — worth stating,
because it is where a reader will stop. `x[[a[b]]]` lexes as
`x` `[[` `a` `[` `b` `]]` `]`. The inner `[` splits the `]]` and consumes only the
first `]`; the lexer then resumes at the next byte and lexes *fresh*, greedily
producing `]]` for the outer subset2. Because `re_lex` re-reads only the current
token and leaves position advancement alone, a split never has to be undone.

Add the nesting ladder as aether-authored tests in `expressions/subset.md`:
`x[a[b]]`, `x[[a[b]]]`, `x[a[[b]]]`, `x[[a[[b]]]]`. All clean parses, no markers —
which is the point, since the failure mode here produces errors on *valid* code
and the imported corpus only covers mismatched brackets and `#177`.

**Verify `re_lex` against buffered lookahead** at the pinned rev before writing
this. `TokenSourceWithBufferedLexer` caches lookahead tokens, and re-lexing the
current token must invalidate that cache. Biome's JS parser relies on the same
contract for `>>`, so it almost certainly holds — but a stale cache here yields a
wrong tree that still round-trips, which is precisely the class of bug no
invariant in this project catches.

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
        // Splits a `]]` as a side effect when `end` is `RBrack`, so from here
        // down the terminator is always a single, exact token.
        if end.split_and_check(p) || p.at(T![EOF]) { break }

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
string-context bump will actually produce. Verify that against `BufferedLexer` at
the pinned rev, but it is the standard Biome contract and the reason
`BumpWithContext` is a separate trait.

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
it and put `RBogusSelector` in the `AnyRSelector` union (§7.2.1). Without a bogus
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
/// the additional job of splitting a glued `]]`.
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
            // and `slot_to_node` panics on anything else (§7.1).
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
| **`[[` / `]]` splitting** | Genuinely ambiguous; tree-sitter needs an external scanner and still has open bugs. | Greedy lex + re-lex split (§11.3). Six corpus tests exercise it. Do not invent a scope stack. |
| **The statement wrapper** | Replaces `R_EXPRESSION_LIST` with `R_STATEMENT_LIST` of `R_STATEMENT`, so air's formatter must change. | Land in Phase 3, before any parser code. Get explicit sign-off on the ungram shape. |
| **Codegen drift** | If aether's generated code diverges from air's, the swap-in fails in confusing ways. | Phase 2 asserts byte-identity, and CI asserts `just codegen` idempotence. |
| **Oracle unavailable on error tests** | tree-sitter bails on any error, so ~25 tests get no oracle coverage. | Accept it; those are exactly the tests where we *intend* to differ. Rely on reviewed snapshots there. |
| **Oracle skipped or rotted** | It is local-only and feature-gated (§10.4.1), so nothing forces anyone to run it, and nothing catches it breaking. | Named as an explicit step in every phase's checklist (§4). If it does break, fix cheaply or abandon it — it is deleted at swap-in regardless. Reviewed snapshots, not the oracle, are what carry forward. |
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
- `just oracle` — differential check against tree-sitter-r. Local only, never in
  CI, and the whole crate goes away at swap-in. Run it at the end of a phase.
- `cargo nextest run -E 'test(/Dollar/)'` — filter by name

Tests are **read-only**. Only `xtask accept` writes corpus files; nextest runs
tests in parallel processes and they would race otherwise.

## Invariants — do not break these
1. `parse(src).syntax().to_string() == src` for ALL inputs. Always.
2. Bogus nodes always come with at least one diagnostic.
3. The parser never panics and never loops forever.
4. Tree shape matches tree-sitter-r except where `docs/deviations.md` says so.

## Test corpus
`crates/aether_r_parser_tests/tests/corpus/**/*.md`, one file per concept, one test per
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
