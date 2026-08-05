# Aether

A hand-written recursive descent + Pratt parser for R, written in Rust.

Aether is built on the same [Biome](https://github.com/biomejs/biome) parser
infrastructure that [air](https://github.com/posit-dev/air) already uses, and
produces the same Rowan green tree that air's formatter and language server
consume. It exists to replace air's tree-sitter based parser with one that
recovers from syntax errors, reports real diagnostics, and models semicolons as
tokens rather than as whitespace.

The pipeline is:

```
source ──RLexer──► tokens ──RTokenSource──► RParser ──► Vec<Event> + Vec<Trivia> ──LosslessTreeSink──► Rowan
```

Everything from `Event` rightwards is Biome's, unchanged.

## Status

Under construction, one phase at a time. The staged plan lives in
`plans/aether-implementation-plan.md`.

## Development

The toolchain is pinned in `rust-toolchain.toml`, Biome is pinned to the same
fork and revision air uses, and tests run under
[cargo-nextest](https://nexte.st).

```sh
just codegen   # regenerate the syntax layer from r.ungram
just test      # cargo nextest run --workspace, plus doctests
```
