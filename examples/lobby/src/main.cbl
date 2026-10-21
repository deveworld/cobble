"""Lobby essentials: a drop-in pack for small survival servers.

Players need no operator permission:
    /trigger menu     open the server menu (also on the pause screen)
    /trigger spawn    teleport to spawn, with a cooldown
    /trigger stats    show playtime, deaths, and mob kills

Operators:
    /function lobby:set_spawn    make your position the spawn point
    /function lobby:uninstall    remove the pack's scoreboards

Player progress lives in scoreboards, so it survives /reload and restarts.
Module-level Cobble variables are reset on every load, so they only hold
per-session state such as the tick counter below.
"""

import stdlib
from stdlib import event, datapack, item_component, text
from config import SAFE_RADIUS, TIP_INTERVAL_TICKS
from commands import use_menu, use_spawn, use_stats
import menus

tick_count = 0
tip_index = 0

datapack.loot_table("starter_kit", {
    "type": "minecraft:chest",
    "pools": [{
        "rolls": 1,
        "entries": [
            {
                "type": "minecraft:item",
                "name": "minecraft:stone_sword",
                "modifier": {
                    "type": "minecraft:set_components",
                    "components": item_component.components(
                        item_component.custom_name(text.colored("Starter Sword", "aqua")),
                        item_component.lore([text.plain("A gift from the server")])
                    )
                }
            }
        ]
    }, {
        "rolls": 1,
        "entries": [{"type": "minecraft:item", "name": "minecraft:stone_pickaxe"}]
    }, {
        "rolls": 1,
        "entries": [{
            "type": "minecraft:item",
            "name": "minecraft:bread",
            "modifier": {"type": "minecraft:set_count", "count": 16}
        }]
    }, {
        "rolls": 1,
        "entries": [{
            "type": "minecraft:item",
            "name": "minecraft:torch",
            "modifier": {"type": "minecraft:set_count", "count": 32}
        }]
    }]
})


def on_load():
    /scoreboard objectives add lobby_playtime minecraft.custom:minecraft.play_time "Playtime"
    /scoreboard objectives add lobby_deaths deathCount {"text":"Deaths","color":"red"}
    /scoreboard objectives add lobby_kills totalKillCount "Kills"
    /scoreboard objectives add lobby_left minecraft.custom:minecraft.leave_game
    /scoreboard objectives add lobby_cooldown dummy
    /scoreboard objectives add lobby_data dummy
    /scoreboard objectives add menu trigger "Server menu"
    /scoreboard objectives add spawn trigger "Teleport to spawn"
    /scoreboard objectives add stats trigger "Your stats"
    /scoreboard objectives setdisplay list lobby_deaths
    enable_triggers()
    /schedule function lobby:show_tip {TIP_INTERVAL_TICKS}t replace


def on_tick():
    global tick_count
    as @a[tag=!lobby_joined] at @s:
        first_join()
    as @a[scores={lobby_left=1..}]:
        welcome_back()
    as @a[scores={menu=1..}]:
        use_menu()
    as @a[scores={spawn=1..}] at @s:
        use_spawn()
    as @a[scores={stats=1..}]:
        use_stats()
    /scoreboard players remove @a[scores={lobby_cooldown=1..}] lobby_cooldown 1
    tick_count += 1
    if tick_count >= 20:
        tick_count = 0
        every_second()


def every_second():
    # Spawn is a safe zone: regeneration, full hunger, and no damage.
    at @Spawn as @a[distance=..{SAFE_RADIUS}]:
        /effect give @s minecraft:regeneration 3 1 true
        /effect give @s minecraft:resistance 3 4 true
        /effect give @s minecraft:saturation 1 0 true


def enable_triggers():
    """Let everyone online use the trigger commands again."""
    /scoreboard players enable @a menu
    /scoreboard players enable @a spawn
    /scoreboard players enable @a stats


def first_join():
    """Runs once per player, the first time they join."""
    /tag @s add lobby_joined
    /scoreboard players add #players lobby_data 1
    /scoreboard players enable @s menu
    /scoreboard players enable @s spawn
    /scoreboard players enable @s stats
    /loot give @s loot lobby:starter_kit
    if entity @Spawn:
        /tp @s @Spawn
    player_number = 0
    store result score player_number temp:
        /scoreboard players get #players lobby_data
    /tellraw @a ["",{"selector":"@s","color":"gold"},{"text":" joined for the first time! Welcome, player #{player_number}.","color":"yellow"}]
    text.subtitle("@s", text.colored("Press Esc for the server menu", "gray"))
    text.title("@s", text.colored("Welcome!", "gold"))
    /playsound minecraft:ui.toast.challenge_complete master @s


def welcome_back():
    /scoreboard players reset @s lobby_left
    text.actionbar("@s", text.colored("Welcome back!", "green"))


def show_tip():
    """Posts the next server tip, then schedules itself again."""
    global tip_index
    match tip_index:
        case 0:
            text.tellraw("@a", text.colored("[Tip] Type /trigger spawn to get back to spawn.", "aqua"))
        case 1:
            text.tellraw("@a", text.colored("[Tip] Type /trigger stats to see your playtime.", "aqua"))
        case _:
            text.tellraw("@a", text.colored("[Tip] Press Esc and open the server menu for everything else.", "aqua"))
    tip_index = (tip_index + 1) % 3
    /schedule function lobby:show_tip {TIP_INTERVAL_TICKS}t replace


def set_spawn():
    """Operator command: `/function lobby:set_spawn` where spawn should be."""
    /kill @e[type=minecraft:marker,tag=lobby_spawn]
    align xyz positioned ~0.5 ~ ~0.5:
        /summon minecraft:marker ~ ~ ~ {Tags:["lobby_spawn"]}
        # Store the operator's facing so arrivals look the same way.
        /tp @Spawn ~ ~ ~ ~ 0
        /forceload add ~ ~
        /setworldspawn ~ ~ ~
    /tellraw @s {"text":"Lobby spawn set here.","color":"green"}


def uninstall():
    """Operator command: run before deleting the pack."""
    for objective in ["lobby_playtime", "lobby_deaths", "lobby_kills", "lobby_left", "lobby_cooldown", "lobby_data", "menu", "spawn", "stats"]:
        /scoreboard objectives remove {objective}
    /kill @e[type=minecraft:marker,tag=lobby_spawn]
    /schedule clear lobby:show_tip
    /tellraw @s {"text":"Lobby scoreboards removed. Delete the pack before the next /reload.","color":"yellow"}


stdlib.addEventListener(event.LOAD, on_load)
stdlib.addEventListener(event.TICK, on_tick)
