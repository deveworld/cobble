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
}
DATA_DRIVEN = {
    "enchantment": "data/minecraft/enchantment/",
    "damage_type": "data/minecraft/damage_type/",
    "biome": "data/minecraft/worldgen/biome/",
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
