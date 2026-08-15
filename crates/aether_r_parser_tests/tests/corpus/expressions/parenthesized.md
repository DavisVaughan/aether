## Parenthesized Expression [pending]

```r
(1)
((1))
(1 + 1)
(fn(a, b))
fn((a), (b))
(function() {
  body
})
```

## Not A Parenthesized Expression 1 [pending, error]

```r
()
```

## Not A Parenthesized Expression 2 [pending, error]

```r
(
  1
  2
)
```

## Not A Parenthesized Expression 3 [pending, error]

```r
(1; 2)
```

## Parentheses With Comments [pending]

```r
(1)

({
  1 + 1
})

((1))

( # comment1
  # comment2
  1 # comment3
  # comment4
) # comment5
```

## Empty Parentheses [pending, error]

```r
()
```

## Multiple Expressions In Parentheses [pending, error]

```r
(
  a
  b
)
```
