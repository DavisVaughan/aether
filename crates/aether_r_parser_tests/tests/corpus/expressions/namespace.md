## Namespace get [error]

```r
foo::
foo::bar
foo::bar(1)
foo::...
foo::..1
...::foo
..1::foo
...::...
..1::..1
```

## Namespace get internal [error]

```r
foo:::
foo:::bar
foo:::bar(1)
foo:::...
foo:::..1
...:::foo
..1:::foo
...:::...
..1:::..1
```

## Namespace With Missing RHS [error]

```r
# It's nice that `::` allows an optional RHS and enforces that it can only be a
# string or identifier, so this gives us a pretty clean tree even though it is
# invalid R code.
# https://github.com/r-lib/tree-sitter-r/issues/65
library(dplyr::)

library()

1
2
3
```

## Namespace Operators

```r
a::b
a:::b

a::`b`
a:::`b`

a::"b"
a:::"b"

a::b()
a:::b()

a::b$c()
a:::b@c()

a::...
a:::...

...::a
...:::a

...::...
...:::...

a::..1
a:::..1

..1::a
..1:::a

..1::..1
..1:::..1
```

## Namespace With A Literal RHS [error]

```r
# TODO: tree-sitter-r currently allows an `optional()` RHS, but our grammar
# requires a RHS, so we get `missing (required)`. These should eventually be
# parse errors if we can switch tree-sitter-r away from using `optional()`.
# For example, `a::1` gives:

#> RNamespaceExpression {
#>     left: RIdentifier {
#>         name_token: IDENT@0..6 "a" [Newline("\n"), Whitespace("    ")] [],
#>     },
#>     operator: COLON2@6..8 "::" [] [],
#>     right: missing (required),
#> },
#> RDoubleValue {
#>     value_token: R_DOUBLE_LITERAL@8..9 "1" [] [],
#> },

a::1
a::NA
a::NULL
a::TRUE
a::(b)
a::{ b }

a:::1
a:::NA
a:::NULL
a:::TRUE
a:::(b)
a:::{ b }
```

## Chained `::` [error]

```r
a::b::c
```

## Chained `:::` [error]

```r
a:::b:::c
```

## Call On The Left Of `::` [error]

```r
a()::b
```

## Call On The Left Of `:::` [error]

```r
a():::b
```

## Namespace Newline Is Significant Only Outside Parens

R discards newlines inside `(`, `[`, and `[[` before `EatLines` ever sees them,
so the newline `foo::\nbar` forbids is fine here

```r
f(stats::
  median)
```
