## Closing Brace [error]

```r
}
```

## Closing Parenthesis [error]

```r
)
```

## Closing Bracket [error]

```r
]
```

## Opening Brace, Closing Parenthesis [error]

```r
# Parenthesis is "not valid" so it isn't matched by the external scanner
{)
```

## Opening Parenthesis, Closing Brace [error]

```r
(}
```

## Opening Parenthesis, Closing Bracket [error]

```r
(]
```

## Opening Bracket2, Unmatched Closing Bracket [error]

```r
x[[2]
```

## Opening Bracket and Bracket2, Unmatched Closing Bracket [error]

```r
x[y[[2]
```

## Opening Bracket2 and Bracket, Matched Closing Bracket [error]

```r
x[[y[2]
```

## Opening Bracket2 and Bracket, Unmatched Closing Bracket [error]

```r
x[[y[2]]
```

## An Argument Name Cut Off By The End Of The File [error]

```r
f(a
```

```r
f(a=
```
