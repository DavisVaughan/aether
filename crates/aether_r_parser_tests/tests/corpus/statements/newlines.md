## Newlines [pending]

```r
apple
(banana)

{
  apple
  (banana)
}

(
  apple
  (banana)
)
```

## Two Expressions On One Line [error]

```r
1 2
```

## Two Expressions On One Line Inside Braces [pending, error]

Confirms the rule survives `parse_braced_statements()` making newlines
significant again from inside a paren.

```r
{ 1 2 }
```

## A Comment Does Not Hide A Line Break

A clean parse, and the one here that can regress silently. It only passes
because the lexer's `after_line_break` flag is sticky across trivia, so the break
still reaches `2` with a comment sitting between. If that stickiness breaks, this
fails at the lexer rather than surfacing later as a mystery
`expected_statement_separator`.

```r
1 # c
2
```
