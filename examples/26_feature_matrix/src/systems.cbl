@Admins = @a[tag=admin]
@NearbyMarkers = @e[type=armor_stand,tag=matrix_marker,distance=..16]

def setup_scoreboards():
    /scoreboard objectives add matrix dummy "Matrix"
    /scoreboard objectives add matrix_energy dummy "Energy"
    /team add matrix
    /team modify matrix color aqua
    /bossbar add cobble:matrix {"text":"Feature Matrix"}
    /bossbar set cobble:matrix max 100
    /bossbar set cobble:matrix value 25

def announce(message):
    /tellraw @a {"text":"{message}","color":"light_purple"}

def give_kit(player, item, count):
    /give {player} minecraft:{item} {count}
    /title {player} actionbar Kit count: {count}

def marker_scan():
    as @NearbyMarkers:
        /data merge entity @s {Glowing:1b}
    as @Admins:
        /tellraw @s {"text":"Marker scan complete","color":"green"}
