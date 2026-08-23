## Semicolons [deviation]

```r
1;2

1 ; 2

1;;2

{
    1;
}

{
    1; 2
}

1;# comment
1; # comment
```

## Semicolon At Start Of File [deviation]

```r
;
1
```

## Semicolon Before The First Expression [deviation]

```r
;1
```

## Semicolon At End Of File [deviation]

```r
# https://github.com/posit-dev/air/issues/238
1;
```

## Two Semicolons At End Of File [deviation]

```r
1;;
```

## Semicolon On Its Own Line At End Of File [deviation]

```r
1

;
```

## A Comment On A Semicolon With No Expression [deviation]

The comment has no expression to attach to, so it lands on the `;` and, once
the formatter drops the `;` for a hard line break, becomes dangling on the
`RStatement`.

```r
;# comment
```

```r
{
  ;# comment
}
```

## A Semicolon After A Line Break Starts Its Own Statement [deviation]

R's `prog` offers `expr '\n'` and `expr ';'` as alternatives, so whichever
terminator arrives first is the one that ends the expression. A newline
therefore takes the `;` out of reach, and R says so:

| Source | R |
|---|---|
| `1;` | one expression |
| `1\n;` | `2:1: unexpected ';'` |
| `1\n\n;` | `3:1: unexpected ';'` |
| `1;\n;` | `2:1: unexpected ';'` |

Inside braces `exprlist` carries `exprlist ';'` and `exprlist '\n'` as peers, so
`{1\n;}` is accepted — but by the same reduction order, the `;` trails a list
`1` has already left rather than terminating `1`.

Either way the `;` is never `1`'s. Aether gives it a statement of its own, which
is also what keeps the blank line above it in that statement's leading trivia
rather than inside a token the formatter is about to delete.

```r
1
;
```

```r
1;
;
```

```r
{
  1
  ;
}
```

```r
{
  1

  ;
}
```

## A Semicolon Terminates Any Kind Of Statement [deviation]

Every one of these is accepted by R.

```r
x <- 1;
f(1);
(1);
{1};
if (TRUE) 1;
for (i in 1) i;
while (FALSE) 1;
repeat break;
function() 1;
```

## Semicolons R Rejects At Top Level But Accepts Inside Braces [deviation]

R's top-level `prog` has no production for a `;` with no expression before it on
the same line, so each of these is `unexpected ';'` to R. `exprlist` does have
one — it is `exprlist ';'` over a possibly empty list — so the braced form of
each is valid.

| Source | R at top level | R inside `{}` |
|---|---|---|
| `;` | `1:1: unexpected ';'` | valid |
| `;;` | `1:1: unexpected ';'` | valid |
| `;1` | `1:1: unexpected ';'` | valid |
| `1;;` | `1:3: unexpected ';'` | valid |
| `1;;2` | `1:3: unexpected ';'` | valid |

Aether accepts both columns, the same way air and tree-sitter-r do. Reporting
the top-level half would follow R the way deviation 11 does for `1 2`, and is
deliberately not done here — a formatter that refuses a file over a stray `;`
helps nobody, and the token is preserved either way.

```r
{
  ;
}
```

```r
{
  ;;
}
```

```r
{
  ;1
}
```

```r
{
  1;;
}
```

```r
{
  1;;2
}
```
