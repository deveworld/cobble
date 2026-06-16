# Parkour System Example
import stdlib
from stdlib import event

# Global variables
checkpoint_count = 0
player_time = 0

def create_checkpoint(x, y, z, id):
    """Create a checkpoint at specific coordinates"""
    /summon minecraft:armor_stand {x} {y} {z} {Invisible:1b,Marker:1b,CustomName:'{"text":"Checkpoint_{id}"}'}
    /particle minecraft:end_rod {x} {y} {z} 0.5 1 0.5 0.01 20
    /setblock {x} {y} {z} minecraft:light_weighted_pressure_plate

def on_checkpoint():
    """Player reaches a checkpoint"""
    /spawnpoint @s ~ ~ ~
    /playsound minecraft:entity.player.levelup master @s
    /title @s subtitle {"text":"Checkpoint Saved!","color":"green"}
    /title @s title ""

    # Give effects
    /effect give @s minecraft:instant_health 1 10 true
    /effect give @s minecraft:saturation 1 10 true

def reset_player():
    """Teleport player to their last checkpoint"""
    /tp @s @e[type=armor_stand,name="Checkpoint_*",limit=1,sort=nearest]
    /effect give @s minecraft:resistance 3 255 true
    /tellraw @s {"text":"Teleported to last checkpoint","color":"yellow"}

def start_parkour():
    """Initialize the parkour course"""
    /scoreboard objectives add parkour_time minecraft.custom:minecraft.play_time "Parkour Timer"
    /scoreboard objectives add checkpoints dummy "Checkpoints"

    # Create checkpoints
    create_checkpoint(100, 65, 0, 1)
    create_checkpoint(120, 70, 0, 2)
    create_checkpoint(140, 75, 0, 3)
    create_checkpoint(160, 80, 0, 4)
    create_checkpoint(180, 85, 0, 5)

    /tellraw @a {"text":"Parkour course ready!","color":"green","bold":true}

def finish_parkour():
    """Player completes the parkour"""
    # Calculate time
    /execute store result score @s parkour_time run time query gametime

    # Announce completion
    /tellraw @a [{"text":"@s completed the parkour in ","color":"gold"},{"score":{"name":"@s","objective":"parkour_time"},"color":"yellow"},{"text":" ticks!","color":"gold"}]

    # Reward
    /give @s minecraft:diamond 5
    /advancement grant @s only namespace:parkour_master

    asat @p:
        # Celebration effects
        /summon minecraft:firework_rocket ~ ~1 ~ {FireworksItem:{id:"minecraft:firework_rocket",Count:1,tag:{Fireworks:{Flight:1,Explosions:[{Type:2,Colors:[I;16776960],FadeColors:[I;16777215]}]}}}}
        /effect give @s minecraft:levitation 2 5 true

def parkour_tick():
    """Check for players on checkpoints"""
    # /execute as @a at @s if block ~ ~-1 ~ minecraft:light_weighted_pressure_plate run function namespace:on_checkpoint
    as @a at @s if block ~ ~-1 ~ minecraft:light_weighted_pressure_plate:
        # Detect players on pressure plates near checkpoints
        on_checkpoint()

    # /execute as @a at @s if entity @s[y=0,dy=50] run function namespace:reset_player
    as @a at @s if entity @s[y=0,dy=50]:
        # Detect players who fell
        reset_player()

# Register events
stdlib.addEventListener(event.LOAD, start_parkour)
stdlib.addEventListener(event.TICK, parkour_tick)
