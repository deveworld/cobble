@Builders = @a[gamemode=creative]
@Survivors = @a[gamemode=survival]

def announce(message):
    /tellraw @a {"text":"{message}","color":"aqua"}

def reward(player, amount):
    /give {player} minecraft:diamond {amount}
    /say rewarded {player} with {amount} diamond

def survivor_ping():
    as @Survivors:
        /say survivor online
