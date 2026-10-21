@Admins = @a[tag=admin]
@NearbyMarkers = @e[type=armor_stand,tag=matrix_marker,distance=..16]

def setup_scoreboards():
    score.objective.add("matrix", "dummy", "Matrix")
    score.objective.add("matrix_energy", "dummy", "Energy")
    score.objective.display("sidebar", "matrix")
    team.add("matrix", "Matrix")
    team.modify("matrix", "color", "aqua")
    bossbar.add("matrix", {"text":"Feature Matrix"})
    bossbar.set_max("matrix", 100)
    bossbar.set_value("matrix", 25)
    bossbar.set_color("matrix", "blue")
    bossbar.set_players("matrix", "@a")

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
