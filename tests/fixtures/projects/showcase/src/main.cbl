"""Cobble Arena: a small wave-based minigame that doubles as a feature tour.

The game half (rounds, waves, rewards) only does visible work when players are
online. The self-test half (`selftest.cbl`) verifies the compiled commands on a
headless server.
"""

import stdlib
from stdlib import event, datapack, item_component, text
import mathx
import arena
import selftest

const MAX_WAVES = 3
const WAVE_SIZE = 2
const REWARD_DIAMONDS = MAX_WAVES * 2

# Module-level state, initialized when the pack loads.
round = 0
wave = 0
ticks_seen = 0
running = False
status = "idle"
arena_title = "Cobble Arena"

@Players = @a[gamemode=!spectator]
@Spectators = @a[gamemode=spectator]
@Mobs = @e[type=minecraft:zombie,tag=arena_mob]

define @Mob = @e[type=minecraft:zombie,tag=arena_mob]
create {
    "Tags": ["arena_mob", "arena_entity"],
    "PersistenceRequired": True,
    "CustomName": {"text": "Arena Zombie", "color": "red"},
    "IsBaby": False
}
end

# ---- Data pack resources (Minecraft 26.3 JSON shapes) ----

datapack.function_tag("hooks/round_start", [
    "cobble_showcase:round_hook_a",
    "cobble_showcase:round_hook_b"
])
datapack.block_tag("arena_floor", [
    "minecraft:stone",
    {"id": "minecraft:deepslate", "required": False}
])
datapack.item_tag("rewards", ["minecraft:diamond", "minecraft:emerald"])
datapack.entity_type_tag("arena_types", [
    "minecraft:marker",
    "minecraft:armor_stand",
    "minecraft:zombie"
], True)

datapack.predicate("checks/always", {
    "type": "minecraft:all_of",
    "terms": [
        {"type": "minecraft:random_chance", "chance": 1},
        {"type": "minecraft:inverted", "term": {"type": "minecraft:random_chance", "chance": 0}}
    ]
})
datapack.predicate("checks/sneaking", {
    "type": "minecraft:entity_properties",
    "entity": "this",
    "predicate": {"flags": {"is_sneaking": True}}
})

datapack.item_modifier("items/reward_name", {
    "type": "minecraft:set_components",
    "components": item_component.components(
        item_component.custom_name(text.colored("Arena Reward", "gold")),
        item_component.lore([text.plain("Won in the Cobble Arena")]),
        item_component.unbreakable()
    )
})

datapack.loot_table("chests/reward", {
    "type": "minecraft:chest",
    "pools": [{
        "rolls": 1,
        "condition": {"type": "minecraft:random_chance", "chance": 1},
        "entries": [{
            "type": "minecraft:item",
            "name": "minecraft:diamond",
            "modifier": {"type": "minecraft:set_count", "count": 2}
        }]
    }]
})
datapack.loot_table("chests/bonus", {
    "type": "minecraft:chest",
    "pools": [{
        "rolls": 1,
        "entries": [{"type": "minecraft:tag", "items": "#minecraft:logs", "expand": True}]
    }]
})

datapack.advancement("arena/root", {
    "display": {
        "icon": {"id": "minecraft:iron_sword"},
        "title": {"text": "Cobble Arena"},
        "description": {"text": "Join the arena"},
        "background": "minecraft:gui/advancements/backgrounds/stone"
    },
    "criteria": {"joined": {"trigger": "minecraft:tick"}}
})
datapack.advancement("arena/champion", {
    "parent": "cobble_showcase:arena/root",
    "display": {
        "icon": {"id": "minecraft:diamond"},
        "title": {"text": "Champion"},
        "description": {"text": "Clear every wave"},
        "frame": "challenge"
    },
    "criteria": {"manual": {"trigger": "minecraft:impossible"}}
})

datapack.recipe("arena_key", {
    "type": "minecraft:crafting_shaped",
    "pattern": ["#", "I"],
    "key": {"#": "minecraft:diamond", "I": "minecraft:stick"},
    "result": {"id": "minecraft:tripwire_hook", "count": 1}
})
datapack.recipe("smelt_rubble", {
    "type": "minecraft:smelting",
    "ingredient": "minecraft:cobblestone",
    "result": {"id": "minecraft:stone"},
    "experience": 0.1,
    "cookingtime": 100
})
datapack.recipe("cut_rubble", {
    "type": "minecraft:stonecutting",
    "ingredient": "minecraft:stone",
    "result": {"id": "minecraft:stone_bricks", "count": 1}
})

