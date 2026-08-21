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

Looking one token past the name runs into `EOF`, which is a token like any other
and never a `=`, so each of these is a value rather than a name clause.

```r
f(a
```

```r
f(a=
```

```r
x[[a
```
