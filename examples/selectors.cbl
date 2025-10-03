# Selector Definition Examples
# Shows custom selector aliases

# Define custom selectors
@Players = @a[gamemode=survival]
@Admins = @a[tag=admin]
@Nearby = @e[distance=..10]
@Boss = @e[type=zombie,tag=boss]

def use_selectors():
    """Use custom selector aliases"""
    # Give items to survival players
    as @Players:
        /give @s minecraft:diamond 1

    # Special effects for admins
    as @Admins:
        /effect give @s minecraft:night_vision 999999 0 true

    # Interact with nearby entities
    as @Nearby:
        /data merge entity @s {Glowing:1b}

    # Boss health display
    /execute as @Boss run data modify entity @s CustomNameVisible set value 1b
