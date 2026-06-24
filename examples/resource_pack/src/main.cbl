from stdlib import event, resource_pack

resource_pack.item_model("cobble_resource_pack:custom_sword", {
    "parent": "minecraft:item/handheld",
    "textures": {
        "layer0": "cobble_resource_pack:item/custom_sword"
    }
})

resource_pack.block_model("display_block", {
    "parent": "minecraft:block/cube_all",
    "textures": {
        "all": "cobble_resource_pack:block/display_block"
    }
})

resource_pack.lang("en_us", {
    "item.cobble_resource_pack.custom_sword": "Cobble Sword",
    "block.cobble_resource_pack.display_block": "Cobble Display Block"
})

def load():
    /tellraw @a {"text":"Resource-pack assets generated","color":"aqua"}

stdlib.addEventListener(event.LOAD, load)
