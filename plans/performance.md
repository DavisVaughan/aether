# Aether: parser performance

**Three measured optimizations, none of them applied yet, and where to look next.**

This document is written for a Claude session picking this up cold. It records
what was measured, what was tried and rejected, and the exact changes to make,
so that none of it has to be rediscovered.

**Nothing in sections 1–3 is in the tree.** Each was written, measured, and then
reverted; this document is the only surviving record. Apply them in any order,
they are independent. The combined effect is measured in section 6.

---

## 0. How performance is measured here

There is no benchmark crate in the repo. Every number below came from a
throwaway harness that was deleted afterwards. To reproduce, build one that:

1. Clones `tidyverse/dplyr` and reads every `R/*.R` file into memory up front.
   The measurements below used commit `d5e94e7`: 106 files, 20,944 lines,
   575.2 KiB.

2. Calls `aether_r_parser::parse(source, RParserOptions::default())` per file,
   with `std::hint::black_box` on both the input and the result, and the result
   dropped *outside* the timed region.

3. Warms up 3 rounds, then takes the median of ~60 iterations per file, summed
   across the corpus. Run the whole thing 3–5 times and take the median of
   medians — run-to-run variance is about 1%, so anything under ~2% is noise.

dplyr is a good corpus for R specifically because **49% of its bytes are comment
text** (roxygen). Any corpus without that property will under-value section 2.

### The number that matters most

A full parse decomposes like this:

| Phase | Time | Share |
| --- | --- | --- |
| Lexing | 1.37 ms | 18.6% |
| Parsing (event generation) | 0.85 ms | 11.1% |
| **Tree sink (green tree construction)** | **5.16 ms** | **70.3%** |

**Parsing R is not the bottleneck. Building the Rowan tree is.** That work lives
in `biome_rowan` and `biome_parser`, not in this repo. Any future effort spent
micro-optimizing the lexer is bounded above by 18.6%, and in practice much less.

A symbolicated profile (`samply record` + `atos` against the `.dSYM`; samply's
`--save-only` does not symbolicate, so addresses must be resolved by hand)
attributes self time as:

```
 5.04%  RLexer::next_token
 3.80%  biome_rowan::arc::Arc::drop_slow
 3.68%  hashbrown::RawTable::reserve_rehash      <- NodeCache growth
 3.55%  biome_rowan::NodeCache::node
 3.55%  biome_rowan::NodeCache::token_with_trivia
 2.96%  aether_r_parser::parse::parse
 2.72%  RLexer::consume_xid_continue_or_dot
 1.45%  ThinArc::from_header_and_iter
```

By binary: 82.6% our code, **12.25% `libsystem_malloc`**, 4.24%
`libsystem_platform` (memmove). Allocation is a first-class cost.

---

## 1. Fat LTO and a single codegen unit

**Expected: 8.3% faster.** The largest of the three by a wide margin.

### Why

There was no `[profile.release]` section at all, so release builds used the
defaults: `codegen-units = 16` and no LTO. Since ~70% of the work happens inside
*other crates* (`biome_rowan`, `biome_parser`), cross-crate inlining was blocked
entirely. This is why the win is so much larger than anything achievable inside
the lexer.

### The change

Append to the workspace `Cargo.toml`:

```toml
[profile.release]
lto = "fat"
codegen-units = 1
```

### Measured

- `lto = "thin"` + `codegen-units = 1`: **7.2%**

- `lto = "fat"` + `codegen-units = 1`: **8.3%**

### The tradeoff

Fat LTO takes an incremental release rebuild (touch `lexer.rs`, rebuild a
dependent binary) from **0.77s to 4.46s**, roughly 6x. The day-to-day loop uses
the debug profile so this is mostly invisible, but if it becomes annoying,
`lto = "thin"` recovers 7.2 of the 8.3 points for about half the link cost.

---

## 2. Scan comments as raw bytes

**Expected: 6.2% faster lexing, ~1.3% faster overall.**

### Why

`consume_comment` walked one byte at a time through
`biome_parser::lexer::Lexer::advance_byte_or_char`, which branches on
`is_ascii()` and UTF-8-decodes anything non-ASCII. With 49% of real R source
being roxygen, this is the hottest loop in the lexer.

The correctness argument for the rewrite: a line break byte (`0x0A`, `0x0D`) can
never appear inside a multi-byte UTF-8 sequence, because every byte of such a
sequence has its high bit set. So scanning raw bytes cannot split a character,
and the resulting `position` is always on a character boundary.

### The change

In `crates/aether_r_parser/src/lexer.rs`, replace the body of
`RLexer::consume_comment`:

```rust
    /// `#` to the end of the line, exclusive of the line break itself
    fn consume_comment(&mut self) -> RSyntaxKind {
        self.assert_byte(b'#');
        self.advance(1);

        // A line break byte can never be part of a multi-byte UTF-8 sequence,
        // so scanning raw bytes can't split a character
        let rest = &self.source.as_bytes()[self.position..];
        let len = rest
            .iter()
            .position(|byte| matches!(byte, b'\n' | b'\r'))
            .unwrap_or(rest.len());
        self.advance(len);

        COMMENT
    }
