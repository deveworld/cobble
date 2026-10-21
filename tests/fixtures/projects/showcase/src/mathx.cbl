"""Integer helpers shared by the showcase (an imported module)."""


def clamp(value, low, high):
    """Clamp value into the inclusive range [low, high]."""
    if value < low:
        return low
    elif value > high:
        return high
    return value


def sign(value):
    if value > 0:
        return 1
    elif value < 0:
        return -1
    return 0


def sum_to(n):
    """1 + 2 + ... + n, computed with a while loop."""
    sum_total = 0
    sum_i = 1
    while sum_i <= n:
        sum_total += sum_i
        sum_i += 1
    return sum_total


def is_even(value):
    if value % 2 == 0:
        return 1
    return 0
