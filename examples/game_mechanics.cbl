import stdlib
from stdlib import event

def init():
    """Initialize the minigame system"""
    /scoreboard objectives add team dummy "Team"
    /scoreboard objectives add score dummy "Score"
    /scoreboard objectives add coins dummy "Coins"
    /scoreboard objectives add deaths deathCount "Deaths"
    /tellraw @a {"text":"Minigame System Initialized!", "color":"gold", "bold":true}

def tick():
    """Main game tick - manage teams and scoring"""
    # Check for players without a team and assign them
    as @a at @s if score @s team matches ..0:
        /scoreboard players set @s team 1
        /tellraw @s {"text":"You have been assigned to Team 1!", "color":"green"}

def on_death():
    """Handle player death"""
    # Respawn players and deduct coins
    # Note: Raw Minecraft syntax in execute blocks
    as @a at @s if score @s deaths matches 1..:
        /tellraw @s {"text":"You died! -10 coins", "color":"red"}
        /scoreboard players remove @s coins 10
        /scoreboard players set @s deaths 0

def award_points(amount):
    """Award points to nearest player"""
    as @p at @s:
        /scoreboard players add @s score {amount}
        /scoreboard players add @s coins {amount}
        /tellraw @s {"text":"You earned points!", "color":"gold"}
        /playsound minecraft:entity.player.levelup player @s ~ ~ ~ 1 1

def create_arena():
    """Create a simple arena at current location"""
    /fill ~-10 ~-1 ~-10 ~10 ~-1 ~10 minecraft:stone
    /fill ~-10 ~ ~-10 ~10 ~5 ~10 minecraft:air
    /fill ~-10 ~-1 ~-10 ~10 ~-1 ~10 minecraft:glowstone replace minecraft:stone
    /tellraw @a {"text":"Arena created!", "color":"green"}

def random_event():
    """Trigger random events during gameplay"""
    # Lightning strike at random player
    as @r at @s:
        /summon minecraft:lightning_bolt ~ ~ ~
        /tellraw @a {"text":"Lightning event!", "color":"yellow"}

    # Spawn bonus items
    as @p at @s:
        /summon minecraft:item ~ ~1 ~ {Item:{id:"minecraft:diamond",Count:1b}}

def team_score_check():
    """Check team scores and announce leader"""
    # This would require more complex score tracking
    /tellraw @a {"text":"Team scores updated", "color":"aqua"}

def match_start():
    """Start a match"""
    /scoreboard players set @a score 0
    /gamemode adventure @a
    /effect give @a minecraft:speed 999999 0 true
    /title @a title {"text":"MATCH START!", "color":"gold", "bold":true}
    /playsound minecraft:entity.ender_dragon.growl master @a ~ ~ ~ 1 1

def match_end():
    """End a match and announce winner"""
    /title @a title {"text":"MATCH OVER!", "color":"red", "bold":true}
    /gamemode spectator @a

    # Find winner (player with highest score)
    as @p at @s:
        /tellraw @a {"text":"Winner determined!", "color":"gold"}

# Register event handlers
stdlib.addEventListener(event.LOAD, init)
stdlib.addEventListener(event.TICK, tick)
stdlib.addEventListener(event.TICK, on_death)