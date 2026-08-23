## Operator precedence

```r
2+a*2
2+a+2
!a + !b
a <= 2 && 2 >= d
a[1] <- foo || bar
a && b(c) && d
val <- foo %>% bar(1) %>% baz()
```

## Precedence Tests

```r
A$"B"^NA
a::b$c
a$b?c
```

## Complex Expressions [error]

```r
repeat if (1) TRUE else repeat 42
if (TRUE) if (FALSE) 2 else NULL
a::b$c[[d]] <- e
TRUE ~ FALSE ~ NULL ? NA ? NaN
if (TRUE) FALSE
else NA
(if (TRUE) FALSE
else NA)
a = TRUE ? FALSE
TRUE <- FALSE = NA
TRUE <- FALSE ? NA
TRUE = FALSE ? NA
TRUE ? FALSE = NA
```

## Subset and Subset2 Precedence

```r
a[[b[1]]]
a[b[[1]]]
```
