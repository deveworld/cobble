# Cobble Examples

Each folder is a complete data pack for Minecraft Java Edition 26.3 that you can
build, drop into a world, and use as-is or adapt.

| Example | What it is | Players use | Operators run |
|---|---|---|---|
| [`lobby/`](lobby/) | Server essentials: first-join welcome and starter kit, spawn teleport with a cooldown, playtime/death/kill stats, a safe spawn zone, rotating tips, and a server menu dialog on the pause screen | `/trigger menu`, `/trigger spawn`, `/trigger stats` | `/function lobby:set_spawn` |
| [`parkour/`](parkour/) | Parkour course kit: build a course in-game, then players get a timer, ordered checkpoints, fall and hazard resets, personal bests, a server record, and a first-clear advancement with a reward | `/trigger pk_restart`, `/trigger pk_leave` | `/function parkour:set_start`, `add_checkpoint`, `set_finish` |
| [`magic_items/`](magic_items/) | Three craftable right-click items with cooldowns: a Blink Rod that dashes and stops at walls, a Healing Staff, and a Frost Wand ray | right-click | `/function magic_items:give_all` |
| [`hello_world.cbl`](hello_world.cbl) | The smallest pack: a load message | | |

## Building

Project examples use `cobble.toml`; build from the example folder:

```bash
cd examples/lobby
cobble build --validate          # writes dist/
cobble build --zip               # also writes lobby.zip for a world's datapacks/
```

`--validate` checks every generated command against the Minecraft 26.3 command
tree. Single files build directly:

```bash
cobble build examples/hello_world.cbl -o output/hello_world
```

## What each example shows

- **lobby**: trigger objectives for commands without operator permission,
  statistic objectives (`play_time`, `leave_game`, `deathCount`), first-join
  tags, cooldowns, a marker entity as a movable spawn point, `multi_action` and
  `notice` dialogs, `datapack.dialog_tag` for the pause screen, loot tables, and
  a self-rescheduling function.
- **parkour**: operator builder functions that place markers and text displays,
  per-player run state in scoreboards, reading entity scores and NBT into Cobble
  variables with `store result`, `as` loops over markers, block tags for
  hazards, advancements with rewards, and macro arguments (`label_checkpoint`).
- **magic_items**: right-click detection with
  `minecraft.used:minecraft.carrot_on_a_stick`, `custom_data` item predicates,
  `item_model`/`item_name` components, compile-time unrolled ray casts,
  int-returning functions, per-item cooldowns, entity type tags, and recipes.

## Notes

- Module-level Cobble variables are reset whenever the pack loads (`/reload` or
  a server start). The examples keep anything that must persist, such as
  records or player stats, in their own scoreboard objectives.
- Function names are shared across a project's files, so each example uses
  distinct names (`use_spawn`, `try_blink`, ...).
- The release-gate fixtures that used to live here (`26_smoke`,
  `26_feature_matrix`, `resource_authoring`, `stdlib_v2`, `stdlib_v3`,
  `resource_pack`, `unrolling`, `inventory.cbl`) are in
  [`tests/fixtures/projects/`](../tests/fixtures/projects/), along with
  `showcase`, a feature tour whose in-game self-test runs in the server gate.

To run the optional real-server QA path from the repository root:

```bash
COBBLE_MINECRAFT_EULA_ACCEPTED=1 scripts/test_minecraft_server.sh
```
