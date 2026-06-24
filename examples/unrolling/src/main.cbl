from stdlib import datapack

datapack.function_tag("minecraft:load", ["cobble_unrolling:load"])

def load():
    for i in range(3):
        /say range index {i}
    for label in ["north", "south"]:
        /say array value {label}
    for n in range(1, 6, 2):
        /say stepped value {n}
