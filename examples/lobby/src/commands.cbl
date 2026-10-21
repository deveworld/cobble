"""Handlers for the player trigger commands.

A trigger objective is locked again after each use, so every handler resets
the score and re-enables it.
"""

from stdlib import text
from config import SPAWN_COOLDOWN_TICKS


def use_menu():
    /scoreboard players reset @s menu
    /scoreboard players enable @s menu
    /dialog show @s lobby:menu


def use_spawn():
    /scoreboard players reset @s spawn
    /scoreboard players enable @s spawn
    # `store result` writes 0 when the score is unset or the test fails.
    cooldown_left = 0
    store result score cooldown_left temp:
        /scoreboard players get @s lobby_cooldown
    spawn_exists = 0
    store result score spawn_exists temp:
        /execute if entity @Spawn
    if cooldown_left > 0:
        wait_seconds = (cooldown_left + 19) / 20
        /tellraw @s {"text":"You can teleport to spawn again in {wait_seconds}s.","color":"red"}
    elif spawn_exists == 0:
        /tellraw @s {"text":"No spawn is set yet. An operator can run /function lobby:set_spawn.","color":"red"}
    else:
        /particle minecraft:portal ~ ~1 ~ 0.3 0.6 0.3 0.5 40
        /tp @s @Spawn
        /playsound minecraft:entity.enderman.teleport master @s
        /scoreboard players set @s lobby_cooldown {SPAWN_COOLDOWN_TICKS}
        text.actionbar("@s", text.colored("Teleported to spawn", "green"))


def use_stats():
    /scoreboard players reset @s stats
    /scoreboard players enable @s stats
    played_ticks = 0
    deaths = 0
    kills = 0
    store result score played_ticks temp:
        /scoreboard players get @s lobby_playtime
    store result score deaths temp:
        /scoreboard players get @s lobby_deaths
    store result score kills temp:
        /scoreboard players get @s lobby_kills
    played_minutes = played_ticks / 1200
    played_hours = played_minutes / 60
    played_minutes = played_minutes % 60
    /tellraw @s {"text":"Your stats","color":"gold","bold":true}
    /tellraw @s ["",{"text":"Playtime: ","color":"gray"},{"text":"{played_hours}h {played_minutes}m","color":"white"}]
    /tellraw @s ["",{"text":"Deaths: ","color":"gray"},{"text":"{deaths}","color":"white"}]
    /tellraw @s ["",{"text":"Mob kills: ","color":"gray"},{"text":"{kills}","color":"white"}]
