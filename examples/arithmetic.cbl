# Arithmetic Examples
# Shows arithmetic operations and operator precedence

def basic_arithmetic():
    """Basic arithmetic operations"""
    x = 10
    y = 5

    # Basic operations
    sum = x + y          # 15
    diff = x - y         # 5
    product = x * y      # 50
    quotient = x / y     # 2
    remainder = x % y    # 0
    power = x ^ 2        # 100

def operator_precedence():
    """Demonstrate operator precedence"""
    # Precedence: ^ > */% > +-
    result1 = 2 + 3 * 4      # 14 (not 20)
    result2 = 10 - 6 / 2     # 7 (not 2)
    result3 = 2 ^ 3 + 1      # 9 (8 + 1, not 2^4)

def complex_expressions():
    """Complex nested expressions"""
    a = 5
    b = 10
    c = 2

    # Complex calculation
    result = a + b * c - 3   # 5 + 20 - 3 = 22

    # Using power
    area = 5 ^ 2             # 25 (5 squared)
    volume = 3 ^ 3           # 27 (3 cubed)

def increment_patterns():
    """Common increment/decrement patterns"""
    counter = 0

    # Self-modification
    counter = counter + 1    # Increment
    counter = counter - 1    # Decrement
    counter = counter * 2    # Double
    counter = counter / 2    # Halve
