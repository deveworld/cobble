# Standard Library Modules

The standard library is a set of helpers that the compiler turns into
commands and resource files. They are grouped into modules; import a module
before calling its helpers:

```python
from stdlib import event, text, score
```

| Import | Effect |
| --- | --- |
| `from stdlib import text` | Activates the `text` module. |
| `from stdlib import text, score` | Activates `text` and `score`. |
| `from stdlib import score.objective` | Activates `score.objective`, which `from stdlib import score` leaves inactive. |
| `import stdlib` | Activates every module, as in 0.7.x. |

The modules are `text`, `score`, `score.objective`, `random`, `timer`,
`storage`, `item_component`, `schedule`, `selector`, `position`, `bossbar`,
`team`, `entity`, `math`, `event`, `datapack`, and the experimental
`resource_pack`. This page covers the command helpers. `event` is described in
[Events](../language/events.md), `datapack` and `item_component` in
[Data Pack Resources](datapack-resources.md), and `resource_pack` in
[Resource Packs](resource-packs.md).

Calling a helper from a module that isn't imported fails with the
`missing-stdlib-module` diagnostic:

```text
Error: missing-stdlib-module: module 'text' not imported.
Add `from stdlib import text` or use `import stdlib` to enable all modules.
See https://deveworld.github.io/cobble/docs/stdlib/modules.html for the import and versioning model.
```

The `[stdlib] version` setting in `cobble.toml` controls this check.
`version = 2` is the default and requires the imports above. `version = 1`
keeps every module active as in 0.7.x and prints a deprecation warning once per
build. Helper behavior and generated output are the same under both versions.

```toml
[stdlib]
version = 2
```

## Text

```python
from stdlib import text

def notify():
    text.tellraw("@a", {"text": "Loaded", "color": "green"})
    text.tellraw("@a", text.plain("Plain message"))
    text.tellraw("@a", text.colored("Gold message", "gold"))
    text.tellraw("@a", text.score("@s", "points"))
    text.tellraw("@a", text.selector("@p"))
    text.title("@a", "Ready")
    text.subtitle("@a", {"text": "Go", "bold": True})
    text.actionbar("@a", "Running")
```

`text.plain`, `text.colored`, `text.score`, and `text.selector` return JSON
text components for use inside other helpers or JSON resource declarations.
Colors accept Minecraft named colors or `#RRGGBB`.

## Score

```python
from stdlib import score, score.objective

def update_score():
    score.set("points", 10)
    score.add("points", 5)
    score.remove("points", 2)
    score.copy("backup", "points")
    score.operation("points", "+=", "backup")
    score.reset("backup")
    score.objective.add("points", "dummy", "Points")
    score.objective.display("sidebar", "points")
```

Score helpers use the pack's variable objective, `<namespace>.vars`.

## Random

```python
from stdlib import random

def roll():
    random.int("roll", 1, 6)
    random.bool("coin")
```

Random helpers compile to Minecraft's `random value` command and store
the result in a scoreboard value. A single-value `random.int("score", n, n)`
range is lowered to a direct scoreboard set because Minecraft rejects
degenerate `random value n..n` ranges.

## Timer

```python
from stdlib import timer

def cooldown():
    timer.set("cooldown", 20)
    timer.tick("cooldown")
    timer.done("cooldown")   # writes cooldown_done as 0 or 1
    timer.reset("cooldown")
```

## Storage

```python
from stdlib import storage

def save_status():
    storage.set("status", {"ready": True, "count": 3})
    storage.merge("status", {"extra": "ok"})
    storage.copy("status_copy", "status")
    storage.set(storage.path("players", "current"), "alex")
    storage.append("events", "loaded")
    storage.get(storage.index("events", 0))
    storage.read_score("event_count", "events")
    storage.read_score("ready_flag", "status.ready", 10)
    storage.remove("status.extra")
```

Storage helpers write to `<namespace>:global`. `storage.read_score(score,
path)` stores what `data get` returns: a number (floored), or the size of a
list or compound, or the length of a string. With the optional third scale
argument, the value at `path` must be a number. `storage.path()`,
`storage.child()`, and `storage.index()` return storage path strings for use
inside storage helpers; they do not emit commands when used by themselves.

## Schedule, Bossbar, Team, Entity, Selector, And Position

```python
from stdlib import schedule, bossbar, team, entity, selector, position

def setup_ui():
    schedule.once("tick", "20t", "replace")
    schedule.clear("tick")
    bossbar.add("timer", "Timer")
    bossbar.set_max("timer", 100)
    bossbar.set_players("timer", "@a")
    team.add("runners", "Runners")
    team.modify("runners", "color", "green")
    entity.tag_add("@a", "runner")
    entity.effect_give("@a", "minecraft:speed", 10, 1, True)
    entity.teleport(selector.current(), position.relative(0, 1, 0))
```

`selector.*` and `position.*` return plain selector or coordinate strings for
use inside other helpers and emit no commands when used by themselves. These
helpers are thin wrappers over Minecraft commands, so use raw commands when a
specialized option is not covered yet.

## Math

```python
from stdlib import math

def calculate():
    root = math.sqrt(100)
    magnitude = math.abs(-50)
    low = math.min(10, 20)
    high = math.max(10, 20)
```

`math.sqrt` uses a generated scoreboard helper.
