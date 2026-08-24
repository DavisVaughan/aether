## For

```r
for (x in y)
  f

for (x in 5:6) {
  for (y in ys) {
    z
  }
}

for (x in y) for (y in z) x + y

for (x in y

) 1 + 1

# `...` as the `variable`
for (... in 1:2) print(get("..."))

# `..i` as the `variable`
for (..1 in 1:2) print(get("..1"))
```

## For No Body [error]

```r
for (i in 1:5)
```

## For With Comments

```r
for (i in 1) i

for (
  i # comment
  in # comment2
  1 # comment3
) i

for # dangling1
  ( # dangling2
    a in 1
  ) # dangling3
  a
```

## `...` And `..i` As The `for` Variable

Technically allowed!

```r
for (... in 1:2) print(get("..."))
for (..1 in 1:2) print(get("..1"))
```

## A `for` Variable That Is Not A Name [error]

```r
for (1 in x) y
```

## A `for` With No Variable [error]

```r
for (in x) y
```

## A `for` With No Sequence [error]

```r
for (i in) y
```
