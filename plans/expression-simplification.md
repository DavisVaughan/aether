# Expression simplification

Splits the single Pratt loop in `crates/aether_r_parser/src/syntax/expr.rs` into
two phases, and retires the binding powers that `$`, `@`, `::`, and `:::` never
use.

Pure refactor. No behaviour change, no diagnostic change, no grammar change. The
acceptance criterion is that `just accept` writes nothing.

## Summary

One loop in `parse_expression_with_binding_power()` currently handles postfix,
binary, extract, and namespace, and tries all four on every iteration. R's
grammar is two layers, not one:

```
expression := prefix* primary (binop expression)*
primary    := atom suffix*
suffix     := '(' args ')' | '[' … ']' | '[[' … ']]' | ('$' | '@' | '::' | ':::') selector
```

Afterwards:

- `parse_lhs_expression()` — a prefix expression, or an atom followed by any
  number of suffixes.

- `parse_expression_with_binding_power()` — that, then the binary loop, and
  nothing else.

`infix_extract_binding_power()` and `infix_namespace_binding_power()` are deleted
outright, and `parse_postfix_expression()` dissolves into three arms of the
suffix `match`.

## Why

### 1. Ranks 16 and 17 are unreachable

`parse_expression_with_binding_power()` has exactly three callers, all inside
`expr.rs`:

| Caller | Binding power passed | Max |
|---|---|---|
| `parse_expression()` | `LOWEST` | 0 |
| `parse_prefix_expression()` | a prefix rank | 14 |
| `parse_infix_binary_expression()` | `rank + 1` left, `rank` right | 15 |

The 15 is `^`, which is right-associative and so passes its own rank. The highest
*left*-associative rank is `:` at 13, which passes 14. So nothing above 15 ever
reaches a comparison, and `16 < current` and `17 < current` are both unreachable.
Set extract and namespace to the same number and no tree changes.

