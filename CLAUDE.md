# Aether

A hand-written Rust parser for R, built on Biome's parser infrastructure,
producing the same Rowan tree as `posit-dev/air`'s tree-sitter-based parser.

## Commands

Tests run under [cargo-nextest](https://nexte.st), not `cargo test`.

- `just codegen`: regenerate `aether_r_syntax` / `aether_r_factory` from `r.ungram`

- `just test`: `cargo nextest run --workspace`

- `just accept`: regenerate markdown snapshots via `xtask accept`, then verify

- `just quick`: run the `tests/quick_test.rs` scratchpad

- `cargo nextest run -E 'test(/Dollar/)'`: filter by name

Tests are read-only. Only `xtask accept` writes corpus files; nextest runs tests
in parallel processes and they would race otherwise.

## Invariants, do not break these

1. `parse(src).syntax().to_string() == src` for ALL inputs. Always.

2. Bogus nodes always come with at least one diagnostic.

3. The parser never panics and never loops forever.

4. Tree shape matches tree-sitter-r except where `docs/deviations.md` says so.
   That file is a manually maintained design record, not an automated check.

## Test corpus

The corpus lives in `crates/aether_r_parser_tests/tests/corpus/**/*.md`. One file
per concept, one test per `##` heading.

Each concept has two files. You write `X.md`, which holds headings, markers,
prose, and ```r blocks:

````md
## Colon

```r
1:2
```

## Closing brace [error]

A stray `}` is reported and skipped, and the parse keeps going.

```r
}
```
````

`just accept` generates `X.snap.md` from it. The snapshot repeats the heading,
the prose, and each source, and adds a ```cst and a ```diagnostics block after
each source. Never hand-edit a snapshot. To change a test, edit `X.md` and re-run
`just accept`.

One heading can hold several ```r blocks. Each block is parsed on its own, so
they are related tests rather than one program split into pieces, and the
heading's marker covers all of them.

Prose between blocks is copied to the snapshot and otherwise ignored. Use it to
say why a test exists. Do not use an R comment for that, since a comment is input
and changes the tree.

The heading marker sets what the blocks under it must do:

- `[error]`: each block produces at least one error diagnostic.

- No marker: each block parses with zero diagnostics and no bogus nodes.

## Editing files

Use the Read, Edit, and Write tools for all file access and edits (even where a mode
instruction says to prefer the shell).

## Comments

Extremely minimal. Far fewer than you think, and far shorter. Explain what is
not obvious from the code; never restate it, and never write a paragraph of
rationale. If a comment is going to end up being long, do not write it. Long
comments should be written by a human.

Rationale that is genuinely worth keeping belongs in `docs/deviations.md`, not in
a doc-comment.

## Prose

Applies to comments, doc-comments, corpus test prose, and everything in `docs/`.

- Write for an easy read. Plain words over jargon, short sentences over long
  ones. If a term needs its own explanation, it is probably the wrong term.

- Prefer a small code example over a verbose paragraph. Show the input and the
  output and let the reader draw the conclusion.

- Never use em dashes. Use a comma, a colon, a full stop, or parentheses.

## Imports

One `use` per line, never `use foo::{a, b}`, in the `aether_*` crates. Doesn't
apply to `xtask`/`xtask/codegen` or to `biome_ungrammar`, which is vendored.
Stable rustfmt can't enforce this (`imports_granularity` is nightly-only), so
watch for it by hand in review.

## Reference implementations

- `tree-sitter-r` (r-lib), especially `src/scanner.c` and the long comments in
  `grammar.js`. Those comments record real bugs; read them before changing lexer
  behavior.

- Biome's JS/CSS parsers, for idiom. When in doubt, do what `biome_js_parser`
  does.
