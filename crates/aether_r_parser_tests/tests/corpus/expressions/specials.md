## Specials

```r
x %% y
x %/% y
x %+% y
x %>% y
x %>% 2 %>% z
x %some text% y
x %//% y

# Not specials, but hard to test errors `%\%`, `%%%`
```

## Pipe

```r
x |> print()

x |> foo() %>% bar() |> baz()

x |> foo() |> bar() + baz()

x |> {function(x) x}()
```

## Pipe placeholder

```r
foo |> bar(x, y = _)
foo |> bar() |> baz(data = _)
```
