#!/usr/bin/env python3
"""Regenerates src/validator/registries.json for a new Minecraft version.

    java -DbundlerMainClass=net.minecraft.data.Main -jar server.jar --reports
    scripts/update_registries.py generated/reports versions/<v>/server-<v>.jar

The file lists the names Minecraft looks up when it loads a function. Names
drop the `minecraft:` namespace. Registries that data packs define, such as
enchantments, come from the `data` folder of the unbundled server jar.
"""
import json
import sys
import zipfile
from pathlib import Path

STATIC = {
    "item": "minecraft:item",
    "block": "minecraft:block",
    "entity_type": "minecraft:entity_type",
    "mob_effect": "minecraft:mob_effect",
    "particle_type": "minecraft:particle_type",
    "attribute": "minecraft:attribute",
    "custom_stat": "minecraft:custom_stat",
    "data_component": "minecraft:data_component_type",
    "data_component_predicate": "minecraft:data_component_predicate_type",
    "poi_type": "minecraft:point_of_interest_type",
}
DATA_DRIVEN = {
    "enchantment": "data/minecraft/enchantment/",
    "damage_type": "data/minecraft/damage_type/",
    "biome": "data/minecraft/worldgen/biome/",
    "block_tag": "data/minecraft/tags/block/",
    "item_tag": "data/minecraft/tags/item/",
    "biome_tag": "data/minecraft/tags/worldgen/biome/",
    "poi_tag": "data/minecraft/tags/point_of_interest_type/",
    "loot_table": "data/minecraft/loot_table/",
    "predicate": "data/minecraft/predicate/",
    "item_modifier": "data/minecraft/item_modifier/",
    "dialog": "data/minecraft/dialog/",
    "dialog_tag": "data/minecraft/tags/dialog/",
    "entity_type_tag": "data/minecraft/tags/entity_type/",
    "enchantment_tag": "data/minecraft/tags/enchantment/",
    "advancement": "data/minecraft/advancement/",
    "recipe": "data/minecraft/recipe/",
    "feature": "data/minecraft/worldgen/feature/",
    "context_int_provider": "data/minecraft/context_int_provider/",
    "context_float_provider": "data/minecraft/context_float_provider/",
    "slot_source": "data/minecraft/slot_source/",
    "world_clock": "data/minecraft/world_clock/",
    "timeline": "data/minecraft/timeline/",
}


def main(reports: Path, server_jar: Path) -> None:
    registries = json.loads((reports / "registries.json").read_text())
    output = {
        name: sorted(key.removeprefix("minecraft:") for key in registries[source]["entries"])
        for name, source in STATIC.items()
    }
    with zipfile.ZipFile(server_jar) as jar:
        names = jar.namelist()
    for name, folder in DATA_DRIVEN.items():
        output[name] = sorted(
            path[len(folder) : -len(".json")]
            for path in names
            if path.startswith(folder) and path.endswith(".json")
        )
    blocks = json.loads((reports / "blocks.json").read_text())
    output["block_properties"] = {
        name.removeprefix("minecraft:"): block["properties"]
        for name, block in sorted(blocks.items())
        if "properties" in block
    }
    target = Path(__file__).parent.parent / "src/validator/registries.json"
    target.write_text(json.dumps(output, separators=(",", ":"), sort_keys=True) + "\n")


if __name__ == "__main__":
    main(Path(sys.argv[1]), Path(sys.argv[2]))
