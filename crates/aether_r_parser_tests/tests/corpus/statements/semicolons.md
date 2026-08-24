## Semicolons

```r
1;2

1 ; 2

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

## Semicolon At End Of File

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
1;;2
```

```r
1
;
```

The comment has no expression to attach to, so it lands on the `;`.

```r
;# comment
```

## A Semicolon With No Expression Is Allowed Inside Braces

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

## A Semicolon Terminates Any Kind Of Statement

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
