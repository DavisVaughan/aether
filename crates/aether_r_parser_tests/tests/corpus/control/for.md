## For [pending]

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

## For No Body [pending]

```r
for (i in 1:5)
```

## For With Comments [pending]

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