The reason underneath: precedence and associativity are both answers to *how far
right does this operator extend?* These four have a fixed answer — one selector
token, never a nested expression. Associativity was already deleted from them for
that reason (PR #16); the rank is the same deletion, finished.

This is also what R's own grammar says. `gram.y` writes `$`, `@`, `::`, and `:::`
as productions over `SYMBOL`/`STR_CONST`, not as operators with a precedence. The
rank only ever earned its keep by being *above 15*, so that `-a$b` and `a^b$c`
group correctly, and after the split the phase ordering does that structurally.

### 2. The loop permits two things that cannot happen

The loop tries postfix, then binary, then continues — so its shape allows a
suffix to be applied to a completed binary or unary expression. Neither is
reachable: any suffix token following a binary operator's right operand is always
consumed by the inner recursive parse first, because suffixes take no
binding-power check at all. The same argument covers the unary case.

Splitting the phases makes the code express only what R allows, rather than a
more general structure whose extra reach happens to be dead.

### 3. It is the grammar, and it is Biome's shape

`biome_js_parser` factors the same way, as `parse_lhs_expression` — atom plus
member/call suffixes as one unit, with the binary loop stacked on top. `CLAUDE.md`
says to do what `biome_js_parser` does when in doubt.

### What changed to make this cheap

This was designed before PR #17 and deferred, because `parse_expression_with_lhs()`
was `pub(crate)` purely so `call.rs` could re-enter the loop with an already-parsed
argument name. That forced the suffix loop to be externally reachable, which
forced a second shared function and created a trap where
`parse_expression_with_binding_power()` would run the suffix loop twice.

PR #17 replaced that path with one token of lookahead and deleted
`parse_expression_with_lhs()`. `call.rs` now reaches the parser only through
`parse_expression()`, so the suffix loop can be private with a single caller.

## Decisions

### D1. `parse_lhs_expression()` is one function, and `parse_primary_expression()` keeps its name

```rust
fn parse_lhs_expression(p: &mut RParser) -> ParsedSyntax {
    if let Present(unary) = parse_prefix_expression(p) {
        return Present(unary);
    }

    let Present(mut lhs) = parse_primary_expression(p) else {
        return Absent;
    };

    loop {
        if at_statement_break(p) {
            break;
        }

        lhs = match p.cur() {
            T!['('] => parse_call(p, lhs),
            T!['['] => parse_subset(p, lhs),
            T!["[["] => parse_subset2(p, lhs),
            T![$] | T![@] => parse_extract_expression(p, lhs),
            T![::] | T![:::] => parse_namespace_expression(p, lhs),
            _ => break,
        };
    }

    Present(lhs)
}
```

The alternative is a `parse_primary_expression()` that wraps a renamed
`parse_atom_expression()`. That reads well but adds `primary`/`atom` and
`suffix`/`postfix` to a file that already has `parse_expression()`,
`parse_expression_with_binding_power()`, and `parse_lhs_expression()`. Keeping the
atom match under its current name costs nothing and adds no vocabulary.

The prefix early-exit is correct and worth understanding: the returned unary does
*not* get suffixes applied, because the recursive parse inside
`parse_prefix_expression()` already took them. `-a[1]` is `-(a[1])` either way.
The early exit just makes that dead path unreachable instead of merely unused.

### D2. The suffix loop is a `match`, and its branches own the `lhs`

Because the arm has already committed by the time it runs, each branch takes
`lhs` **by value** and returns `CompletedMarker` rather than `ParsedSyntax`. Every
`lhs.clone().precede(p)` in the file disappears — the clone exists today only
because these functions take `&CompletedMarker` and might return `Absent`.

### D3. The `infix_` prefix goes

`parse_infix_extract_expression()` → `parse_extract_expression()`, and likewise
for namespace. They are still infix; they are just no longer precedence-bearing,
and nothing else in the file is named for a precedence class anymore.

### D4. `at_statement_break()` is factored out before the loop is split

```rust
fn at_statement_break(p: &RParser) -> bool {
    p.scope().newlines_are_significant() && p.has_preceding_line_break()
}
```

Both loops need this guard: it is what makes `f\n(x)` two statements in the suffix
loop, and `a\n+ b` two statements in the binary loop. It is the subtlest line in
the file, so it gets one definition and two call sites rather than two copies that
can drift.

### D5. `BindingPower::HIGHEST` drops to 16

It is 18 today, standing for the postfix tier, with a `debug_assert!` message
saying "Postfix must stay the tightest-binding construct". Once 16 and 17 are gone
that number describes nothing.

16 is the right value, and exactly: the top rank in the table is `^` at 15, and a
left-associative operator at 15 would pass `rank + 1`. So
`current_binding_power <= HIGHEST` becomes precisely "no caller passes a binding
power above what the table can produce", and the message should say that suffixes
bind tighter than every numbered rank rather than naming postfix alone.

## Implementation

### Step 1 — Delete the dead binding powers

Remove `infix_extract_binding_power()` and `infix_namespace_binding_power()`, and
drop the `current_binding_power` parameter from both parse functions along with
the `new_binding_power < current_binding_power` guards.

### Step 2 — Factor the newline guard

Add `at_statement_break()` and use it at the existing loop's top. This lands
first, as its own commit, so the split does not have to introduce it and duplicate
it in the same change.

### Step 3 — Add `parse_lhs_expression()`

Move the suffix arms into it, fold `parse_postfix_expression()` into the `match`
as the `(`, `[`, and `[[` arms, and switch the branch signatures to take `lhs` by
value.

### Step 4 — Reduce `parse_expression_with_binding_power()`

It becomes `parse_lhs_expression()` plus the binary loop, and applies
`at_statement_break()` in that loop.

### Step 5 — Docs

- §11.2 — the "What the loop is shaped like" subsection, and the precedence
  table, where rows 16 and 17 join row 18 as *shape, not precedence*. Row 18
  already carries `—` in the associativity column and appears in no lookup table;
  16 and 17 become the same kind of row.

- §11.2 — `BindingPower::HIGHEST`'s paragraph, for D5.

- §11.4 — the "Three infix functions, not one with three branches" paragraph,
  which becomes two of the three plus a suffix arm, and the sentence about ranks
  16 and 17 sitting above `^`.

- §12.2 — if the performance section still describes the loop.

## Verification

- `just test` — 202 tests pass.

- **`just accept` writes nothing.** This is the real check. A pure refactor may
  not move a snapshot, and that is stronger than the test count, because the
  snapshots carry every tree and every diagnostic in the corpus.

- `cargo fmt --all --check` and `cargo clippy --workspace --all-targets` clean.

- The oracle runs as part of the suite, so every clean block is still compared
  against tree-sitter-r.

Performance is *not* a gate here. §12.2 defers real benchmarking to Phase 14, and
an ad-hoc harness is only worth reaching for if something looks wrong.

## Non-goals

- No behaviour change of any kind. If a snapshot moves, the change is wrong.

- `call.rs` and `parse_argument_name_clause()` are not touched. They landed in
  PR #17 and are settled.

- `parse_primary_expression()`'s arms are not restructured. §11.2 argues at length
  for the uniform one-arm-per-token match with no shared tail, and that argument is
  unaffected by this work.

## Interaction with Phases 11–13

An earlier draft of this argued for waiting until after Phase 13, on the grounds
that `function`, `\(x)`, `if`/`else`, `for`, `while`, `repeat`, and `{` all land on
this file.

That was wrong, and doing this first is better. All seven are *atoms* with greedy
bodies, so they arrive as `parse_primary_expression()` arms either way and do not
interact with the suffix split at all. Landing the split first means those phases
are written against the settled shape instead of needing a second restructure
afterwards.
