## Calls

```r
f()
f(,)
f(,,)
f(x,)
f(x,,)
f(,x)
f(,,x)
f(x,,y)
f(,,x,,y,,)
f(x=,)
f("x"=,)
f(,y=)

f(x)
f(1+1)
f(1 ~ 1)
f(x, y)
f(x, y = 2)
f(x = 1 + 1)
f(x, y =)
f(f2(x, y))

# Dots as unnamed and named argument
f(...)
f(, ..., 1)
f(... = 1)
f(... = ,)
f(... = ...)

# `..i` as unnamed and named argument
f(..1)
f(, ..1, 1)
f(..1 = 1)
f(..1 = ,)
f(..1 = ..1)
```

## Calls With NULL As Argument Name

```r
fn(NULL =)
fn(NULL =,)
fn(NULL = 1)

# This was likely added to R to support this case for `switch()`
switch(typeof(x), NULL = this, integer = that)
```

## Calls With `return` As Argument Name

```r
# `return` is not a reserved word in R (it's not in `?Reserved`), so it is
# allowed as an argument name (#189).

fn(return = TRUE)
fn(return = )

# One specific example from #189
switch(
  x,
  return = ,
)
```

## `return` Is Just An Identifier

```r
# `return` is a regular symbol in R, not a keyword (#189). A `return()`
# call is parsed the same as any other call, with `return` as the
# identifier. A bare `return` is also parsed as an identifier.

function(x) return(x)
return(x)
return
```

## Switch

```r
switch(foo,
  x = 1,
  "y" = 2,
  z = ,
  3
)
```

## Calls With NA As Argument Name [error]

```r
# Unlike `NULL`, other keywords like `NA` aren't allowed here
fn(NA = )
```

## Not A Call, Subset, Or Subset2 [error]

```r
f
(x)

foo
[bar]

foo
[[x]]
```

## Not A Call, Subset, Or Subset2 Due To Sequential Arguments [error]

```r
f(x y)
foo[x y]
foo[[x y]]

f(x, y z)
foo[x, y z]
foo[[x, y z]]

f(, x y)
foo[, x y]
foo[[, x y]]
```

## Call Arguments [pending]

```r
fn()
fn(a)
fn(a, b)
fn(...)
fn(a, ..., b)

fn(a = 1)
fn(a = )

fn(a = 1, b = 2)
fn(a = , b = 2)

fn(... = 1)
fn(... =)

fn(..1 = 2)
fn(..1 =)

fn("arg" = 1)
fn("arg" =)

# Comma tests
fn(,)
fn(,,,)
fn(,,a,,b,,)

# Comment tests
fn(
  # comment
)
fn(,
  # comment
,)
fn(
  a, # comment1
  b # comment2
)
fn(
  a # comment1
  = # comment2
  1 # comment3
)

{expr}(a = 1)

# `NULL` argument name (r-lib/tree-sitter-r#164)
fn(
  NULL = 1
)
fn(
  NULL = 
)
fn(
  NULL = ,
)

# Standard call as of https://github.com/r-lib/tree-sitter-r/pull/197
return()
```

## Side By Side Arguments [error]

```r
# Can't have two arguments side by side without a delimiter
fn(a b)
```

## Closing Delimiter Belonging To Someone Else [error]

The argument list ends at any closing delimiter, not just its own, so the `}`
survives for its owner rather than being eaten as an argument. Nothing was
written between the `(` and the `}`, so the list is empty rather than holding
one empty argument.

```r
fn(}
```

```r
fn(a}
```

## A Newline Or Comment Between A Name And Its `=`

`parse_argument()` looks one token past the name to find the `=`, and that
lookahead skips trivia to reach it

```r
f(a
  = 1)

f(a # c
  = 1)

f("a" # c
  = 1)
```
