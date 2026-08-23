## Operator precedence

```r
2+a*2
2+a+2
!a + !b
a <= 2 && 2 >= d
a[1] <- foo || bar
a && b(c) && d
val <- foo %>% bar(1) %>% baz()
```

## Precedence Tests

```r
A$"B"^NA
a::b$c
a$b?c
```

## Complex Expressions

A top level `if (TRUE) FALSE` / `else NA` pair came with this test. tree-sitter
takes it, since a top level `else` lexes as an identifier there, but R does not,
and one error in the block would cost the other eleven expressions their oracle
comparison. It lives in `control/if.md` instead, as
`A Newline Before `else` At Top Level`.

```r
repeat if (1) TRUE else repeat 42
if (TRUE) if (FALSE) 2 else NULL
a::b$c[[d]] <- e
TRUE ~ FALSE ~ NULL ? NA ? NaN
(if (TRUE) FALSE
else NA)
a = TRUE ? FALSE
TRUE <- FALSE = NA
TRUE <- FALSE ? NA
TRUE = FALSE ? NA
TRUE ? FALSE = NA
```

## Subset and Subset2 Precedence

```r
a[[b[1]]]
a[b[[1]]]
```
