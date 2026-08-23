## While

```r
while(TRUE)
  bar

while(x > 0)
  x <- x - 1

while(TRUE)
  break

while(TRUE)
  next
```

## While No Body [error]

```r
while (a < b)
```

## While With Braces

```r
while(TRUE) FALSE

while(TRUE) {}

while(a) {

}

while(fn()) {

}
```

## A `while` With No Condition [error]

```r
while () b
```
