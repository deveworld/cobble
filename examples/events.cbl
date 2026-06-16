# Event System Example
import stdlib
from stdlib import event

# Global variables
tick_counter = 0
is_day = True

def on_load():
    """Initialize data pack when loaded"""
    /tellraw @a {"text":"=========================","color":"gold"}
    /tellraw @a {"text":"  Data Pack Loaded!","color":"green","bold":true}
    /tellraw @a {"text":"=========================","color":"gold"}

    # Initialize gamerules
    /gamerule advance_time false
    /gamerule advance_weather false
    /time set day

def on_tick():
    """Called every game tick (20 times per second)"""
    global tick_counter
    tick_counter = tick_counter + 1

    # Every second (20 ticks)
    if tick_counter >= 20:
        tick_counter = 0
        every_second()

    # Particle effects for all players
    # /execute as @a at @s run particle minecraft:happy_villager ~ ~2.5 ~ 0.3 0.3 0.3 0 1 force
    as @a at @s:
        /particle minecraft:happy_villager ~ ~2.5 ~ 0.3 0.3 0.3 0 1 force

    # Check for new players
    # /execute as @a[tag=!initialized] run function namespace:init_player
    as @a[tag=!initialized] at @s:
        # /function namespace:init_player
        init_player()

def every_second():
    """Called every second"""
    global is_day

    # Day/night cycle every 10 seconds
    if is_day:
        /time set night
        /tellraw @a {"text":"Night falls...","color":"blue","italic":true}
        is_day = False
    else:
        /time set day
        /tellraw @a {"text":"Day breaks!","color":"yellow","italic":true}
        is_day = True

def init_player():
    """Initialize new player"""
    /tag @s add initialized
    /tellraw @s {"text":"Welcome to the server!","color":"green","bold":true}
    /give @s minecraft:bread 16
    /give @s minecraft:torch 32
    /effect give @s minecraft:resistance 10 255 true
    /effect give @s minecraft:saturation 1 20 true

    # Spawn particles
    asat @s:
        /particle minecraft:totem_of_undying ~ ~1 ~ 0 0 0 1 100 force

def on_player_death():
    """Handle player death"""
    /tellraw @a [{"selector":"@s","color":"red"},{"text":" has died!","color":"gray"}]
    # /execute at @s run particle minecraft:soul ~ ~ ~ 0.5 1 0.5 0.1 50
    asat @s:
        /particle minecraft:soul ~ ~ ~ 0.5 1 0.5 0.1 50

def on_player_kill():
    """Handle player kill"""
    /tellraw @s {"text":"You got a kill!","color":"gold"}
    asat @s:
        /playsound minecraft:entity.experience_orb.pickup master @s ~ ~ ~ 1 1

# Register event handlers
stdlib.addEventListener(event.LOAD, on_load)
stdlib.addEventListener(event.TICK, on_tick)
