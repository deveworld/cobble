# Example Packs

The repository's
[`examples/`](https://github.com/deveworld/cobble/tree/main/examples) folder
has complete packs you can build and use as-is or adapt:

| Example | What it is |
| --- | --- |
| [`lobby`](https://github.com/deveworld/cobble/tree/main/examples/lobby) | Server essentials: a first-join welcome and starter kit, a spawn teleport with a cooldown, playtime, death, and kill stats, a safe spawn zone, rotating tips, and a server menu dialog on the pause screen. |
| [`parkour`](https://github.com/deveworld/cobble/tree/main/examples/parkour) | A parkour course kit: build a course in game, then players get a timer, ordered checkpoints, fall resets, personal bests, a server record, and a first-clear advancement. |
| [`magic_items`](https://github.com/deveworld/cobble/tree/main/examples/magic_items) | Three craftable right-click items with cooldowns: a Blink Rod, a Healing Staff, and a Frost Wand. |
| [`hello_world.cbl`](https://github.com/deveworld/cobble/blob/main/examples/hello_world.cbl) | The smallest pack: a load message. |

Build a project example from its folder:

```bash
cd examples/lobby
cobble build --validate
```

The [examples README](https://github.com/deveworld/cobble/blob/main/examples/README.md)
lists the commands each pack adds and the Cobble features it uses.
