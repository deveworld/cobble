# Your First Pack

## Create A Project

```bash
cobble init --name my_datapack
cd my_datapack
```

This writes `cobble.toml`, `src/main.cbl`, and a `.gitignore`.
`cobble init --list-templates` shows the other starter templates.

## Write Some Code

Replace `src/main.cbl` with:

```python
from stdlib import event

def on_load():
    /tellraw @a {"text":"Hello from Cobble!", "color":"green"}

def reward(player, amount):
    /give {player} minecraft:diamond {amount}

stdlib.addEventListener(event.LOAD, on_load)
```

`on_load` runs whenever the pack loads. `reward` takes two parameters, so it
becomes a macro function.

## Build

```bash
cobble build --validate
```

The pack is written to `output/`. `--validate` checks every generated command
against the Minecraft 26.3 command tree; leave it out for a quick build.
`cobble check` reports source errors without building.

## Load It In Minecraft

Copy or build the pack into a world's `datapacks/` folder:

```bash
cobble build -o ~/.minecraft/saves/MyWorld/datapacks/my_datapack
```

`~/.minecraft` is the Linux location. On Windows it is `%APPDATA%\.minecraft`
(`$env:APPDATA\.minecraft` in PowerShell), and on macOS
`~/Library/Application\ Support/minecraft`.

Then, in the world:

```text
/reload
/datapack list
/function my_datapack:reward {player:"@s", amount:3}
```

`/datapack list` shows the pack as enabled. To turn it off or on, use
`/datapack disable "file/my_datapack"` and `/datapack enable "file/my_datapack"`.

## Rebuild On Save

```bash
cobble link --world ~/.minecraft/saves/MyWorld
cobble watch --link
```

`cobble link` saves the world's `datapacks/` folder for this project, and
`cobble watch --link` rebuilds into it whenever a source file changes. Run
`/reload` in game to pick up each build.

## If Something Goes Wrong

- **The pack is missing from `/datapack list`.** Check that the folder is
  directly under `datapacks/` and contains `pack.mcmeta`, and that the world
  runs Minecraft 26.3.
- **A function is not found.** Run `/reload` after building, and call it with
  the project namespace, such as `my_datapack:on_load`.
- **Changes do not appear.** Check that the build went to the world you have
  open, then `/reload`.
- **Variables look wrong.** Cobble keeps numeric variables on the
  `<namespace>.vars` objective:
  `/scoreboard players get <name> my_datapack.vars`.
