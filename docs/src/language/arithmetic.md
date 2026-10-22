# Arithmetic

Numeric variables are scoreboard integers. Arithmetic compiles to
`scoreboard players operation` commands on the pack's `<namespace>.vars`
objective.

```python
def calculate():
    a = 10
    b = 3

    total = a + b        # 13
    diff = a - b         # 7
    product = a * b      # 30
    quotient = a / b     # 3
    remainder = a % b    # 1
    power = b ^ 3        # 27

    mixed = a + b * 2 - a / 5   # 14, standard precedence

    a += 1               # same as a = a + 1
    a -= 1
    a *= 2
    a /= 2
    a %= 3
    b ^= 2
```

## Precedence

From highest to lowest:

1. `^` (power), right to left
2. `*`, `/`, `%`, left to right
3. `+`, `-`, left to right
4. `==`, `!=`, `<`, `<=`, `>`, `>=`

## How Expressions Compile

- Constant operands go through helper fake players such as `#multiplier`,
  `#divisor`, and `#modulus`, because `scoreboard players operation` only
  takes scores.
- Intermediate results use fake players such as `#expr_temp_<n>`.
- A power with a constant exponent is expanded at compile time: `x ^ 3`
  becomes `x * x * x`.
- A power with a runtime exponent calls a generated helper function. A
  negative runtime exponent gives `0`, since scores cannot hold fractions.
- Compound assignment works on variables and storage-backed targets.
  `x += y` compiles as `x = x + y`.
- Expressions made only of constants are folded at compile time.

## Integer Division And Modulo

`/` and `%` follow Minecraft's scoreboard `/=` and `%=`: division rounds
toward negative infinity, and modulo is a floor modulus whose sign follows the
divisor. Constant folding uses the same rules, so a folded expression and the
same expression computed from variables agree:

```python
a = -7 / 2      # -4
b = -7 % 3      # 2
c = 1 / 3 * 3   # 0, each step is integer arithmetic
```

`const` declarations are compile-time values that raw commands may substitute,
so `/` in a `const` declaration is real division: `const HALF = 1 / 2`
substitutes `0.5` into `{HALF}`. `%` is the same floor modulus there. Storing
a fractional constant in a score truncates it, with a precision warning.

## Division By Zero

Dividing by a constant zero is a compile error:

```text
Division by zero in constant expression: 10 / 0
```

A divisor that is only known at runtime is not checked. Minecraft rejects the
operation, the command fails, and the target keeps the left operand's value,
so `10 / b` with `b = 0` leaves `10`. Check the divisor first:

```python
def share(total, players):
    if players != 0:
        each = total / players
    else:
        each = 0
```
