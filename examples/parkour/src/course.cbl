"""Course builder commands for operators.

Stand where a point should go and run the matching function. Each point is a
marker entity with a floating label, and its chunk is force-loaded so a
player who falls far from the last checkpoint can still be sent back to it.
"""


def set_start():
    /kill @e[tag=pk_start]
    align xyz positioned ~0.5 ~ ~0.5:
        /summon minecraft:marker ~ ~ ~ {Tags:["pk_course","pk_point","pk_start"]}
        # Players sent back here face the way the builder was facing.
        /tp @e[type=minecraft:marker,tag=pk_start] ~ ~ ~ ~ 0
        /summon minecraft:text_display ~ ~1.8 ~ {Tags:["pk_course","pk_start"],billboard:"center",text:{text:"Parkour Start",color:"green",bold:true}}
        /forceload add ~ ~
    /scoreboard players set @e[type=minecraft:marker,tag=pk_start] pk_index 0
    /tellraw @s {"text":"Start placed. Add checkpoints in course order next.","color":"green"}


def add_checkpoint():
    /scoreboard players add #checkpoints pk_data 1
    number = 0
    store result score number temp:
        /scoreboard players get #checkpoints pk_data
    align xyz positioned ~0.5 ~ ~0.5:
        /summon minecraft:marker ~ ~ ~ {Tags:["pk_course","pk_point","pk_checkpoint","pk_new"]}
        /tp @e[type=minecraft:marker,tag=pk_new] ~ ~ ~ ~ 0
        label_checkpoint(number)
        /forceload add ~ ~
    /scoreboard players operation @e[type=minecraft:marker,tag=pk_new] pk_index = #checkpoints pk_data
    /tag @e[tag=pk_new] remove pk_new
    /tellraw @s {"text":"Checkpoint {number} placed.","color":"aqua"}


def label_checkpoint(number):
    /summon minecraft:text_display ~ ~1.8 ~ {Tags:["pk_course"],billboard:"center",text:{text:"Checkpoint {number}",color:"aqua"}}


def set_finish():
    /kill @e[tag=pk_finish]
    align xyz positioned ~0.5 ~ ~0.5:
        /summon minecraft:marker ~ ~ ~ {Tags:["pk_course","pk_finish"]}
        /summon minecraft:text_display ~ ~1.8 ~ {Tags:["pk_course","pk_finish"],billboard:"center",text:{text:"Finish",color:"gold",bold:true}}
        /forceload add ~ ~
    /tellraw @s {"text":"Finish placed. The course is ready.","color":"gold"}


def remove_course():
    /kill @e[tag=pk_course]
    /scoreboard players set #checkpoints pk_data 0
    /tag @a remove pk_running
    /tellraw @s {"text":"Course removed. Force-loaded chunks stay; use /forceload remove if nothing else needs them.","color":"yellow"}


def reset_records():
    /scoreboard players reset * pk_best
    /scoreboard players reset #record pk_data
    /tellraw @s {"text":"Personal bests and the server record were reset.","color":"yellow"}
