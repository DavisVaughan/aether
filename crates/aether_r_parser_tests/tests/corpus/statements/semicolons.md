## Semicolons [pending, deviation]

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

## Semicolon At Start Of File [pending, deviation]

```r
;
1
```

## Semicolon Before The First Expression [pending, deviation]

```r
;1
```

## Semicolon At End Of File [pending, deviation]

```r
# https://github.com/posit-dev/air/issues/238
1;
```

## Two Semicolons At End Of File [pending, deviation]

```r
1;;
```

## Semicolon On Its Own Line At End Of File [pending, deviation]

```r
1

;
```
