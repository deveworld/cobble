"""Magic items: three custom right-click items with cooldowns.

    Blink Rod       dash forward, stopping before walls
    Healing Staff   heal yourself and everyone nearby
    Frost Wand      a ray that slows and damages mobs

Craft them (see items.cbl for the recipes) or, as an operator, get all three
with `/function magic_items:give_all`.
"""

import stdlib
from stdlib import event, datapack, text
from config import BLINK_COOLDOWN_TICKS, HEAL_COOLDOWN_TICKS, FROST_COOLDOWN_TICKS
from abilities import blink, heal, frost
import items

# Blocks a blink or a frost ray can pass through.
datapack.block_tag("passable", [
    "#minecraft:air",
    "#minecraft:small_flowers",
    "minecraft:short_grass",
    "minecraft:tall_grass",
    "minecraft:fern",
    "minecraft:large_fern",
    "minecraft:snow",
    "minecraft:light",
    "minecraft:water"
])

# Mobs the frost ray hurts: hostile ones only.
datapack.entity_type_tag("frost_targets", [
    "#minecraft:skeletons",
    "#minecraft:zombies",
    "#minecraft:raiders",
    "minecraft:creeper",
    "minecraft:spider",
    "minecraft:cave_spider",
    "minecraft:enderman",
    "minecraft:slime",
    "minecraft:magma_cube",
    "minecraft:blaze",
    "minecraft:breeze",
    "minecraft:phantom",
    "minecraft:silverfish",
    "minecraft:creaking"
])


def on_load():
    /scoreboard objectives add magic_used minecraft.used:minecraft.carrot_on_a_stick
    /scoreboard objectives add magic_cd_blink dummy
    /scoreboard objectives add magic_cd_heal dummy
    /scoreboard objectives add magic_cd_frost dummy


def on_tick():
    as @a[scores={magic_used=1..}] at @s:
        use_held_item()
    /scoreboard players remove @a[scores={magic_cd_blink=1..}] magic_cd_blink 1
    /scoreboard players remove @a[scores={magic_cd_heal=1..}] magic_cd_heal 1
    /scoreboard players remove @a[scores={magic_cd_frost=1..}] magic_cd_frost 1


def use_held_item():
    """Runs when a player right-clicks with any carrot on a stick."""
    /scoreboard players reset @s magic_used
    if items entity @s weapon.mainhand *[minecraft:custom_data~{magic_item:"blink"}]:
        try_blink()
    if items entity @s weapon.mainhand *[minecraft:custom_data~{magic_item:"heal"}]:
        try_heal()
    if items entity @s weapon.mainhand *[minecraft:custom_data~{magic_item:"frost"}]:
        try_frost()


def try_blink():
    left = 0
    store result score left temp:
        /scoreboard players get @s magic_cd_blink
    if left > 0:
        show_recharge(left)
    else:
        moved = blink()
        # A blocked blink costs nothing.
        if moved > 0:
            /scoreboard players set @s magic_cd_blink {BLINK_COOLDOWN_TICKS}


def try_heal():
    left = 0
    store result score left temp:
        /scoreboard players get @s magic_cd_heal
    if left > 0:
        show_recharge(left)
    else:
        heal()
        /scoreboard players set @s magic_cd_heal {HEAL_COOLDOWN_TICKS}


def try_frost():
    left = 0
    store result score left temp:
        /scoreboard players get @s magic_cd_frost
    if left > 0:
        show_recharge(left)
    else:
        frost()
        /scoreboard players set @s magic_cd_frost {FROST_COOLDOWN_TICKS}


def show_recharge(ticks):
    seconds = ticks / 20
    tenths = (ticks % 20) / 2
    /title @s actionbar {"text":"Recharging: {seconds}.{tenths}s","color":"red"}
    /playsound minecraft:block.note_block.bass player @s ~ ~ ~ 0.6 0.5


def give_all():
    """Operator command: `/function magic_items:give_all`."""
    /loot give @s loot magic_items:items/blink_rod
    /loot give @s loot magic_items:items/healing_staff
    /loot give @s loot magic_items:items/frost_wand


stdlib.addEventListener(event.LOAD, on_load)
stdlib.addEventListener(event.TICK, on_tick)
