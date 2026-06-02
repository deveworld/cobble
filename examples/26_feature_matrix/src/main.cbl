import stdlib
from stdlib import event
import systems

const BASE_SCORE = 7
const BONUS = 3
const MAX_STAGE = 4

score = BASE_SCORE
energy = 5
stage = 0
enabled = True
status = "boot"

@Players = @a[gamemode=!spectator]
@Markers = @e[type=armor_stand,tag=matrix_marker]

datapack.function_tag("utility", ["cobble_26_feature_matrix:load"])
datapack.function_tag("minecraft:load", ["cobble_26_feature_matrix:load"])
datapack.predicate("always_true", {
    "condition": "minecraft:random_chance",
    "chance": 1
})
datapack.predicate("other_matrix:checks/always_true", {
    "condition": "minecraft:random_chance",
    "chance": 1
})
datapack.dialog("notice", {
    "type": "minecraft:notice",
    "title": {"text": "Feature Matrix"}
})

def load():
    """Run once when the pack loads"""
    setup_scoreboards()
    announce("Feature matrix online")
    give_kit("@a", "diamond", 2)
    text.tellraw("@a", {"text":"Feature matrix loaded","color":"aqua","bold":True})
    text.title("@a", {"text":"Matrix score"})
    score.set("matrix_ready", 1)
    random.int("matrix_roll", 1, 6)
    timer.set("matrix_timer", 20)
    storage.set("status", {"state":"loaded","enabled":True})
    storage.append("events", "loaded")
    storage.read_score("matrix_state_count", "events", 1)
    schedule.once("tick", "20t", "replace")
    entity.tag_add("@a", "matrix_loader")
    root = math.sqrt(100)
    /datapack list enabled
    /random roll 1..6
    /dialog clear @a
    /waypoint list
    /stopwatch create cobble:matrix
    /stopwatch query cobble:matrix 20.0
    /version

def tick():
    """Exercise calculations and control flow"""
    global score
    global energy
    global stage
    global status

    score = score + BONUS
    doubled = score * 2
    combined = doubled + 5
    remainder = combined % 4
    power = 3 ^ 3
    status = "running"

    timer.tick("matrix_timer")
    timer.done("matrix_timer")

    if score > 20 and not energy == 0:
        /tellraw @Players {"text":"High score ","color":"gold"}
    elif score == 10 or energy == 0:
        /tellraw @Players {"text":"Threshold reached","color":"yellow"}
    else:
        /tellraw @Players {"text":"Matrix warming","color":"gray"}

    match remainder:
        case 0:
            stage = 1
            /say stage zero
        case 1 to 2:
            stage = 2
            /say stage middle
        case _:
            stage = MAX_STAGE
            /say stage fallback

    while energy > 0:
        /say draining {energy}
        energy = energy - 1

    for i in range(6) by 2:
        /tellraw @a {"text":"even loop {i}","color":"green"}

    for n in range(8) by -3:
        /say reverse loop {n}

    as @Players at @s if entity @s[distance=..32]:
        /particle minecraft:happy_villager ~ ~1 ~ 0.25 0.25 0.25 0 3

    /execute positioned ~ ~2 ~ rotated ~ 0 run particle minecraft:flame ~ ~ ~ 0 0 0 0 1
    /tellraw @a {"text":"Status: {status}","color":"blue"}

def admin_audit(player):
    """Exercise parameterized commands and nested Minecraft commands"""
    /tag {player} add matrix_admin
    /effect give {player} minecraft:glowing 5 0 true
    /playsound minecraft:block.note_block.pling master {player} ~ ~ ~ 1 1
    /schedule function cobble_26_feature_matrix:tick 20t replace
    /return run say audit complete

stdlib.addEventListener(event.LOAD, load)
stdlib.addEventListener(event.TICK, tick)
