## Functions With `return` As Parameter Name

```r
# `return` is not a reserved word in R (it's not in `?Reserved`), so it is
# allowed as a parameter name (#189).

function(return) return
function(return = 1) return
```

## Function Dots And Dot Dot I [pending]

```r
# Dots as parameter without default
function(...) { list(...) }

# Dots as parameter with default
function(... = 1) { get("...") }

# `..i` as parameter without default
function(..1) { get("..1") }

# `..i` as parameter with default
function(..1 = 1) { get("..1") }

# Miscellaneous
function(x, ...) f(...)
function(x, ...) ..1 + ..2
```

## A Hole In A Parameter List [error]

```r
function(a,,b) 1
function(a,) 1
function(,) 1
```

## A Parameter That Is Not A Name [error]

```r
function(1) x
function("a") x
function(a$b) x
```

## A Missing Comma Between Parameters [error]

A name is a single token, so the token after one may well be another parameter.
Reporting the separator without consuming anything keeps both.

```r
function(a b) x
```

## A Default With No Value [error]

The default is required once an `=` is written (r-lib/tree-sitter-r#161).

```r
function(x = ) 1
```

## A Parameter List With No Closing Parenthesis [error]

The list runs to the end of the file and the `)` is reported missing, leaving
`RParameters` and the definition around it whole.

```r
function(a
```

```r
\(x
```

A closer that belongs to someone else ends the list without being consumed, so
the list keeps its own terminator missing and the stray closer survives as a
statement of its own rather than being swallowed here.

```r
function(a} 1
function(a] 1
```
