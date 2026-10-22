# Events

Register a function to run when the pack loads or on every game tick:

```python
from stdlib import event

def on_load():
    /scoreboard objectives add kills dummy
    /tellraw @a {"text":"Data pack loaded!", "color":"green"}

def on_tick():
    as @a at @s:
        /particle minecraft:happy_villager ~ ~2 ~ 0.5 0.5 0.5 0 1

stdlib.addEventListener(event.LOAD, on_load)
stdlib.addEventListener(event.TICK, on_tick)
```

- `event.LOAD` runs when the pack loads: on `/reload` and on server start.
- `event.TICK` runs every game tick, 20 times per second.

Cobble adds the functions to the `minecraft:load` and `minecraft:tick` function
tags.