```

The loop it replaces was:

```rust
        while let Some(byte) = self.current_byte() {
            if matches!(byte, b'\n' | b'\r') {
                break;
            }
            self.advance_byte_or_char(byte);
        }
```

### Measured

Lex-only time across the corpus: **1.42 ms to 1.33 ms**.

---

## 3. Pre-size the trivia buffer

**Expected: ~1% faster overall.**

### Why

`RTokenSource::trivia` started at `Vec::new()` and grew by doubling. Measured
density on dplyr is **0.099 trivia pieces per source byte**, so a 30 KiB file
pushes roughly 3,000 entries and pays about 12 reallocations, each memmoving the
whole buffer. This shows up as part of the 4.24% spent in `libsystem_platform`.

The equivalent fix for the *events* buffer is not available: it is
`Vec::new()` inside `biome_parser::ParserContext::new`, which exposes no
capacity constructor. Events are denser still, at 0.357 per byte.

### The change

In `crates/aether_r_parser/src/token_source.rs`, in `RTokenSource::from_str`:

```rust
        // Measured at roughly one trivia piece per ten bytes of R source
        let mut source = Self {
            lexer,
            trivia: Vec::with_capacity(source.len() / 10),
        };
```

replacing `trivia: Vec::new()`.

### Note

This over-allocates for a comment-free file — a 1 MiB file with no trivia
reserves ~100K entries it will not use. The allocation is transient and freed
when the parse ends, so this was judged an acceptable trade, but it is the one
place where a pathological input does strictly more work than before.

---

## 4. Tried and rejected

Both of these looked plausible and are worth *not* retrying.

### `#[inline]` on lexer helpers

Annotating the 16 small `RLexer` methods and `Lexer` trait impls (`advance`,
`current_byte_unchecked`, `eat_byte`, `eat_bytes`, `next_token`, `lex_token`,
and friends) made an isolated lex loop **17% faster** (1.32 ms to 1.10 ms) — and
did nothing whatsoever to full parse time (7.24 ms to 7.19 ms, inside noise).

The isolated harness calls `next_token` directly from within
`aether_r_parser`, where it inlines beautifully. The real path drives the lexer
through `BufferedLexer` in `biome_parser`, where it does not.

Under fat LTO the annotations were actively **worse** (6.57 ms to 6.67 ms):
LLVM already had full cross-crate visibility, and the hints pushed it toward
inlining `lex_token`'s large `match` into `next_token` and bloating the hot
loop.

**Lesson: a phase-isolated microbenchmark can badly misreport a change.** Always
confirm against end-to-end parse time.

### Byte-scanning whitespace

The exact transformation from section 2, applied to `consume_whitespaces`, is a
wash (6.57 ms to 6.64 ms — noise, trending worse). Whitespace runs are 2–4 bytes
of indentation, so building the slice costs more than the loop it replaces.

---

## 5. Where to look next

Ranked by expected value.

1. **`NodeCache` is ~11.7% of total time**, and `reserve_rehash` alone is 3.68%
   of pure waste: the cache is constructed empty on every `parse()` call and
   rehashes its way up as it fills. `biome_rowan::NodeCache` only derives
   `Default` and exposes no `with_capacity`, and `TreeBuilder::default()` owns
   one internally, so `LosslessTreeSink::new` and `with_cache` are equivalent —
   there is no lever from this side. Fixing it means adding a capacity
   constructor to the pinned `lionel-/biome` fork.

2. **Allocation generally.** 12.25% of samples are in `libsystem_malloc`. Green
   node construction (`ThinArc::from_header_and_iter`) is inherent to Rowan, but
   the sheer malloc share suggests an arena or a bump allocator for the
   transient buffers is worth an experiment.

3. **`ParserContext::events`**, same problem as section 3 and same blocker: it
   needs a capacity constructor upstream. At 0.357 events per byte it is the
   densest buffer in the pipeline.

4. **Incremental reparse is entirely unexplored.** Both aether and air throw the
   `NodeCache` away on every call, so the structural-sharing machinery Rowan
   provides is never exercised. For the LSP keystroke path this is likely a far
   larger win than anything in this document.

---

## 6. Verification and combined result

All three changes were applied together, measured, verified, and then reverted.
Re-applying them should reproduce this.

### Net result

**7.378 ms to 6.630 ms on the dplyr corpus, an 11.3% improvement.** Throughput
76.1 MiB/s to 84.7 MiB/s. Both figures are the median of five back-to-back runs
of each build.

### Checks

Any change here must hold the invariants in `CLAUDE.md`. The checks used were:

- `cargo nextest run --workspace` — 203 tests passed, and `cargo clippy
  --workspace --all-targets` was clean.

- Across all 106 dplyr files: round-trip
  (`parse(src).syntax().to_string() == src`), zero diagnostics, and
  `aether_r_oracle::compare` returning `Match` against air's tree. All three
  were 106/106 both before and after, which is what establishes that the
  comment rewrite in section 2 preserves invariant 1 and tree-shape parity on
  real-world code.
