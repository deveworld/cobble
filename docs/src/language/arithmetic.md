# Arithmetic

Numeric variables use signed 32-bit integer arithmetic:

```python
def calculate():
    a = 10
    b = 3
    total = a + b        # 13
    difference = a - b   # 7
    product = a * b      # 30
    quotient = a / b     # 3
    remainder = a % b    # 1
    power = b ^ 3        # 27
    a += 1
    b *= 2
```

`**` also means power, and `//` also means integer division. Compound
assignments such as `+=`, `-=`, `*=`, `/=`, and `%=` work on variables,
list items, and map fields.

## Precedence

From highest to lowest: power (`^`, `**`), multiplication and division
(`*`, `/`, `//`, `%`), addition and subtraction (`+`, `-`), then comparisons.
Power groups right to left; the other arithmetic operators group left to
right. Use parentheses to make grouping explicit.

## Integer Division And Modulo

Division rounds down, including for negative numbers. The remainder has the
same sign as the divisor:

```python
a = -7 / 2      # -4
b = -7 % 3      # 2
c = 1 / 3 * 3   # 0
```

In a `const` declaration, division can produce a fraction:
`const HALF = 1 / 2` inserts `0.5` into a command's `{HALF}` argument.
Assigning that value to a numeric variable truncates it, with a warning.
Arithmetic overflow wraps as it does on Minecraft scoreboards.

## Division By Zero

A constant zero divisor is a compile error. Guard runtime divisors yourself:

```python
def share(total, players):
    if players != 0:
        return total / players
    return 0
```

An unguarded division by runtime zero fails the Minecraft operation and
leaves the left operand unchanged.
