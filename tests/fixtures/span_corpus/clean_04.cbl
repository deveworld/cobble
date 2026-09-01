from stdlib import datapack

@Players = @a[type=player]

define @Marker = @e[type=marker]
create {"Tags": ["cobble"]}
end

datapack.function_tag("minecraft:load", ["clean_04:setup"])
datapack.predicate("always", {"condition": "minecraft:random_chance", "chance": 1})

def setup():
    /say setup
