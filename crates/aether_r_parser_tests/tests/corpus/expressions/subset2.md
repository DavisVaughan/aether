## Subset2

```r
foo[[]]
foo[[,]]
foo[[,,]]
foo[[x,]]
foo[[x,,]]
foo[[,x]]
foo[[,,x]]
foo[[x,,y]]
foo[[,,x,,y,,]]

foo[[x]]
foo[[x, y]]
foo[[1, 2]]
foo[[x=1,,y=3,4]]
```

## Unsupported Subset2 With `]]` Over Multiple Lines (#177) [error]

```r
x[["a"]
]
```

## Empty Subset2 Joins Its `]]`

Nothing stands between the `[[` and the `]]`, so the join has to happen on the
bump of the `[[` itself. That is what the `[[` being consumed inside its own
scope buys.

```r
x[[]]
```

## Side By Side Arguments Still Join The `]]` [error]

Recovering inside `[[` must leave the `]]` alone. It is a closing delimiter like
any other, so the recovery set names it; without that, recovery swallows both
brackets and strands the `[[` without a terminator.

```r
x[[a b]]
```

## Subset2 Arguments [pending]

```r
fn
fn[[a]]
fn[[a, b]]
fn[[...]]
fn[[a, ..., b]]

fn[[a = 1]]
fn[[a = ]]

fn[[a = 1, b = 2]]
fn[[a = , b = 2]]

fn[[... = 1]]
fn[[... =]]

fn[["arg" = 1]]
fn[["arg" =]]

# Comma tests
fn[[,]]
fn[[,,,]]
fn[[,,a,,b,,]]

# Comment tests
fn[[
  # comment
]]
fn[[,
  # comment
,]]
fn[[
  a, # comment1
  b # comment2
]]
fn[[
  a # comment1
  = # comment2
  1 # comment3
]]

{expr}[[a = 1]]
```
