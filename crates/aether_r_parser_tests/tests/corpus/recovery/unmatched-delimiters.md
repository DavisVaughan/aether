## Closing Brace [pending, error]

```r
}
```

## Closing Parenthesis [pending, error]

```r
)
```

## Closing Bracket [pending, error]

```r
]
```

## Opening Brace, Closing Parenthesis [pending, error]

```r
# Parenthesis is "not valid" so it isn't matched by the external scanner
{)
```

## Opening Parenthesis, Closing Brace [pending, error]

```r
(}
```

## Opening Parenthesis, Closing Bracket [pending, error]

```r
(]
```

## Opening Bracket2, Unmatched Closing Bracket [pending, error]

```r
x[[2]
```

## Opening Bracket and Bracket2, Unmatched Closing Bracket [pending, error]

```r
x[y[[2]
```

## Opening Bracket2 and Bracket, Matched Closing Bracket [pending, error]

```r
x[[y[2]
```

## Opening Bracket2 and Bracket, Unmatched Closing Bracket [pending, error]

```r
x[[y[2]]
```
