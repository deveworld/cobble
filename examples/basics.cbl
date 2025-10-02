# Basic Cobble syntax examples

# Global Variables
score = 0
player_count = 5
message = "Hello World"

# Simple function
def hello():
    """A simple greeting function"""
    /say Hello World
    /tellraw @a {"text":"Welcome!", "color":"green"}

# Function with parameters
def greet_player(name):
    """Greet a specific player"""
    /tellraw @a {"text":"Welcome, {name}!", "color":"gold"}

# Control flow - if statement
def check_score():
    """Check if score threshold is reached"""
    if score >= 10:
        /say High score achieved!
        /advancement grant @p only namespace:high_score

# Control flow - for loop
def spawn_entities():
    """Spawn multiple entities"""
    for i in range(5):
        asat @p:
            /summon minecraft:pig ~ ~1 ~
            /effect give @e[type=pig,distance=..5] minecraft:glowing 30

# Control flow - while loop
def countdown():
    """Countdown example"""
    counter = 10
    while counter > 0:
        /tellraw @a Countdown: {counter}
        counter = counter - 1