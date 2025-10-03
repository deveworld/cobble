# Type System Examples
# Shows static type inference and type safety

# Module-level variables with inferred types
score = 0           # Type: Integer
active = True       # Type: Boolean
player_count = 10   # Type: Integer

def type_inference():
    """Types are automatically inferred"""
    x = 100              # Type: Integer
    is_active = True     # Type: Boolean
    result = x + 50      # Type: Integer (arithmetic result)
    comparison = x > 50  # Type: Boolean (comparison result)

def type_safety():
    """Types cannot change"""
    value = 42
    # value = True  # ERROR: Cannot reassign Integer to Boolean

    value = value + 10  # OK: Still Integer
    value = value * 2   # OK: Still Integer

def const_example():
    """Compile-time constants"""
    const MAX_HEALTH = 100
    const ATTACK_DAMAGE = 10
    const DEFENSE = 5

    # Constants are replaced at compile time
    health = MAX_HEALTH
    damage = ATTACK_DAMAGE + DEFENSE

def type_errors_prevented():
    """Common type errors that Cobble catches"""
    counter = 10

    # This would cause a type error:
    # counter = "hello"  # ERROR: Cannot reassign Integer to String

    # This is OK:
    counter = counter + 5  # Still Integer
