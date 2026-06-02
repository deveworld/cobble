import stdlib
from stdlib import event

# Global variables
score = 0

def reset():
    """Called when the data pack is loaded/reloaded"""
    global score
    /tellraw @a {"text":"Data pack loaded!", "color":"green"}
    score = 0
    main()

def on_tick():
    """Called every game tick (20 times per second)"""
    # /execute as @a at @s run particle minecraft:happy_villager ~ ~2 ~ 0.5 0.5 0.5 0 1
    as @a at @s:
        /particle minecraft:happy_villager ~ ~2 ~ 0.5 0.5 0.5 0 1

def greet(player_name):
    """Greet a player"""
    global score

    /say Hello, {player_name}!
    /tellraw @a {"text":"Welcome to the server!, {player_name}", "color":"gold"}
    score = score + 1
    check_score()

def check_score():
    """Check if score threshold is reached"""
    global score

    if score >= 10:
        /say Achievement unlocked!
        /advancement grant @a only example:first_milestone
        score = 0

# Register event handlers
stdlib.addEventListener(event.LOAD, reset)
stdlib.addEventListener(event.TICK, on_tick)

# Main game logic
def main():
    """Main entry point"""
    greet("Player")

    # Direct Minecraft commands still work
    /gamemode creative @a[gamemode=survival]

    # Python-style control structures
    for i in range(5):
        as @a at @s:
            /summon minecraft:firework_rocket ~ ~1 ~ {LifeTime:20}
