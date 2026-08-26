# Code review findings — `aether_r_parser` and `aether_r_parser_tests`

A full read of both crates as of `b597c54`, hunting for anything that can panic,
abort, loop forever, or be made simpler or faster.

Findings were verified rather than eyeballed. Two fuzzers were run against the
parser — one assembling random token pieces, one assembling random characters
weighted toward the bytes the lexer branches on — for roughly 5M inputs total
across debug and release. Both asserted the round-trip invariant and
"a bogus node always comes with a diagnostic". Neither found anything; every
finding below came from reading the code and then building a targeted input for
it. Throughput numbers are `--release` on an M-series Mac.

Each section is self-contained: what is wrong, where, how to reproduce it, and
what to do about it. Take them in any order — nothing here depends on anything
else here.

## Status

| # | Finding | Kind | Status |
|---|---|---|---|
| 1 | Unbounded expression nesting aborts the process | Correctness | ✅ Fixed on `feature/guard` |
| 2 | Deep trees from *iterative* parsing still abort | Correctness | Open |
| 3 | `expected_node()` interpolates an unbounded span into the message | Correctness / perf | Open |
| 4 | `position as u32` truncates silently | Correctness | Open |
| 5 | `RTokenSource::re_lex()` doc-comment is wrong | Docs | Open |
| 6 | `ERROR_TOKEN` comment in `expr.rs` is incomplete | Docs | Open |
| 7 | Upstream: `BufferedLexer::re_lex()` drops lookahead diagnostics | Correctness | Open, not reachable today |
| 8 | The Pratt loop's 13 binary arms | Simplification | Open |
| 9 | `parse_argument_name_clause()` repeats itself five times | Simplification | Open |
| 10 | Three near-identical name parsers | Simplification | Open |
| 11 | `check()` rebuilds the red tree four times | Simplification | Open |
| 12 | `consume_digits()` takes a function pointer | Performance | Open |
| 13 | `try_from_raw_string_open()` scans an unbounded hyphen run | Performance | Open |
| 14 | `parse_with_cache()` is private and its cache is thrown away | Design | Open |
| 15 | Nothing bounds the number of diagnostics | Robustness | Open |
| 16 | `has_bogus_nodes_or_missing_required_children()` greps the `Debug` output | Test quality | Open |
| 17 | The lexer test `render()` runs on every call, not just failures | Test quality | Open |

