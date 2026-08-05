# Regenerate `aether_r_syntax` and `aether_r_factory` from `r.ungram`
codegen:
    cargo run -p xtask_codegen -- grammar

# Run the full test suite
test:
    cargo nextest run --workspace

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
