# Inventory Management Example

# Shop system
def create_shop():
    """Create a villager shop"""
    asat @s:
        /summon minecraft:villager ~ ~ ~ {VillagerData:{profession:"minecraft:librarian",level:5,type:"minecraft:plains"},CustomName:'{"text":"Shop Keeper","color":"gold"}',PersistenceRequired:1b,NoAI:1b,Invulnerable:1b}

def give_starter_kit(player):
    """Give starter items to a player"""
    # Clear inventory first
    /clear {player}

    # Armor
    /item replace entity {player} armor.head with minecraft:iron_helmet
    /item replace entity {player} armor.chest with minecraft:iron_chestplate
    /item replace entity {player} armor.legs with minecraft:iron_leggings
    /item replace entity {player} armor.feet with minecraft:iron_boots

    # Tools
    /give {player} minecraft:iron_sword[minecraft:custom_name='{"text":"Starter Sword","color":"gray"}']
    /give {player} minecraft:iron_pickaxe[minecraft:enchantments={levels:{"minecraft:efficiency":2}}]
    /give {player} minecraft:iron_axe
    /give {player} minecraft:iron_shovel

    # Food and supplies
    /give {player} minecraft:bread 32
    /give {player} minecraft:torch 64
    /give {player} minecraft:cobblestone 64

    /tellraw {player} {"text":"Starter kit received!","color":"green"}

def create_custom_item():
    """Create a custom enchanted item"""
    asat @s:
        /give @s minecraft:diamond_sword[minecraft:custom_name='{"text":"Excalibur","color":"gold","bold":true,"italic":false}',minecraft:lore=['{"text":"A legendary blade","color":"gray","italic":true}','{"text":"Forged by ancient smiths","color":"gray","italic":true}'],minecraft:enchantments={levels:{"minecraft:sharpness":5,"minecraft:unbreaking":3,"minecraft:mending":1,"minecraft:looting":3}},minecraft:unbreakable={}]

def create_potion_set():
    """Create a set of useful potions"""
    asat @s:
        /give @s minecraft:potion[minecraft:potion_contents={potion:"minecraft:strong_healing"}] 3
        /give @s minecraft:potion[minecraft:potion_contents={potion:"minecraft:strong_strength"}] 2
        /give @s minecraft:potion[minecraft:potion_contents={potion:"minecraft:swiftness"}] 2
        /give @s minecraft:potion[minecraft:potion_contents={potion:"minecraft:fire_resistance"}] 1
        /give @s minecraft:splash_potion[minecraft:potion_contents={potion:"minecraft:strong_harming"}] 3

def check_inventory(player):
    """Check if player has specific items"""
    # Check for diamond
    /execute if entity {player}[nbt={Inventory:[{id:"minecraft:diamond"}]}] run tellraw {player} {"text":"You have diamonds!","color":"aqua"}

    # Check for empty slots
    /execute store result score {player} empty_slots run data get entity {player} Inventory
    /tellraw {player} [{"text":"Inventory slots used: ","color":"gray"},{"score":{"name":"{player}","objective":"empty_slots"},"color":"white"}]

def organize_chest():
    """Create an organized storage chest"""
    asat @s:
        /setblock ~ ~ ~ minecraft:chest{CustomName:'{"text":"Storage","color":"dark_green"}'}
        
        # Fill with categorized items
        /item replace block ~ ~ ~ container.0 with minecraft:iron_ingot 64
        /item replace block ~ ~ ~ container.1 with minecraft:gold_ingot 32
        /item replace block ~ ~ ~ container.2 with minecraft:diamond 16
        /item replace block ~ ~ ~ container.3 with minecraft:emerald 8
        
        # Tools section
        /item replace block ~ ~ ~ container.9 with minecraft:iron_pickaxe
        /item replace block ~ ~ ~ container.10 with minecraft:iron_axe
        /item replace block ~ ~ ~ container.11 with minecraft:iron_shovel
        /item replace block ~ ~ ~ container.12 with minecraft:iron_hoe
        
        # Food section
        /item replace block ~ ~ ~ container.18 with minecraft:bread 64
        /item replace block ~ ~ ~ container.19 with minecraft:cooked_beef 64
        /item replace block ~ ~ ~ container.20 with minecraft:golden_apple 3
