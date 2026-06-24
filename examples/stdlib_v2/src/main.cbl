from stdlib import datapack, event, score, storage, text

datapack.function_tag("utility", ["cobble_stdlib_v2:announce"])
datapack.predicate("checks/always", {
    "condition": "minecraft:random_chance",
    "chance": 1
})

def load():
    score.set("points", 10)
    storage.set("state", {"ready": True})
    text.tellraw("@a", "Stdlib v2 modules ready")
    announce()

def announce():
    /say stdlib v2 utility tag

stdlib.addEventListener(event.LOAD, load)
