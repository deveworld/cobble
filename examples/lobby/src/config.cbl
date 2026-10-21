"""Lobby settings. Change these and rebuild the pack."""

# Seconds a player waits between two `/trigger spawn` uses.
const SPAWN_COOLDOWN_SECONDS = 30
# Players this close to the spawn point heal and take no damage.
const SAFE_RADIUS = 24
# Minutes between two server tips in chat.
const TIP_INTERVAL_MINUTES = 5

const SPAWN_COOLDOWN_TICKS = SPAWN_COOLDOWN_SECONDS * 20
const TIP_INTERVAL_TICKS = TIP_INTERVAL_MINUTES * 1200

# The spawn point is a marker entity placed by `/function lobby:set_spawn`.
@Spawn = @e[type=minecraft:marker,tag=lobby_spawn,limit=1]
