# Hello World Example
# A simple example showing basic Cobble syntax

import stdlib
from stdlib import event

def hello():
    """Say hello to all players"""
    /say Hello from Cobble!
    /tellraw @a {"text":"Welcome to Cobble data packs!", "color":"green"}

def init():
    """Initialize the data pack"""
    /tellraw @a {"text":"Hello World pack loaded!", "color":"gold"}

# Register event handlers
stdlib.addEventListener(event.LOAD, init)
