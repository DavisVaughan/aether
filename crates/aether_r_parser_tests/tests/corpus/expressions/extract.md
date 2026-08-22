## Dollar

```r
foo$bar
foo$bar$baz
foo$bar@baz
foo$bar()
foo$"bar"
foo$bar()$baz[[1]]$bam
foo$...
foo$..1
```

## Slot

```r
foo@bar
foo@bar$baz
foo@bar()
foo@"bar"
foo@...
foo@..1
```

## Dollar, At, Namespace, Namespace Internal With Expression RHS [error]

```r
# These nodes allows an optional RHS, and the RHS must be a string/identifier,
# so we nicely get a true node here alongside the braces. Even if that's not
# parsable R code, it's useful for completions and highlighting.
foo${bar}
foo@{bar}
foo::{bar}
foo:::{bar}
```

## Dollar No RHS [error]

```r
foo$
```

## Slot No RHS [error]

```r
foo@
```

## Dollar, At, Namespace, Namespace Internal With `if` RHS [error]

```r
# The RHS's of these nodes are restricted to strings and identifiers, and the RHS
# is optional. This ends up trying to match to an `if_statement` node, leaving the RHS
# empty, and then the `if_statement` node errors because it doesn't have a `(`. This
# is actually pretty decent behavior.
foo$if
foo@if
foo::if
foo:::if
```

## Dollar, At, Namespace, Namespace Internal With Newline Before Operator [error]

```r
# Newlines are not allowed before the operator, so none of these are valid
x
$y

x
@y

x
::y

x
:::y
```

## Extract Operators

```r
a$b
a@b

a$`b`
a@`b`

a$"b"
a@"b"

a()$b
a()@b

a$b$c
a@b@c

a$b()
a@b()

a$...
a@...

a$..1
a@..1

a$..10
a@..10
```

## Extract With A Literal RHS [error]

```r
# TODO: tree-sitter-r currently allows an `optional()` RHS, but our grammar
# requires a RHS, so we get `missing (required)`. These should eventually be
# parse errors if we can switch tree-sitter-r away from using `optional()`.
# For example, `a$1` gives:

#> RExtractExpression {
#>     left: RDoubleValue {
#>         value_token: R_DOUBLE_LITERAL@10..16 "1" [Newline("\n"), Whitespace("    ")] [],
#>     },
#>     operator: DOLLAR@16..17 "$" [] [],
#>     right: missing (required),
#> },
#> RDoubleValue {
#>     value_token: R_DOUBLE_LITERAL@17..18 "2" [] [],
#> },

a$1
a$NA
a$NULL
a$TRUE
a$(b)
a${ b }

a@1
a@NA
a@NULL
a@TRUE
a@(b)
a@{ b }
```

## Dollar Tolerates A Newline

```r
df$
  x
```
