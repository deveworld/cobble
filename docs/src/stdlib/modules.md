# Standard Library Modules

The standard library is a set of helpers that the compiler turns into
commands and resource files. They are grouped into modules; import a module
before calling its helpers:

```python
from stdlib import event, text, score
```

Import each module you use. `score.objective` needs its own import;
`import stdlib` enables every module. The default stdlib version is `2`.

The examples below cover command helpers. See [Events](../language/events.md),
[Data Pack Resources](datapack-resources.md), and
[Resource Packs](resource-packs.md) for declarations.

## Helper Arguments

Most helpers take literals, constants, or function parameters. To pass a
runtime variable to a command helper, call it through a function:

```python
from stdlib import entity

def mark(who, label):
    entity.tag_add(who, label)
```

Text helpers can also display runtime values with f-strings. JSON resource
contents must be known at build time. Boolean helper options, such as
`bossbar.set_visible`, require a literal or constant.

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

The text of `text.tellraw`, `text.title`, `text.subtitle`, and
`text.actionbar` uses f-strings as raw `tellraw` does (see
[JSON Text](../language/commands.md#json-and-snbt-text)):

```python
from stdlib import text

def countdown(seconds):
    text.actionbar("@a", {"text": f"Starts in {seconds}", "color": "yellow"})
```

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
    dice = random.randint(1, 6)
    coin = random.randint(0, 1)
    return dice + coin
```

`random.randint(minimum, maximum)` returns an integer with both bounds included.
Bounds can be integer variables or expressions; equal bounds return that value.
The older `random.int(name, minimum, maximum)` and `random.bool(name)` helpers
write directly to a scoreboard value.

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

Storage helpers use `<namespace>:global`. `storage.read_score` reads a
number or a container's size into a score. Its optional third argument
scales numeric values. `storage.path`, `storage.child`, and `storage.index`
build paths for use in other storage helpers.

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

Schedule helpers take a function name. A literal can name another namespace,
such as `"other:tick"`; parameter names resolve inside the current pack.
`selector.*` and `position.*` build strings for other helpers. Use raw
commands for options a helper does not expose.

## Math

```python
from stdlib import math

def calculate():
    root = math.sqrt(100)
    magnitude = math.abs(-50)
    low = math.min(10, 20)
    high = math.max(10, 20)
```
