## Parenthesized Expression

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

## Not A Parenthesized Expression 1 [error]

```r
()
```

## Not A Parenthesized Expression 2 [error]

```r
(
  1
  2
)
```

## Not A Parenthesized Expression 3 [error]

```r
(1; 2)
```

## Parentheses With Comments

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

## Empty Parentheses [error]

```r
()
```

## Multiple Expressions In Parentheses [error]

```r
(
  a
  b
)
```
