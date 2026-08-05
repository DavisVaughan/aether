# Regenerate `aether_r_syntax` and `aether_r_factory` from `r.ungram`
#
# Live from Phase 2, when the syntax layer is vendored.
codegen:
    cargo run -p xtask_codegen -- grammar
    cargo fmt

# Run the full test suite
#
# `--no-tests=warn` keeps an empty workspace from exiting non-zero. Drop it once
# the parser lands its first tests.
test:
    cargo nextest run --workspace --no-tests warn
    cargo test --doc --workspace

# Regenerate all markdown snapshots, then verify they are stable
#
# Live from Phase 6, when the corpus harness lands.
accept:
    cargo run -p xtask -- accept
    cargo nextest run --workspace

# Differential check against tree-sitter-r. Local only; CI does not run this,
# and the whole crate is deleted at swap-in.
#
# Live from Phase 6, when the corpus harness lands.
oracle:
    cargo nextest run -p aether_r_parser_tests --features oracle

# Run the scratchpad in tests/quick_test.rs
#
# Live from Phase 6, when the corpus harness lands.
quick:
    cargo nextest run -p aether_r_parser_tests --run-ignored all -E 'test(quick_test)' --no-capture
