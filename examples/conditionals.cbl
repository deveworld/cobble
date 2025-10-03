# Conditional Examples
# Shows if/elif/else and match statements

def if_example():
    """Basic if statement"""
    score = 100

    if score >= 100:
        /tellraw @a {"text":"Perfect score!", "color":"gold"}
    elif score >= 80:
        /tellraw @a {"text":"Great job!", "color":"green"}
    elif score >= 60:
        /tellraw @a {"text":"Good work!", "color":"yellow"}
    else:
        /tellraw @a {"text":"Keep trying!", "color":"red"}

def boolean_operators():
    """Show boolean operators: and, or, not"""
    x = 10
    y = 20

    if x > 5 and y < 25:
        /say Both conditions are true!

    if x == 0 or y == 0:
        /say At least one is zero

    if not x == 0:
        /say X is not zero

def match_example():
    """Match statement with ranges"""
    score = 75

    match score:
        case 0 to 59:
            /say Grade: F
        case 60 to 69:
            /say Grade: D
        case 70 to 79:
            /say Grade: C
        case 80 to 89:
            /say Grade: B
        case 90 to 100:
            /say Grade: A
        case _:
            /say Invalid score
