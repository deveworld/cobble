# Migrating From Cobble 0.10 To 0.11

Status: 0.11.0 migration notes (unreleased).

Cobble 0.11 keeps the Minecraft Java Edition 26.3 target and pack format
`121.0`, so `cobble.toml` and the command tree stay as they are. The changes are
in generated output: each pack keeps its variables on its own objective, and
module variables can opt into keeping their values across loads.

## Recommended Flow

1. Review the project with the migration report (0.10 to 0.11 is the default
   route):

```bash
cobble migrate --json
```

   The report lists every raw command and execute header that names the
   `temp` objective (`temp-objective-reference`). These lines keep working;
   the list shows where your pack reads or writes Cobble variables, which helps
   with step 3.

2. Rebuild and re-run project checks:

```bash
cobble check
cobble build --validate
```

3. Update anything outside the pack that reads Cobble variables by objective
   name, such as command blocks, other data packs, plugins, or commands typed
   in chat. `/scoreboard players get round temp` becomes
   `/scoreboard players get round <namespace>.vars`.

## Per-Pack Variable Objective

0.10 kept every pack's numeric variables and internal helper scores on one
shared `temp` objective, so two Cobble packs on one server that used the same
variable name overwrote each other. The `math` and `__internal__` objectives
were shared the same way.

In 0.11 these scores live on `<namespace>.vars`, for example `parkour.vars`
for the namespace `parkour`. The load function creates it with
`scoreboard objectives add <namespace>.vars dummy`. Strings, lists, and maps
already lived in the per-namespace storage `<namespace>:global` and are
unchanged.

`temp` stays the name Cobble source uses for this objective. The compiler
rewrites it to `<namespace>.vars` in `scoreboard` arguments, `if score`,
`unless score`, and `store result score`, `scores={...}` selector filters, and
score text components. Other text, such as a chat message containing the word
`temp`, is left alone. See "The `temp` Objective In Raw Commands" in
[`language.md`](language.md).

Values already stored under `temp` in an existing world are not carried over.
Module variables reset on every load in 0.10 as well, so the new objective
starts from the same values. Once no other pack uses it, remove the old
objective with `/scoreboard objectives remove temp`; do the same for `math` and
`__internal__` if no other pack uses them.

## Persistent Module Variables

Module variables still reset on every load. Declare one with `persist` to keep
its value across `/reload` and server restarts:

```python
persist best_time = 0
persist champion = "nobody"
```

The initializer only runs while the score or storage value is unset. `persist`
is only allowed at module level and takes a constant initializer. Packs that
kept records in a hand-written objective to survive reloads can switch to
`persist` for pack-wide values; per-player records still need their own
objective.

`persist` is not a reserved word: a variable named `persist` keeps working.

## Tooling Changes

- `cobble migrate` defaults to `--from 0.10 --to 0.11`. Routes from 0.8 and 0.9
  to 0.11 are also supported; pass `--to 0.10` for the previous route.
- The serde AST adds `Assignment.persist`, omitted from JSON when `false`.
