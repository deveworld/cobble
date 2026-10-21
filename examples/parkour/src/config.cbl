"""Parkour settings. Change these and rebuild the pack."""

# Dropping this many blocks below your last checkpoint counts as a fall.
const FALL_DEPTH = 6
# How close (in blocks) a player must get to the start, a checkpoint, or the
# finish for it to count.
const TOUCH_RADIUS = 1.5

# Every course point (start and checkpoints) a player can be sent back to.
@Points = @e[type=minecraft:marker,tag=pk_point]
@Start = @e[type=minecraft:marker,tag=pk_start,limit=1]
