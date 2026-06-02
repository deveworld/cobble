import stdlib
from stdlib import event
import utils

const START_SCORE = 10
score = START_SCORE
health = 20
active = True

@Operators = @a[tag=operator]

datapack.dialog("notice", {"type": "minecraft:notice", "title": {"text": "Smoke Notice"}})

def init():
    """Initialize the smoke-test data pack"""
    announce("Cobble 26.1.2 smoke test loaded")
    reward("@a", 1)
    /scoreboard objectives add smoke dummy "Smoke Test"
    /tellraw @a {"text":"Smoke datapack initialized","color":"green"}
    /dialog clear @a
    /dialog show @a cobble_26_smoke:notice
    /fetchprofile name Notch
    /fetchprofile id 123e4567-e89b-12d3-a456-426614174000
    /waypoint list
    /stopwatch create cobble:smoke
    /stopwatch query cobble:smoke 20.0
    /version

def tick():
    """Exercise generated control-flow commands every tick"""
    global score
    score = score + 1

    if score >= 15:
        /say score high {score}
    elif score >= 12:
        /say score medium {score}
    else:
        /say score low {score}

    match score:
        case 0 to 14:
            /say warming
        case 15 to 30:
            /say active
        case _:
            /say overflow

    for i in range(3):
        /say loop {i}

    as @Builders at @s:
        /particle flame ~ ~1 ~

def admin_probe(player):
    """Exercise commands that require a player argument"""
    /transfer example.org 25565 {player}
    /swing {player} mainhand
    /return run say nested return ok
    /test run minecraft:always_pass 1 true

def operator_probe():
    as @Operators:
        /tellraw @s {"text":"Operator probe","color":"yellow"}

stdlib.addEventListener(event.LOAD, init)
stdlib.addEventListener(event.TICK, tick)
