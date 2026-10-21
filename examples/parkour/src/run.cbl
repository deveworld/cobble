"""What happens to a player during a run. Every function runs as the player."""

from stdlib import text
from config import FALL_DEPTH, TOUCH_RADIUS

# Filled in by split_run_time() for the messages below.
time_seconds = 0
time_tenths = 0


def split_run_time():
    """Turns the player's run clock (ticks) into seconds and tenths."""
    global time_seconds
    global time_tenths
    run_ticks = 0
    store result score run_ticks temp:
        /scoreboard players get @s pk_time
    time_seconds = run_ticks / 20
    time_tenths = (run_ticks % 20) / 2


def start_run():
    /tag @s add pk_running
    /scoreboard players set @s pk_time 0
    /scoreboard players set @s pk_cp 0
    /scoreboard players set @s pk_falls 0
    /scoreboard players enable @s pk_restart
    /scoreboard players enable @s pk_leave
    text.subtitle("@s", text.colored("Touch every checkpoint, then the finish", "gray"))
    text.title("@s", text.colored("Parkour", "green"))
    /playsound minecraft:block.note_block.pling master @s ~ ~ ~ 1 2


def run_tick():
    /scoreboard players add @s pk_time 1
    if entity @e[type=minecraft:marker,tag=pk_start,distance=..{TOUCH_RADIUS}]:
        # Standing on the start keeps the clock and progress at zero.
        /scoreboard players set @s pk_time 0
        /scoreboard players set @s pk_cp 0
    check_checkpoint()
    check_fall()
    show_hud()
    if entity @e[type=minecraft:marker,tag=pk_finish,distance=..{TOUCH_RADIUS}]:
        try_finish()


def check_checkpoint():
    """Counts the next checkpoint once the player touches it."""
    wanted = 0
    store result score wanted temp:
        /scoreboard players get @s pk_cp
    wanted += 1
    touched = 0
    as @e[type=minecraft:marker,tag=pk_checkpoint,distance=..{TOUCH_RADIUS}]:
        index = 0
        store result score index temp:
            /scoreboard players get @s pk_index
        if index == wanted:
            touched = 1
    if touched == 1:
        /scoreboard players add @s pk_cp 1
        split_run_time()
        total = 0
        store result score total temp:
            /scoreboard players get #checkpoints pk_data
        /tellraw @s ["",{"text":"Checkpoint {wanted}/{total}","color":"aqua"},{"text":" at {time_seconds}.{time_tenths}s","color":"gray"}]
        /playsound minecraft:entity.experience_orb.pickup master @s
        /particle minecraft:happy_villager ~ ~1 ~ 0.4 0.6 0.4 0 15


def check_fall():
    """Sends the player back to their last point after a fall or a hazard."""
    feet_y = 0
    store result score feet_y temp:
        /data get entity @s Pos[1]
    in_hazard = 0
    if block ~ ~ ~ #parkour:hazards:
        in_hazard = 1
    last_point = 0
    store result score last_point temp:
        /scoreboard players get @s pk_cp
    sent_back = 0
    /tag @s add pk_self
    as @Points:
        point_index = 0
        store result score point_index temp:
            /scoreboard players get @s pk_index
        if point_index == last_point:
            point_y = 0
            store result score point_y temp:
                /data get entity @s Pos[1]
            if in_hazard == 1 or feet_y < point_y - FALL_DEPTH:
                /tp @a[tag=pk_self,limit=1] @s
                sent_back = 1
    /tag @s remove pk_self
    if sent_back == 1:
        /scoreboard players add @s pk_falls 1
        /playsound minecraft:entity.enderman.teleport master @s


def show_hud():
    split_run_time()
    hud_progress = 0
    hud_total = 0
    store result score hud_progress temp:
        /scoreboard players get @s pk_cp
    store result score hud_total temp:
        /scoreboard players get #checkpoints pk_data
    /title @s actionbar ["",{"text":"Time {time_seconds}.{time_tenths}s","color":"yellow"},{"text":"   Checkpoint {hud_progress}/{hud_total}","color":"aqua"}]


def try_finish():
    progress = 0
    total = 0
    store result score progress temp:
        /scoreboard players get @s pk_cp
    store result score total temp:
        /scoreboard players get #checkpoints pk_data
    if progress < total:
        # Sent after the HUD, so this message wins the action bar.
        text.actionbar("@s", text.colored("You missed a checkpoint!", "red"))
    else:
        finish_run()


def finish_run():
    /tag @s remove pk_running
    /scoreboard players add @s pk_clears 1
    split_run_time()
    run_ticks = 0
    best_ticks = 0
    record_ticks = 0
    store result score run_ticks temp:
        /scoreboard players get @s pk_time
    store result score best_ticks temp:
        /scoreboard players get @s pk_best
    store result score record_ticks temp:
        /scoreboard players get #record pk_data
    /tellraw @a ["",{"selector":"@s","color":"gold"},{"text":" finished the parkour in {time_seconds}.{time_tenths}s","color":"yellow"}]
    if best_ticks == 0 or run_ticks < best_ticks:
        /scoreboard players operation @s pk_best = @s pk_time
        text.tellraw("@s", text.colored("New personal best!", "green"))
    if record_ticks == 0 or run_ticks < record_ticks:
        /scoreboard players operation #record pk_data = @s pk_time
        /tellraw @a ["",{"text":"New server record by ","color":"light_purple"},{"selector":"@s","color":"gold"},{"text":"!","color":"light_purple"}]
    # Only the first grant pays out the advancement's reward.
    /advancement grant @s only parkour:first_clear
    /playsound minecraft:ui.toast.challenge_complete master @s
    /particle minecraft:totem_of_undying ~ ~1 ~ 0.5 1 0.5 0.5 60


def use_restart():
    /scoreboard players reset @s pk_restart
    /scoreboard players enable @s pk_restart
    /tag @s remove pk_running
    if entity @Start:
        /tp @s @Start
    unless entity @Start:
        text.tellraw("@s", text.colored("This server has no parkour course yet.", "red"))


def use_leave():
    /scoreboard players reset @s pk_leave
    /scoreboard players enable @s pk_leave
    /tag @s remove pk_running
    text.actionbar("@s", text.colored("You left the parkour", "gray"))
