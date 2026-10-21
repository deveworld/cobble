"""What each item does. Every function runs as the player, at their feet."""

from stdlib import text
from config import BLINK_RANGE, HEAL_RADIUS, FROST_RANGE, FROST_DAMAGE


def blink():
    """Dash up to BLINK_RANGE blocks the way you face, stopping at walls.

    Returns how far the player moved, or 0 when something is in the way.
    """
    # The loop unrolls at compile time: one check per block, and each check
    # only runs while every block before it was clear.
    distance = 0
    for step in range(1, BLINK_RANGE + 1):
        if distance == step - 1:
            rotated ~ 0 positioned ^ ^ ^{step} if block ~ ~ ~ #magic_items:passable if block ~ ~1 ~ #magic_items:passable:
                distance = step
    if distance > 0:
        /particle minecraft:reverse_portal ~ ~1 ~ 0.3 0.6 0.3 0.05 30
        dash(distance)
        at @s:
            /particle minecraft:portal ~ ~1 ~ 0.3 0.6 0.3 0.5 30
            /playsound minecraft:entity.enderman.teleport player @a ~ ~ ~
    else:
        text.actionbar("@s", text.colored("Something is in the way", "red"))
    return distance


def dash(distance):
    rotated ~ 0:
        /tp @s ^ ^ ^{distance}


def heal():
    """Heal everyone within HEAL_RADIUS, the user included."""
    healed = 0
    store result score healed temp:
        /execute if entity @a[distance=..{HEAL_RADIUS}]
    as @a[distance=..{HEAL_RADIUS}]:
        /effect give @s minecraft:instant_health 1 0 true
        /effect give @s minecraft:regeneration 5 0 true
    /particle minecraft:heart ~ ~1 ~ 2 0.5 2 0 25
    /playsound minecraft:block.amethyst_block.chime player @a ~ ~ ~ 1 1.2
    /title @s actionbar {"text":"Healed {healed} players","color":"light_purple"}


def frost():
    """Send a frost ray up to FROST_RANGE blocks; it stops at the first wall."""
    /tag @s add magic_caster
    reach = 0
    for step in range(1, FROST_RANGE + 1):
        if reach == step - 1:
            anchored eyes positioned ^ ^ ^{step} if block ~ ~ ~ #magic_items:passable:
                reach = step
                /particle minecraft:snowflake ~ ~ ~ 0.15 0.15 0.15 0 3
                # `dx=0,dy=0,dz=0` selects mobs whose hitbox overlaps the
                # one-block cube around this point of the ray.
                positioned ~-0.5 ~-0.5 ~-0.5 as @e[type=#magic_items:frost_targets,dx=0,dy=0,dz=0,tag=!magic_frozen]:
                    freeze_target()
    /tag @e[tag=magic_frozen] remove magic_frozen
    /tag @s remove magic_caster
    /playsound minecraft:block.powder_snow.break player @a ~ ~ ~ 1 0.8


def freeze_target():
    """Runs as a mob the ray touched. The tag keeps one cast from hitting it twice."""
    /tag @s add magic_frozen
    /effect give @s minecraft:slowness 5 3
    # Crediting the caster makes the kill count for them.
    /damage @s {FROST_DAMAGE} minecraft:freeze by @a[tag=magic_caster,limit=1]
    /particle minecraft:item_snowball ~ ~1 ~ 0.3 0.5 0.3 0 12
