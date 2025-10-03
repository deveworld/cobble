# Counter Example
# Shows module-level variables and tick functions

import stdlib
from stdlib import event

# Module-level variables
counter = 0

def tick():
    """Called every game tick"""
    global counter
    counter = counter + 1

    # Every second (20 ticks)
    if counter >= 20:
        counter = 0
        /tellraw @a {"text":"One second passed!", "color":"aqua"}

def init():
    """Initialize the counter"""
    /tellraw @a {"text":"Counter pack loaded!", "color":"green"}

# Register event handlers
stdlib.addEventListener(event.LOAD, init)
stdlib.addEventListener(event.TICK, tick)
