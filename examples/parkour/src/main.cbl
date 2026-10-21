"""Parkour course kit: build a course in-game, then players time their runs.

Building (operators), standing where each point should go:
    /function parkour:set_start
    /function parkour:add_checkpoint      (repeat, in course order)
    /function parkour:set_finish
    /function parkour:remove_course       (start over)
    /function parkour:reset_records

Running (everyone):
  - Step on the start. The clock starts when you leave it.
  - Touch every checkpoint in order. Falling, or touching water or lava,
    sends you back to your last checkpoint.
  - Reach the finish to post your time. Your first clear grants an
    advancement and a reward.
    /trigger pk_restart   back to the start
    /trigger pk_leave     stop the current run

Times and records are scoreboards, so they survive /reload and restarts.
"""

import stdlib
from stdlib import event, datapack
from config import TOUCH_RADIUS
from run import start_run, run_tick, use_restart, use_leave
import course

datapack.block_tag("hazards", ["minecraft:water", "minecraft:lava"])

datapack.loot_table("first_clear_reward", {
    "type": "minecraft:chest",
    "pools": [{
        "rolls": 1,
        "entries": [{
            "type": "minecraft:item",
            "name": "minecraft:diamond",
            "modifier": {"type": "minecraft:set_count", "count": 3}
        }]
    }, {
        "rolls": 1,
        "entries": [{"type": "minecraft:item", "name": "minecraft:golden_apple"}]
    }]
})

datapack.advancement("root", {
    "display": {
        "icon": {"id": "minecraft:feather"},
        "title": {"text": "Parkour"},
        "description": {"text": "Courses built on this server"},
        "background": "minecraft:gui/advancements/backgrounds/adventure",
        "show_toast": False,
        "announce_to_chat": False
    },
    "criteria": {"joined": {"trigger": "minecraft:tick"}}
})

datapack.advancement("first_clear", {
    "parent": "parkour:root",
    "display": {
        "icon": {"id": "minecraft:golden_boots"},
        "title": {"text": "Clean Run"},
        "description": {"text": "Finish the parkour course"},
        "frame": "challenge"
    },
    "criteria": {"finished": {"trigger": "minecraft:impossible"}},
    "rewards": {"loot": ["parkour:first_clear_reward"], "experience": 50}
})


def on_load():
    /scoreboard objectives add pk_index dummy
    /scoreboard objectives add pk_data dummy
    /scoreboard objectives add pk_time dummy
    /scoreboard objectives add pk_cp dummy
    /scoreboard objectives add pk_falls dummy
    /scoreboard objectives add pk_best dummy
    /scoreboard objectives add pk_clears dummy {"text":"Parkour clears","color":"gold"}
    /scoreboard objectives add pk_restart trigger
    /scoreboard objectives add pk_leave trigger
    /scoreboard objectives setdisplay sidebar pk_clears
    /scoreboard players enable @a pk_restart
    /scoreboard players enable @a pk_leave


def on_tick():
    as @a[tag=!pk_running] at @s if entity @e[type=minecraft:marker,tag=pk_start,distance=..{TOUCH_RADIUS}]:
        start_run()
    as @a[tag=pk_running] at @s:
        run_tick()
    as @a[scores={pk_restart=1..}]:
        use_restart()
    as @a[scores={pk_leave=1..}]:
        use_leave()


stdlib.addEventListener(event.LOAD, on_load)
stdlib.addEventListener(event.TICK, on_tick)
