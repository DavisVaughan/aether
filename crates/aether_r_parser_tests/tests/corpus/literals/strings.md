## Strings

```r
"foo"
"foo
bar"
"#"
","
"}"
'foo'
'foo
bar'
'#'
','
'}'
```

## Strings Without Content

```r
""
''
```

## Strings With Escape Sequences

```r
"\\"
"\ "
"\n"
"\t"
"\r"

"\0"
"\1"
"\7"
"\12"
"\123"
"\1234"

"\x1"
"\x01"
"\x012"

"\u1"
"\u01"
"\u001"
"\u0001"
"\u00011"

"\u{1}"
"\u{01}"
"\u{001}"
"\u{0001}"

"\U{1}"
"\U{01}"
"\U{001}"
"\U{0001}"
"\U{00001}"
"\U{000001}"
"\U{0000001}"
"\U{00000001}"

"\U1"
"\U01"
"\U001"
"\U0001"
"\U00001"
"\U000001"
"\U0000001"
"\U00000001"
"\U000000011"

"foo\"bar"
"foo\ "
"foo\
"
"\""
'\''
```

## Raw Strings

```r
r"(raw string)"
R"{another raw string}"
R"--[yet another ]- raw string]--"

r"(")"
r"("")"
r"(')"
r"('')"

r"-(-)-"
r"-()-)-"
r"--()-")--"

r"( () )"
r"(())"
r"( ())"
r"(() )"
r"-())-"
r"-(())-)-"

r"(raw
string
)"

# Empty strings with no `string_content` node
r"()"
r'()'
R"{}"
r"--[]--"

# Leading whitespace and newlines after open
r"(  hello)"
r"(\nhello)"
r"(
hello
)"
f(r"(\nhello\n)")
f(r"(
hello
)")

# Single quoted raw strings
r'(a)'
r'(')'
r'(")'

r()
```

## Unclosed Double Quote [error]

```r
"\"
```

## Unclosed Single Quote [error]

```r
'\'
```

## Unclosed Raw String [error]

```r
r"(
"
```

## String Values

```r
"hi there!"
"\""
'hi there!'
'\''
"'"
"multiline
string"
r"(")"
r"("")"
r"(')"
r"('')"
r"-(-)-"
r"-()-)-"
r"--()-")--"
r"( () )"
r"(())"
r"( ())"
r"(() )"
r"-())-"
r"-(())-)-"
r"(raw
string
)"
r"("some raw string
business")"
```
