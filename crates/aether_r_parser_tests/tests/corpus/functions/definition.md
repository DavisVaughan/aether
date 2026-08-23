## Functions [pending]

```r
function() 1
function() {}
function(arg1, arg2) {
  arg2
}

function(arg1, arg2 = 2) {}

function(x,
         y,
         z = 3) {

}

function()


  1

function
() 1 + 1

function

() 1 + 1

function

# this important comment

() 1 + 1

function() function() {}

function(x = function() {}) {}

# With no intermediate `{` scope
function() for(i in 1:5) i
```

## Function No Body [error]

```r
function(x, y)
```

## Function No Body With Assignment [error]

```r
x <- function(x, y)
```

## Function No Body Inside Another Function [pending, error]

```r
function(x = function()) {}
```

## Function Definitions

```r
function() 1
function(a) a
function(a, b) a + b
function(
  a # important!
) a
function(...) 1
function(a, ..., b) 1
function(a = 1, ..., b = 2) a
function(..1, ..2) get("..1") + get("..2")
function(..1 = 1, ..2 = 2) get("..1") + get("..2")
function(
  x = # important!
  4
) 1

\(x, y) 1
```

## A Body Stops At The Binary `?`

Weirdly in R, help is precedence 1 and function and loop bodies are precedence 2.
So `function() x ? y` must parse as `(function () x) ? y`.

```r
function() a ? b
function() a ? b ? c
?function() a
```

```r
function() a <- b
function() a = b
1 + function(x) x + 2
```

## A Definition With No Parameter List [error]

```r
function 1
\ 1
function
```
