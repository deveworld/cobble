import stdlib
from stdlib import event
import rewards

datapack.function_tag("utility", ["cobble_resource_authoring:init"])
datapack.item_tag("reward_items", ["minecraft:diamond", "minecraft:emerald"])
datapack.block_tag("building_blocks", ["minecraft:stone", "minecraft:deepslate"])
datapack.entity_type_tag("hostile_targets", ["minecraft:zombie", "minecraft:skeleton"])
datapack.predicate("checks/always", {
    "condition": "minecraft:random_chance",
    "chance": 1
})
datapack.advancement("story/root", {
    "criteria": {"load": {"trigger": "minecraft:tick"}}
})
datapack.loot_table("chests/empty_reward", {"type": "minecraft:empty"})
datapack.recipe("stonecutting/polished_granite", {
    "type": "minecraft:stonecutting",
    "ingredient": "minecraft:granite",
    "result": {"id": "minecraft:polished_granite"}
})
datapack.item_modifier("items/reward_name", {
    "function": "minecraft:set_name",
    "name": {"text": "Cobble Reward", "color": "gold"}
})
datapack.dialog("notice", {
    "type": "minecraft:notice",
    "title": {"text": "Resources Ready"}
})

def init():
    """Load generated resources and announce the fixture."""
    /tellraw @a {"text":"Resource authoring fixture loaded","color":"green"}
    /dialog clear @a
    /dialog show @a cobble_resource_authoring:notice
    grant_reward("@a")

def tick():
    pass

stdlib.addEventListener(event.LOAD, init)
stdlib.addEventListener(event.TICK, tick)
