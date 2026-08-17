## Subset

```r
foo[]
foo[,]
foo[,,]
foo[x,]
foo[x,,]
foo[,x]
foo[,,x]
foo[x,,y]
foo[,,x,,y,,]

foo[bar]
foo[1, 2]
foo[x=1,,y=3,4]
```

## Nested Brackets

The `]]` a subset2 wants reaches the parser as two `]`, so every rung of this
ladder has to hand the right number of them to the right list. All clean parses,
which is the point: the failure mode here is errors on valid code, and the
imported corpus only covers mismatched brackets.

```r
x[a[b]]
x[[a[b]]]
x[a[[b]]]
x[[a[[b]]]]
```

## Subset Arguments [pending]

```r
fn
fn[a]
fn[a, b]
fn[...]
fn[a, ..., b]

fn[a = 1]
fn[a = ]

fn[a = 1, b = 2]
fn[a = , b = 2]

fn[... = 1]
fn[... =]

fn["arg" = 1]
fn["arg" =]

# Comma tests
fn[,]
fn[,,,]
fn[,,a,,b,,]

# Comment tests
fn[
  # comment
]
fn[,
  # comment
,]
fn[
  a, # comment1
  b # comment2
]
fn[
  a # comment1
  = # comment2
  1 # comment3
]

{expr}[a = 1]
```
