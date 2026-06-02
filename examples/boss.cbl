# Boss Fight System Example
import stdlib
from stdlib import event

# Global variables
boss_health = 100
phase = 1

def spawn_boss():
    """Spawn the boss entity"""
    asat @p:
        /summon minecraft:wither_skeleton ~ ~1 ~ {CustomName:'{"text":"Dark Lord","color":"dark_red","bold":true}', Health:200f, attributes:[{id:"minecraft:max_health",base:200.0d}], HandItems:[{id:"minecraft:netherite_sword",Count:1b},{}], ArmorItems:[{},{},{},{id:"minecraft:dragon_head",Count:1b}]}
    /bossbar add boss {"text":"Dark Lord"}
    /bossbar set boss players @a
    /bossbar set boss max 200
    /bossbar set boss value 200
    /bossbar set boss color red

def boss_tick():
    """Boss fight logic - runs every tick"""
    global phase
    # Update boss bar with current health
    /execute store result bossbar boss value run data get entity @e[type=wither_skeleton,name="Dark Lord",limit=1] Health

    # Check for phase transition
    if boss_health <= 50:
        if phase == 1:
            phase = 2
            enter_phase_2()

def enter_phase_2():
    """Boss enters rage mode"""
    /tellraw @a {"text":"The Dark Lord enters rage mode!","color":"red","bold":true}
    /effect give @e[type=wither_skeleton,name="Dark Lord"] minecraft:strength 999999 1
    /effect give @e[type=wither_skeleton,name="Dark Lord"] minecraft:speed 999999 0

    # Spawn minions
    for i in range(3):
        asat @p:
            /summon minecraft:zombie ~ ~ ~ {IsBaby:0b,ArmorItems:[{},{},{},{id:"minecraft:iron_helmet",Count:1b}]}

def boss_defeated():
    """Called when boss is defeated"""
    /bossbar remove boss
    /tellraw @a {"text":"Victory! The Dark Lord has been defeated!","color":"gold","bold":true}
    /advancement grant @a only namespace:defeat_boss

    # Victory fireworks
    for i in range(5):
        asat @p:
            /summon minecraft:firework_rocket ~ ~1 ~ {FireworksItem:{id:"minecraft:firework_rocket",Count:1,tag:{Fireworks:{Flight:2,Explosions:[{Type:1,Colors:[I;16711680,16776960],FadeColors:[I;16777215]}]}}}}

# Register tick event for boss AI
stdlib.addEventListener(event.TICK, boss_tick)
