# Execute Block Examples
# Shows execute blocks with modifiers

def execute_as_at():
    """Execute as and at entities"""
    as @a at @s:
        /particle minecraft:flame ~ ~1 ~ 0.2 0.2 0.2 0 5

def execute_with_conditions():
    """Execute with if conditions"""
    as @a if entity @s[tag=special]:
        /say I am special!
        /effect give @s minecraft:speed 10 1

def asat_shorthand():
    """Use 'asat' shorthand for 'as X at X'"""
    asat @e[type=armor_stand]:
        /particle minecraft:witch ~ ~ ~ 0.1 0.1 0.1 0 1

def chained_conditions():
    """Chain multiple conditions"""
    as @a at @s if entity @s[tag=player] if entity @s[gamemode=survival]:
        /give @s minecraft:apple 1

def positioned_execute():
    """Execute at specific position"""
    as @a:
        /execute positioned ~ ~10 ~ run setblock ~ ~ ~ minecraft:diamond_block
