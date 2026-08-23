## Semicolons [error]

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

## Semicolon At Start Of File [error]

```r
;
1
```

## Semicolon Before The First Expression [error]

```r
;1
```

## Semicolon At End Of File [deviation]

```r
# https://github.com/posit-dev/air/issues/238
1;
```

## Two Semicolons At End Of File [error]

```r
1;;
```

## Semicolon On Its Own Line At End Of File [error]

```r
1

;
```

## A Semicolon With No Expression Is An Error At Top Level [error]

R reaches a `;` at top level only through `prog`'s `expr_or_assign_or_help ';'`,
so a `;` must follow an expression that ends on the same line. `prog` also
offers `expr_or_assign_or_help '\n'` as a peer, so the newline in `1\n;` has
already reduced `1` and leaves the `;` with nothing to terminate.

Each of these is `unexpected ';'` to R, and each has a mirror in the heading
below that R accepts.

```r
;
```

```r
;;
```

```r
;1
```

```r
1;;
```

```r
1
;
```

The comment has no expression to attach to, so it lands on the `;`.

```r
;# comment
```

## A Semicolon With No Expression Is Allowed Inside Braces [deviation]

`exprlist` carries `exprlist ';'` over a possibly empty list, so every source
above is valid R once wrapped in `{}`. The `;` still gets an `RStatement` of its
own rather than joining the expression before it: in `{1\n;}` the newline
reduced `1` through `exprlist '\n'` first, and the `;` trails a list `1` has
already left. That is also what keeps a blank line above the `;` in that
statement's leading trivia, rather than inside a token the formatter deletes.

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

```r
{
  ;# comment
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
