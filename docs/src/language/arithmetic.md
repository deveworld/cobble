# Arithmetic

| Type | Example | Calculation |
| --- | --- | --- |
| Integer | `count = 10` | Signed 32-bit, wrapping on overflow |
| Float | `speed = 0.25` | Minecraft's native 32-bit floating point |
| Double | `precise = double(0.1)` | Software 64-bit floating point |

Decimal and exponent literals, including `1.0` and `1e3`, are floats.
Use `int(value)`, `float(value)`, or `double(value)` to convert a number;
`int()` truncates toward zero. Constructors also accept numeric text.

```python
def calculate():
    integer = 5 / 2                      # 2
    fraction = 5.0 / 2                   # 2.5, float
    rounded_down = -5.0 // 2             # -3.0
    remainder = -5.5 % 2                 # 0.5
    precise = double(0.1) + double(0.2)   # 0.30000000000000004
```

Mixed calculations use `double` when either operand is a double, otherwise
`float` when either is a float. Converting an existing float to double keeps
the float's rounded value; write `double(0.1)` to start at double precision.

## Operators

From highest to lowest: power (`^`, `**`), multiplication and division
(`*`, `/`, `//`, `%`), addition and subtraction (`+`, `-`), then comparisons.
Power groups right to left; the other arithmetic operators group left to
right. Compound assignments work on variables, list items, and map fields.

For two integers, `/` and `//` both round down, including negative results.
For real numbers, `/` divides and `//` rounds the quotient down. `%` follows
the divisor's sign. `abs`, `min`, `max`, `sum`, sorting, and `math.sqrt`
preserve real values. An integer square root remains an integer.

Double arithmetic supports `+`, `-`, `*`, `/`, `//`, `%`, comparisons,
square roots, and powers with a signed 32-bit integer exponent. Its software
implementation costs more commands than float arithmetic. Double values
round to the nearest representable value, with ties going to the even one.

## Limits

Real values must be finite. Overflow, invalid real operations, and an invalid
runtime conversion report an error in chat and produce zero. Double powers
with fractional exponents are currently unsupported. Guard runtime divisors:

```python
def share(total, players):
    if players != 0:
        return total / players
    return 0
```

A constant zero divisor is a compile error. An unguarded integer division by
runtime zero keeps Minecraft's behavior: the operation fails and leaves the
left operand unchanged. Out-of-range integer literals are clamped with a warning.

A `const` declaration is evaluated at build time; `/` can produce a fraction
for command arguments, as in `const HALF = 1 / 2`.
