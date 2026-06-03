# Function Examples
# Shows function parameters and macro system

def greet(player_name):
    """Greet a player by name using macro parameters"""
    /tellraw {player_name} {"text":"Hello!", "color":"green"}
    /title {player_name} title {"text":"Welcome!", "color":"gold"}

def give_items(player, item, count):
    """Give items to a player"""
    /give {player} minecraft:{item} {count}
    /tellraw {player} {"text":"You received items!", "color":"aqua"}

def teleport_player(player, x, y, z):
    """Teleport a player to coordinates"""
    /tp {player} {x} {y} {z}
    /tellraw {player} {"text":"Teleported!", "color":"yellow"}

def call_functions():
    """Example of calling functions"""
    # Note: To call these functions with arguments,
    # you would need to pass them from another context
    greet(@a)
    give_items(@a, "diamond", 5)
