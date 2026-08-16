## Functions With `return` As Parameter Name [pending]

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
