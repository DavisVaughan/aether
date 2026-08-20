## Identifiers

```r
foo
foo2
foo.bar
.foo.bar
.__NAMESPACE__.
.
..
._
._.
foo_bar
`_foo`
`a "literal"`
`another
literal \` foo`
`backslash followed by newline \
`
`\``
# Pipe placeholder
_
# Recognized as a single `_foo` identifier, even if invalid R code (#71).
_foo
__foo
_foo_
```

## Unicode Identifiers

```r
你好
.你.好.
.你_好.
```

## Invalid Identifiers [error]

```r
# R does not allow identifiers to start with a `.` followed by a digit, as this
# would cause ambiguity with `.1L` and `.1i`. R itself rejects this as a syntax
# error, we currently parse this as `.1` followed by `foo`, just like we do with
# `1foo` (#190).
.1foo
```

## Dot Dot I

```r
..1
..10
```

## Unclosed Backtick [error]

```r
`\`
```

## Dots [pending]

```r
# Parses, but fails at runtime
...

{ ... }

# An actual base R function
...()
```

## Dot Dot I In Functions And Calls [pending]

```r
..1
..2
..10

function(...) {
  ..1 + ..2
}

# Parses as a function
..1()
```