Where the review measured something and found it *fine*, that is recorded too —
see [Measured and left alone](#measured-and-left-alone). Do not "optimize" the
lexer scan loops without reading that section first.

---

## 1. Unbounded expression nesting aborts the process

✅ **Fixed** on `feature/guard` (commit `e45006f`). Recorded here so the
reasoning is not lost, and because finding 2 is the half that is still open.

`parse_expression_with_binding_power()` recursed once per nesting level with no
limit. Exhausting the stack raises `SIGABRT`, not a panic, so no caller can
catch it — air's LSP would die rather than report an error. That is a direct
break of invariant 3.

Measured in release on a 2 MiB stack, the default for both a spawned thread and
a tokio worker:

| Input | OK at | Aborts at |
|---|---|---|
| `"-".repeat(n) + "1"` | 1600 | 3200 |
| `"(".repeat(n) + "1" + ")".repeat(n)` | 1600 | 3200 |
| `"{".repeat(n) + "1" + "}".repeat(n)` | 1600 | 3200 |
| `"if (a) ".repeat(n) + "1"` | 1600 | 3200 |
| `"f(".repeat(n) + "1" + ")".repeat(n)` | 1600 | 3200 |
| `"a<-".repeat(n) + "1"` | 1600 | 3200 |
| `"2^".repeat(n) + "1"` | 1600 | 3200 |

Roughly 3 KB of input was enough. Runs that parsed and then `mem::forget`-ed the
result confirmed the abort was inside `parse()` itself, not in tree teardown.

The fix adds `MAX_NESTING_DEPTH = 256` and `RParser::with_nesting()`, and wraps
the Pratt loop in it. Every nesting path in the grammar bottoms out there, so one
guard covers all of them. Refusing consumes nothing, so callers recover over the
offending tokens exactly as they do over any other unparseable text.

Prior art, since it is easy to assume otherwise: **neither ruff nor rust-analyzer
nor biome does this.** Ruff has no depth limit at all — past
`MAX_UNCHECKED_RECURSION_DEPTH = 20` it grows the stack with the `stacker` crate
and keeps going. rust-analyzer has a global *step* limit (`PARSER_STEP_LIMIT`)
that asserts "the parser seems stuck", which is biome's `ParserProgress` idea
scaled up, not a nesting bound. Biome has neither.

---

## 2. Deep trees built *iteratively* still abort

**Open. This is the half of finding 1 that the depth guard does not cover, and
it needs a different fix.**

The Pratt loop's postfix arms (`(`, `[`, `[[`, `$`, `@`, `::`, `:::`) and its
left-associative binary arms are iterative. They build an arbitrarily deep
left-nested tree without deep parser recursion, so `MAX_NESTING_DEPTH` never
trips. But `biome_rowan` walks the tree recursively in `Drop`, `Display`, and
`Debug`, so the overflow just moves from parsing to everything afterwards.

Reproduce, on a 2 MiB stack:

```rust
let source = format!("f{}", "()".repeat(50_000));
let parsed = parse(&source, RParserOptions::default());
parsed.syntax().to_string();   // stack overflow, SIGABRT
```

`n = 12_800` survives, `n = 50_000` does not. Same for `"1".to_string() +
&"+1".repeat(n)` and `"a".to_string() + &"$b".repeat(n)`.

**Do not fix this by counting loop iterations toward `MAX_NESTING_DEPTH`.** A cap
of 256 would reject a 257-term sum, which is ordinary R and must keep parsing.

Real options, roughly in order of appeal:

- Leave it. 50,000 chained calls is far less plausible than 3,000 nested parens,
  and the depth guard already removed the reachable case. Document the residual
  and move on.

- `stacker::maybe_grow` at the recursive tree walks, the way ruff does it for
  parsing. Costs a dependency and only moves the ceiling.

- An explicit worklist in the walks that matter. This is upstream in
  `biome_rowan`, not something aether can fix locally.

Whoever picks this up should first decide whether it is worth fixing at all;
that decision is the deliverable, not necessarily a patch.

---

## 3. `expected_node()` interpolates an unbounded span into the message

`crates/aether_r_parser/src/diagnostics.rs:165-178`

```rust
format!("Expected {article} {name} but instead found '{}'.", p.text(range))
```

Every caller passes a *node* range, not a token range.
`statement_list.rs:69` passes `bogus.range(p)` explicitly, and
`or_recover_with_token_set()` hands `error_builder` the whole recovered span
(`biome_parser/src/parsed_syntax.rs:264`). So the message grows with the junk.

Longest single diagnostic message, measured:

| Input | Message size |
|---|---|
| `",".repeat(1_000_000)` | 1,000,044 bytes |
| `"*".repeat(500_000)` | 500,044 |
| `format!("f({})", "*".repeat(500_000))` | 500,044 |
| `format!("function({}) 1", "*".repeat(500_000))` | 500,047 |
| `format!("for ({} in x) 1", "*".repeat(500_000))` | 500,046 |

An LSP serializes that to the client. It also costs real time even when nobody
sees it: `"%".to_string() + &"a".repeat(1_000_000)` spends ~27 ms building a 1 MB
message that `merge_diagnostics()` then discards, because the lexer's
`unterminated_special` starts at the same offset and wins.

**Fix:** truncate the interpolated text — 40-ish characters with an ellipsis.
Keep the full `range` on the diagnostic; only the interpolation needs capping.

Note that biome's own `ParseDiagnostic::new_single_node()` has the identical
flaw, so this is inherited. But `expected_node()` is aether's function and
already deviates from upstream deliberately (it drops the duplicative hint), so
fixing it here is in keeping.

Touches all ten `expected_*` constructors, since they all funnel through
`expected_node()`. One change, no call-site churn.

---

## 4. `position as u32` truncates silently

`crates/aether_r_parser/src/lexer.rs:218`, `:235`, `:342`

```rust
self.current_start = TextSize::from(self.position as u32);
```

The trait already offers `text_position()`, which is
`TextSize::try_from(self.position()).expect("Input to be smaller than 4 GB")`.
The cast wraps past 4 GB and yields corrupt ranges; the trait method fails
loudly. The lexer uses `text_position()` everywhere else, so these three are just
inconsistent.

Not reachable with real R input. Worth taking because it is a two-minute change
that removes a silent-corruption path in favour of an existing loud one.

Line 235 is a `debug_assert!` and can use `text_position()` too.

---

## 5. `RTokenSource::re_lex()` doc-comment is wrong

`crates/aether_r_parser/src/token_source.rs:65`

> Re-lexes the current token, invalidating any buffered lookahead

It does not. `BufferedLexer::re_lex()` clears the lookahead *only* when the kind
changes, and when the kind is unchanged it explicitly preserves the lookahead and
rewinds the inner lexer back around it:

```rust
if new_kind != current_kind {
    self.lookahead.clear();
} else if !self.lookahead.is_empty() {
    self.current = Some(self.inner.checkpoint());
    self.inner.rewind(current_checkpoint);
}
```

Rewrite the comment to say what actually happens. The distinction matters —
see finding 7.

---

## 6. `ERROR_TOKEN` comment in `expr.rs` is incomplete

`crates/aether_r_parser/src/syntax/expr.rs:85`

```rust
// Lexer already raised `unexpected_character()`
ERROR_TOKEN => parse_literal_expression(p, ERROR_TOKEN, R_BOGUS_EXPRESSION),
```

`ERROR_TOKEN` also comes from `unterminated_string()` and
`unterminated_backtick_identifier()`. The invariant the comment is reaching for
is that *every* `ERROR_TOKEN` carries a lexer diagnostic, which is what keeps
invariant 2 true here. Say that instead of naming one of the three producers.

---

## 7. Upstream: `BufferedLexer::re_lex()` drops lookahead diagnostics

**Not reachable in aether today.** Recorded so it is not rediscovered from
scratch, and because it constrains how lookahead may grow.

`BufferedLexer::re_lex()` rewinds to `self.current` before re-lexing, and
`RLexer::rewind()` does `self.diagnostics.truncate(diagnostics_pos)`. If the kind
turns out unchanged it rewinds *forward* again — but `truncate` to a larger
length is a no-op, so any diagnostic produced while lexing the lookahead is gone
for good.

Aether is safe only because it never holds two or more lookahead entries at a
re-lex point: the single `p.nth_at(1, T![=])` in `parse_argument_name_clause()`
is always fully drained by the following `bump()` before `do_bump()` reaches its
`JoinBrackets` call. Verified by tracing `x[[a = 1]]` and `x[[a  ]]`.

**If lookahead depth ever grows beyond one non-trivia token, re-check this.** The
fix belongs upstream in `biome_parser`.

---

## 8. The Pratt loop's 13 binary arms

`crates/aether_r_parser/src/syntax/expr.rs:116-247` — roughly 140 lines

Thirteen arms, each structurally identical:

```rust
kind @ (T![|] | T![||]) if BindingPower::BINARY_OR >= current_binding_power => {
    parse_binary_expression(p, lhs, kind, BindingPower::BINARY_OR, Associativity::BINARY_OR)
}
```

Collapse to a lookup plus one arm:

```rust
kind => match binary_operator(kind) {
    Some(op) if op.power >= current_binding_power => {
        parse_binary_expression(p, lhs, kind, op)
    }
    _ => break,
},
```

The bigger win is what it makes impossible. `BindingPower` and `Associativity`
currently carry two *parallel* const families kept in sync only by convention, so
`BindingPower::BINARY_OR` paired with `Associativity::BINARY_AND` compiles
happily. Folding them into one

```rust
struct BinaryOperator { power: BindingPower, associativity: Associativity }
```

makes the mismatch unrepresentable and lets the whole precedence table be read
in one place against `gram.y`.

**All 13 pairings are correct today** — checked one by one during the review — so
this is purely maintainability, not a latent bug. Behaviour must not change;
the corpus should stay byte-identical through `just accept`.

---

## 9. `parse_argument_name_clause()` repeats itself five times

`crates/aether_r_parser/src/syntax/call.rs:161-198`

Five arms with the same five-line body, differing only in the token kind and the
node kind. Map first, then act once:

```rust
fn parse_argument_name_clause(p: &mut RParser) {
    let kind = p.cur();
    let node = match kind {
        IDENT => R_IDENTIFIER,
        DOTS => R_DOTS,
        DOTDOTI => R_DOT_DOT_I,
        T![null] => R_NULL_EXPRESSION,
        R_SINGLE_QUOTED_STRING_LITERAL
        | R_DOUBLE_QUOTED_STRING_LITERAL
        | R_RAW_STRING_LITERAL => R_STRING_VALUE,
        _ => return,
    };

    if !p.nth_at(1, T![=]) {
        return;
    }

    let clause = p.start();
    parse_literal_expression(p, kind, node);
    p.bump(T![=]);
    clause.complete(p, R_ARGUMENT_NAME_CLAUSE);
}
```

Keep the lookahead *after* the kind match, as above, so `nth_at()` is still only
called for the five kinds that can start a name — that ordering is load-bearing
for finding 7.

The two comments above the function (why `NULL` is accepted here and nowhere
else, and why one token of lookahead is needed) are worth keeping.

---

## 10. Three near-identical name parsers

- `crates/aether_r_parser/src/syntax/function.rs:110` — `parse_parameter_name()`

- `crates/aether_r_parser/src/syntax/loop_statements.rs:59` — `parse_for_variable()`

- `crates/aether_r_parser/src/syntax/selector.rs:22` — `parse_selector()`

The first two are byte-for-byte identical (`IDENT` / `DOTS` / `DOTDOTI`). The
third is that plus the three string kinds.

One shared helper for the identifier trio, with `parse_selector()` extending it
for strings. Watch that the three call sites keep their distinct recovery sets
and diagnostics — those genuinely differ and should not be merged.

---

## 11. `check()` rebuilds the red tree four times

`crates/aether_r_parser_tests/src/check.rs:34`, `:36`, `:45`, `:67`

```rust
let ast = format!("{:#?}", parsed.tree());                              // 1
let round_tripped = parsed.syntax().to_string();                        // 2
let has_bogus = has_bogus_nodes_or_missing_required_children(&ast, &parsed.syntax());  // 3
let unexplained = undiagnosed_bogus_nodes(&parsed.syntax(), parsed.diagnostics());     // 4
```

`RParse::syntax()` rebuilds a red root on every call — its own doc-comment says
"hold the result rather than calling it in a loop", and this is exactly that.
Bind `let root = parsed.syntax();` once and pass `&root` around.

Test-only, so the cost is small. Worth doing because the code currently reads as
a worked example of the thing the API docs warn against.

---

## 12. `consume_digits()` takes a function pointer

`crates/aether_r_parser/src/lexer.rs:727`

```rust
fn consume_digits(&mut self, is_digit: fn(&u8) -> bool) -> bool {
```

A `fn` pointer, so every digit byte costs an indirect call that cannot inline or
vectorize. Two call sites, two predicates (`u8::is_ascii_digit`,
`u8::is_ascii_hexdigit`).

Make it generic (`impl Fn(u8) -> bool`, monomorphized) or split into
`consume_decimal_digits()` / `consume_hex_digits()`.

Small — number lexing measured at ~1.8 ms/4 MB, so this is not a hotspot. Take it
because it is free, not because it is urgent.

---

## 13. `try_from_raw_string_open()` scans an unbounded hyphen run

`crates/aether_r_parser/src/lexer.rs:74`

```rust
let hyphens = bytes[2..].iter().take_while(|byte| **byte == b'-').count();
let hyphens = u8::try_from(hyphens).ok()?;
```

Counts the entire run before `u8::try_from` rejects anything over 255. Add
`.take(256)` before `.take_while(..)`.

Measured, so nobody over-invests: `r"` followed by 1M hyphens parses in 2.7 ms
and 4M in 3.9 ms — linear, not a problem today. This is tidiness.

---

## 14. `parse_with_cache()` is private and its cache is thrown away

`crates/aether_r_parser/src/parse.rs:83-100`

```rust
pub fn parse(text: &str, options: RParserOptions) -> RParse {
    let mut cache = NodeCache::default();
    parse_with_cache(text, options, &mut cache)
}
```

`parse_with_cache()` is private and has exactly one caller, which builds a
throwaway `NodeCache` each time. The cache still dedups within a single file, so
this is not pure waste — but reusing a green cache across reparses is the case
`NodeCache` exists for, and air's LSP reparsing on every keystroke is exactly
that case.

Either make it `pub` so air can hold a cache per document, or drop the
indirection and inline it. The current shape is the one arrangement that gets
neither the API nor the simplicity.

This is a design question for the owner, not a defect. Decide before writing
code.

---

## 15. Nothing bounds the number of diagnostics

A degenerate file produces one diagnostic per offending token, with no cap:

| Input | Diagnostics | Time |
|---|---|---|
| `";".repeat(4_000_000)` | 4,000,000 | 2.6 s |
| `format!("function({}) 1", ",".repeat(1_000_000))` | 1,000,001 | 1.2 s |

Each `ParseDiagnostic` carries a message and an advice `Vec`, so this is also
hundreds of megabytes of resident memory for a pathological input. Linear, not
quadratic — but unbounded.

Biome does not cap either, so this is inherited and arguably fine. Worth a
decision rather than a silent inheritance, especially combined with finding 3,
which multiplies the per-diagnostic cost. If it is to be capped, the natural
place is `RParser::finish()`.

---

## 16. `has_bogus_nodes_or_missing_required_children()` greps the `Debug` output

`crates/aether_r_parser_tests/src/check.rs:107`

```rust
ast.contains("missing (required)") || root.descendants().any(|node| node.kind().is_bogus())
```

The bogus half asks the tree. The missing-required half greps a `{:#?}` string
for a phrase `biome_rowan` happens to print. If that formatting ever changes,
the check silently stops detecting missing required children and every corpus
test keeps passing.

Ask the tree instead — walk slots and look for an empty one in a required
position. If that turns out to be awkward with the current `AstNode` API, then at
minimum add a test that asserts the phrase still appears for a known-bad parse,
so the coupling fails loudly instead of silently.

---

## 17. The lexer test `render()` runs on every call, not just failures

`crates/aether_r_parser/src/lexer/tests.rs:81` and `:135`

```rust
let render = render(source, &tokens);
```

Computed eagerly at the top of `check_lex()` and `check_line_breaks()`, then used
only in assertion messages. Every passing lexer test formats the entire token
stream and throws it away.

Worse, `render()` does `&source[token.range]`. If a lexer bug ever produced a
range off a UTF-8 char boundary, the *failure message* would panic before
reporting the real failure — the one case where it needs to work.

Make it lazy: build it inside the assertion messages, or wrap in a closure. The
"print the whole stream on failure" behaviour is good and should be kept; only
the eagerness needs to go.

---

## Measured and left alone

Recorded so this ground is not re-covered, and so nobody optimizes the wrong
thing.

**The lexer is not the bottleneck.** Tree and marker construction in
`biome_rowan` dominates. End-to-end throughput:

| Shape | Throughput |
|---|---|
| Comments | 1100 MiB/s |
| Strings | 229 MiB/s |
| Identifiers | 102 MiB/s |
| Numbers | 80 MiB/s |
| Calls | 28 MiB/s |

**The `advance_byte_or_char`-per-character scan loops are fine.** They *look*
worse than `consume_comment()`'s byte-slice scan, and they are not. Over 1 MB
bodies with no diagnostics: comment 1.07 ms, `%...%` 1.42 ms, string 1.52 ms,
backtick 0.79 ms, identifier 0.83 ms, raw string 0.66 ms — all within 2× at
roughly 1 GB/s. **Rewriting these with `memchr` would be premature.** An earlier
draft of this review flagged them on inspection; measurement contradicted it.

**Argument-name lookahead is not a cost.** `f(aa = 1, bb = 2)` measured *faster*
per byte than `f(aa, bb, cc, dd)` (30.8 vs 38.7 ns/byte), so the `nth_at(1, ..)`
in `parse_argument_name_clause()` does not need avoiding.

**No quadratic blowups.** 1 MB versus 4 MB pathological inputs — hyphen runs,
unterminated strings/backticks/specials, comma floods, long identifiers and
numbers — all scaled linearly.

**List loops terminate by construction.** `ParseRecoveryTokenSet::recover()`
either consumes at least one token or returns `Err`, and the `p.expect(T![,])`
fallthrough in both `parse_argument_list()` and `parse_parameter_list()` is
always followed by a recovery that consumes. `ParserProgress::assert_progressing`
is a real `assert!`, live in release too, so a stuck parser panics rather than
hanging — and the fuzzers never tripped it.

**The `]]` re-lex is sound.** Traced and tested across `x[[]]`, `x[[b ]]`,
`x[[a[b]]]`, `x[[a[[b]]]]`, `x[[(a)]]`, `x[[{1}]]`, and `x[[a = 1]]`. The
`p.expect()` calls deliberately placed *outside* their `with_scope()` blocks are
load-bearing — moving one inside breaks the join. `x[[b] ]` erroring is the
intended `r-lib/tree-sitter-r#177` deviation, covered by the corpus.
