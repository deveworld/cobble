
import stdlib
from stdlib import event

@Players = @a[type=player]

define @Marker = @e[type=marker]
create {"Tags": ["cobble"]}
end

create @Marker

datapack.function_tag("minecraft:load", ["cobble:setup"])
datapack.predicate("always", {"condition": "minecraft:random_chance", "chance": 1})

def setup():
    /say setup
