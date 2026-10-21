"""Arena layout: selector aliases, entity templates, and spawning."""

const ARENA_Y = 100

@ArenaMarkers = @e[type=minecraft:marker,tag=arena_marker]
@ArenaEntities = @e[tag=arena_entity]

# A reusable summon shape: `create @Beacon` summons a tagged marker here.
define @Beacon = @e[type=minecraft:marker,tag=arena_marker]
create {
    "Tags": ["arena_marker", "arena_entity"],
    "data": {"wave": 0, "label": "beacon"}
}
end

define @Guard = @e[type=minecraft:armor_stand,tag=arena_guard]
create {
    "Tags": ["arena_guard", "arena_entity"],
    "CustomName": {"text": "Arena Guard", "color": "gold"},
    "CustomNameVisible": True,
    "NoGravity": True,
    "ShowArms": True
}
end


def clear_arena():
    """Remove everything a previous run spawned."""
    /kill @ArenaEntities
    /kill @e[type=minecraft:marker,tag=arena_probe]


def build_arena():
    """Lay a floor and spawn three beacons and one guard."""
    clear_arena()
    /fill -3 99 -3 3 99 3 minecraft:stone
    for x in [-1, 0, 1]:
        positioned {x} 100 0:
            create @Beacon
    positioned 0 101 2:
        create @Guard
