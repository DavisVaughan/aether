## If [pending]

```r
if (x)
  log(y)

if (a.b) {
  log(c)
  d
}

if (
  x <
  y
) {
  z
}
```

## If-else [pending]

```r
if (x)
  y else if (a)
  b

if (x)
  y else if (a)
  b else d

if (a) {
  c
  d
} else {
  e
}
```

## Complex If Expression [pending]

```r
# Invalid at top level due to newline before `else`, so not a real if statement
if (TRUE) {
  1
}
else {
  2
}

# Invalid for same reason as above
if (TRUE)
  1
else
  2

# Valid inside `{` only due to special `else` handling with newlines
{
  if (TRUE) {
    1
  }
  else {
    2
  }
}

# Valid with comments in special newline territory
{
  if (TRUE) {
    1
  }
  # hi there

  # another one!

  else {
    2
  }
}

# Valid. This test ensures we handle the newline after `1 + 1` correctly (#125).
{
  if (TRUE)

    1 + 1
}

# Valid. Newlines are allowed between the `else` and the `alternative`, even at top level (#141).
if (TRUE) {
  1
} else
{
  2
}

# Valid. Same as above but in `{ }` so it is valid no matter where the newlines are.
{
  if (TRUE) {
    1
  } else
  {
    2
  }
}

# Valid. Newlines and comments are allowed between the `else` and the `alternative`, even at top level.
if (TRUE) {
  1
} else

# do this alternative
{
  2
}

# Valid. Newlines are allowed between the `else` and the `alternative`, even at top level.
if (TRUE) 1 else
  2
```

## Identifiers starting with `else` that are not the `else` keyword (#200) [pending]

```r
# ASCII letter
{
  if (TRUE) 1
  elseidx <- 1
}

# ASCII number
{
  if (TRUE) 1
  else1idx <- 1
}

# ASCII `_`
{
  if (TRUE) 1
  else_idx <- 1
}

# ASCII `.`
{
  if (TRUE) 1
  else.idx <- 1
}

# Non-ASCII, but considered `iswalnum()` in R with UTF-8 locale.
# Not considered `iswalnum()` in our scanner with C locale, but
# our overapproximation with `>= 128` allows this and aligns with R.
{
  if (TRUE) 1
  elseμ <- 1
}

# Non-ASCII (U+00B7), rejected by R as syntax error since `iswalnum()` returns false.
# Our overapproximation with `>= 128` allows this, but that's ok since the R
# side behavior is a syntax error.
{
  if (TRUE) 1
  else· <- 1
}
```

## Identifiers starting with `else` that are not the `else` keyword - 2 (#200) [pending]

```r
# Correctly treats `else.x` as a single identifier, not as `else` followed by `.x`,
# but should really be a syntax error. See https://github.com/r-lib/tree-sitter-r/issues/194.
if (TRUE) {
  1
} else.x
```

## If With Comments And `else`-Like Identifiers [pending]

```r
if (a) 1
if (a) 1 else 2
if (a) 1 else if (b) 2 else 3

if # comment1
  (a) # comment2
  1 else # comment3
  2 # comment4

# ------------------------------------------------------------------------------
# `else`-like identifier is not treated as `else` (#499)

# ASCII letter
{
  if (TRUE) 1
  elseidx <- 1
}

# ASCII number
{
  if (TRUE) 1
  else1idx <- 1
}

# ASCII `_`
{
  if (TRUE) 1
  else_idx <- 1
}

# ASCII `.`
{
  if (TRUE) 1
  else.idx <- 1
}

# Non-ASCII, but considered `iswalnum()` in R with UTF-8 locale.
# Not considered `iswalnum()` in our scanner with C locale, but
# our overapproximation with `>= 128` allows this and aligns with R.
{
  if (TRUE) 1
  elseμ <- 1
}

# Non-ASCII (U+00B7), rejected by R as syntax error since `iswalnum()` returns false.
# Our overapproximation with `>= 128` allows this, but that's ok since the R
# side behavior is a syntax error.
{
  if (TRUE) 1
  else· <- 1
}
```

## If And Else

```r
if (a) 1
if (a) 1 else 2
if (a) 1 else if (b) 2 else 3
```

An `else` binds to the nearest `if`.

```r
if (a) if (b) c else d
```

## A Consequence And An Alternative Stop At The Binary `?`

Both share the rank a function or loop body has, so `if (a) b ? c` must parse as
`(if (a) b) ? c`.

```r
if (a) b ? c
if (a) b else c ? d
```

An assignment binds tighter than either, so the consequence swallows it.

```r
if (a) b <- c else d
1 + if (a) b
```

## A Newline Before `else` At Top Level [error]

Fatal here. The `if` ends at the consequence and the `else` is left over.

```r
if (a) 1
else 2
```

## A Newline Before `else` Inside A Delimiter

Newlines carry no meaning inside `(`, `[`, or `[[`, so the `else` attaches with no
braces in sight. This is the case a check written as "inside braces" gets wrong.

```r
f(if (a) 1
  else 2)
```

```r
x[if (a) 1
  else 2]
```

```r
x[[if (a) 1
  else 2]]
```

## A Newline Between `else` And Its Alternative

Allowed even at top level, unlike a newline before the `else` itself (#141).

```r
if (a) 1 else
  2
```

## An `if` With No Condition [error]

```r
if () 1
```
