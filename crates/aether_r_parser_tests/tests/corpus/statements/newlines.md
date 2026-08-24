## Newlines

```r
apple
(banana)

{
  apple
  (banana)
}

(
  apple
  (banana)
)
```

## Two Expressions On One Line [error]

```r
1 2
```

## Two Expressions On One Line Inside Braces [error]

```r
{ 1 2 }
```

## A Comment Does Not Hide A Line Break

Lexer's `after_line_break` flag is sticky across trivia

```r
1 # c
2
```
