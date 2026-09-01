
def route(player):
    mode = 1
    match mode:
        case 0:
            /say zero
        case 1 to 3:
            /say range
        case _:
            /say other
    asat @s:
        /say asat
    as @a at @s if entity @s:
        /say execute