datapack.dialog("welcome", {
    "type": "minecraft:notice",
    "title": {"text": "Welcome to the Cobble Arena"},
    "body": {"type": "minecraft:plain_message", "contents": {"text": "Survive every wave."}},
    "action": {"label": {"text": "Fight"}}
})


# ---- Game logic ----

def on_load():
    """Set up objectives, the arena chunks, and the HUD."""
    /forceload add -3 -3 3 3
    /scoreboard objectives add arena_score dummy {"text":"Arena","color":"gold"}
    score.objective.display("sidebar", "arena_score")
    team.add("arena_players", text.colored("Arena", "gold"))
    team.modify("arena_players", "color", "gold")
    team.modify("arena_players", "friendlyFire", "false")
    /say Cobble showcase loaded: {arena_title}


def on_tick():
    """Advance the round clock; only does visible work while running."""
    global ticks_seen
    ticks_seen += 1
    if running and ticks_seen % 20 == 0:
        second_tick()


def second_tick():
    global wave
    as @Players at @s if entity @s[y=90,dy=20]:
        /particle minecraft:happy_villager ~ ~2 ~ 0.3 0.3 0.3 0 2
    alive = 0
    as @Mobs:
        alive += 1
    if alive == 0 and wave < MAX_WAVES:
        wave += 1
        spawn_wave(wave)
    elif alive == 0:
        finish_round()


def start_round():
    """Teleport players in, announce, and spawn the first wave."""
    global round
    global running
    global status
    global wave
    round += 1
    wave = 0
    running = True
    status = "running"
    arena.build_arena()
    team.join("arena_players", "@Players")
    as @Players:
        /tp @s 0 101 0
        /dialog show @s cobble_showcase:welcome
        /advancement grant @s only cobble_showcase:arena/root
    /tellraw @a [{"text":"Round {round} ","color":"gold","bold":true},{"text":"status: {status}","color":"gray"}]
    text.title("@a", text.colored("Round start", "gold"))
    bossbar.add("waves", {"text": "Waves"})
    bossbar.set_max("waves", MAX_WAVES)
    bossbar.set_players("waves", "@a")


def spawn_wave(number):
    """Spawn WAVE_SIZE zombies per wave number."""
    for slot in range(WAVE_SIZE):
        positioned 0 100 0 rotated {slot}0 0:
            /summon minecraft:lightning_bolt ^ ^ ^3
    count = number * WAVE_SIZE
    spawned = 0
    while spawned < count:
        positioned 0 100 0:
            create @Mob
        spawned += 1
    /tellraw @a {"text":"Wave {number} with {count} mobs","color":"red"}
    store result bossbar cobble_showcase:waves value:
        /scoreboard players get wave temp


def finish_round():
    global running
    global status
    running = False
    status = "won"
    reward = mathx.clamp(round * 2, 1, REWARD_DIAMONDS)
    as @Players at @s:
        give_reward("@s", reward)
        /advancement grant @s only cobble_showcase:arena/champion
        /playsound minecraft:ui.toast.challenge_complete master @s ~ ~ ~ 1 1
    match round:
        case 1:
            text.tellraw("@a", text.colored("First win!", "green"))
        case 2 to 4:
            text.tellraw("@a", text.colored("Veteran win", "aqua"))
        case _:
            text.tellraw("@a", text.colored("Arena legend", "light_purple"))
    team.leave("@Players")
    bossbar.remove("waves")


def give_reward(player, amount):
    """Give named diamonds; called with macro arguments."""
    /give {player} minecraft:diamond[minecraft:custom_name={"text":"Arena Reward","color":"gold"}] {amount}
    /loot give {player} loot cobble_showcase:chests/reward
    /item modify entity {player} weapon.mainhand cobble_showcase:items/reward_name
    /tellraw {player} {"text":"You received {amount} diamonds","color":"gold"}


def sneak_check():
    as @Players if predicate cobble_showcase:checks/sneaking:
        /title @s actionbar {"text":"Sneaking in the arena","color":"gray"}
    as @Spectators:
        pass


stdlib.addEventListener(event.LOAD, on_load)
stdlib.addEventListener(event.TICK, on_tick)
