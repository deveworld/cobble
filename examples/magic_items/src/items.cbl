"""Item definitions: loot tables to hand them out, recipes to craft them.

Each item is a carrot on a stick, the classic right-click detector: using one
raises the player's `minecraft.used:minecraft.carrot_on_a_stick` score. The
`custom_data` component tells the three items apart, `item_model` makes
each look like a different vanilla item, and `item_name` gives it a name that
is not italic like a renamed item.

Recipe results cannot reference a loot table, so each recipe repeats the
item's components.
"""

from stdlib import datapack, item_component, text

datapack.loot_table("items/blink_rod", {
    "type": "minecraft:chest",
    "pools": [{
        "rolls": 1,
        "entries": [{
            "type": "minecraft:item",
            "name": "minecraft:carrot_on_a_stick",
            "modifier": {
                "type": "minecraft:set_components",
                "components": item_component.components(
                    item_component.lore([text.colored("Right-click to dash forward", "gray")]),
                    {
                        "minecraft:item_name": {"text": "Blink Rod", "color": "aqua"},
                        "minecraft:custom_data": {"magic_item": "blink"},
                        "minecraft:item_model": "minecraft:breeze_rod",
                        "minecraft:enchantment_glint_override": True
                    }
                )
            }
        }]
    }]
})

datapack.loot_table("items/healing_staff", {
    "type": "minecraft:chest",
    "pools": [{
        "rolls": 1,
        "entries": [{
            "type": "minecraft:item",
            "name": "minecraft:carrot_on_a_stick",
            "modifier": {
                "type": "minecraft:set_components",
                "components": item_component.components(
                    item_component.lore([text.colored("Right-click to heal everyone nearby", "gray")]),
                    {
                        "minecraft:item_name": {"text": "Healing Staff", "color": "light_purple"},
                        "minecraft:custom_data": {"magic_item": "heal"},
                        "minecraft:item_model": "minecraft:blaze_rod",
                        "minecraft:enchantment_glint_override": True
                    }
                )
            }
        }]
    }]
})

datapack.loot_table("items/frost_wand", {
    "type": "minecraft:chest",
    "pools": [{
        "rolls": 1,
        "entries": [{
            "type": "minecraft:item",
            "name": "minecraft:carrot_on_a_stick",
            "modifier": {
                "type": "minecraft:set_components",
                "components": item_component.components(
                    item_component.lore([text.colored("Right-click to freeze mobs in a line", "gray")]),
                    {
                        "minecraft:item_name": {"text": "Frost Wand", "color": "blue"},
                        "minecraft:custom_data": {"magic_item": "frost"},
                        "minecraft:item_model": "minecraft:end_rod",
                        "minecraft:enchantment_glint_override": True
                    }
                )
            }
        }]
    }]
})

datapack.recipe("blink_rod", {
    "type": "minecraft:crafting_shaped",
    "pattern": [" E", "B "],
    "key": {"E": "minecraft:ender_pearl", "B": "minecraft:breeze_rod"},
    "result": {
        "id": "minecraft:carrot_on_a_stick",
        "components": {
            "minecraft:item_name": {"text": "Blink Rod", "color": "aqua"},
            "minecraft:lore": [{"text": "Right-click to dash forward", "color": "gray"}],
            "minecraft:custom_data": {"magic_item": "blink"},
            "minecraft:item_model": "minecraft:breeze_rod",
            "minecraft:enchantment_glint_override": True
        }
    }
})

datapack.recipe("healing_staff", {
    "type": "minecraft:crafting_shaped",
    "pattern": [" G", "B "],
    "key": {"G": "minecraft:golden_apple", "B": "minecraft:blaze_rod"},
    "result": {
        "id": "minecraft:carrot_on_a_stick",
        "components": {
            "minecraft:item_name": {"text": "Healing Staff", "color": "light_purple"},
            "minecraft:lore": [{"text": "Right-click to heal everyone nearby", "color": "gray"}],
            "minecraft:custom_data": {"magic_item": "heal"},
            "minecraft:item_model": "minecraft:blaze_rod",
            "minecraft:enchantment_glint_override": True
        }
    }
})

datapack.recipe("frost_wand", {
    "type": "minecraft:crafting_shaped",
    "pattern": [" I", "R "],
    "key": {"I": "minecraft:blue_ice", "R": "minecraft:end_rod"},
    "result": {
        "id": "minecraft:carrot_on_a_stick",
        "components": {
            "minecraft:item_name": {"text": "Frost Wand", "color": "blue"},
            "minecraft:lore": [{"text": "Right-click to freeze mobs in a line", "color": "gray"}],
            "minecraft:custom_data": {"magic_item": "frost"},
            "minecraft:item_model": "minecraft:end_rod",
            "minecraft:enchantment_glint_override": True
        }
    }
})
