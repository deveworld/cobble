"""In-game self-test suite.

Run `/function cobble_showcase:selftest` on a server. Every check prints
`PASS <label>` or `FAIL <label> ...`, and the suite ends with a summary line.
"""

import mathx
from mathx import clamp, sign
from arena import build_arena

const BASE = 7
const BONUS = 3
const HALF = 1 / 2
const SPEED = 0.25

passed = 0
failed = 0
hook_calls = 0

# Entity templates only feed `create`; target their entities with an alias.
@Guards = @e[type=minecraft:armor_stand,tag=arena_guard]


def check(label, actual, expected):
    """Report one result; the server smoke test greps PASS/FAIL lines."""
    global passed
    global failed
    if actual == expected:
        passed += 1
        /say PASS {label}
    else:
        failed += 1
        /say FAIL {label} expected {expected} got {actual}


def double(value):
    return value * 2


def classify(value):
    match value:
        case 0:
            return 0
        case 1 to 9:
            return 1
        case 10 to 99:
            return 2
        case _:
            return 3


def test_arithmetic():
    a = 10
    b = 5
    c = 3
    check("precedence", a + b * c, 25)
    check("parentheses", (a + b) * c, 45)
    check("sub_div", a - b / c, 9)
    check("mod_add", a % c + b, 6)
    check("chain", a + b + c - 1, 17)
    check("power_const", 3 ^ 3, 27)
    exp = 4
    check("power_runtime", c ^ exp, 81)
    check("power_zero", c ^ (exp - 4), 1)
    check("unary_minus", -a + 3, -7)
    n = -7
    d = 2
    check("floor_div_runtime", n / d, -4)
    check("floor_div_folded", -7 / 2, -4)
    check("floor_mod_runtime", n % 3, 2)
    check("floor_mod_folded", -7 % 3, 2)
    check("const_expr", BASE * BONUS + 1, 22)
    x = 4
    x = 10 - x
    check("self_reference", x, 6)
    acc = 10
    acc += 5
    acc -= 3
    acc *= 2
    acc /= 5
    acc %= 3
    check("compound_ops", acc, 1)
    base = 2
    base ^= exp
    check("compound_power", base, 16)
    check("call_in_expr", double(5) + 1, 11)
    doubled = double(double(c))
    check("nested_calls", doubled, 12)


def test_math():
    check("sqrt_const", math.sqrt(144), 12)
    v = 200
    root = math.sqrt(v)
    check("sqrt_runtime", root, 14)
    check("abs", math.abs(-50), 50)
    low = math.min(v, 30)
    high = math.max(v, 30)
    check("min", low, 30)
    check("max", high, 200)
    check("imported_clamp_high", clamp(150, 0, 100), 100)
    check("imported_clamp_low", clamp(-5, 0, 100), 0)
    check("imported_clamp_mid", clamp(42, 0, 100), 42)
    check("imported_sign", sign(-9), -1)
    check("module_call", mathx.sum_to(10), 55)


def test_conditions():
    a = 10
    b = 20
    hits = 0
    if a == 10:
        hits += 1
    if a != 11:
        hits += 1
    if a < b:
        hits += 1
    if a <= 10:
        hits += 1
    if b > a:
        hits += 1
    if b >= 20:
        hits += 1
    check("comparisons", hits, 6)

    hits = 0
    if a > 5 and b < 25:
        hits += 1
    if a == 1 or b == 20:
        hits += 1
    if not a == 3:
        hits += 1
    if not (a > 5 and b > 50):
        hits += 1
    if not (a > 5 or b > 5):
        hits += 100
    if a % 3 == 1 and b ^ 2 == 400:
        hits += 1
    if (a == 10 or b == 0) and not b == 3:
        hits += 1
    check("boolean_ops", hits, 6)

    score_value = 85
    grade = 0
    if score_value >= 90:
        grade = 4
    elif score_value >= 80:
        grade = 3
    elif score_value >= 70:
        grade = 2
    else:
        grade = 1
    check("elif_chain", grade, 3)

    nested = 0
    if a > 0:
        if b > 0:
            if a + b == 30:
                nested = 3
    check("nested_if", nested, 3)

    flag = True
    flag = not flag
    flag = not flag
    flag = not flag
    check("not_flag", flag, 0)

    calls = 0
    if mathx.is_even(b) and not mathx.is_even(a + 1):
        calls += 1
    if double(a) == b:
        calls += 1
    check("call_conditions", calls, 2)


def test_loops():
    total = 0
    for i in range(5):
        total += i
    check("range_unrolled", total, 10)

    total = 0
    for i in range(1, 6, 2):
        total += i
    check("range_stepped", total, 9)

    total = 0
    for i in range(10) by 2:
        total += i
    check("range_by", total, 20)

    order = 0
    for i in range(4) by -1:
        order = order * 10 + i
    check("range_reverse", order, 3210)

    limit = 6
    total = 0
    for i in range(limit):
        total += i
    check("range_runtime", total, 15)

    start = 2
    step = 3
    total = 0
    for i in range(start, limit + 5, step):
        total += i
    check("range_runtime_step", total, 15)

    values = [4, 8, 15, 16]
    total = 0
    for v in values:
        total += v
    check("list_loop", total, 43)

    order = 0
    for v in values by -1:
        order = order * 100 + v
    check("list_loop_reverse", order, 16150804)

    total = 0
    for v in values by 2:
        total += v
    check("list_loop_step", total, 19)

    total = 0
    for v in [start, start + 3, limit]:
        total += v
    check("runtime_literal_array", total, 13)

    grid = [[1, 2], [3, 4, 5]]
    row = 1
    total = 0
    for v in grid[row]:
        total += v
    check("list_path_loop", total, 12)

    cells = 0
    for i in range(3):
        for j in range(4):
            cells += 1
    check("nested_unrolled", cells, 12)

    countdown = 5
    ticks = 0
    while countdown > 0 and ticks < 100:
        countdown -= 1
        ticks += 1
    check("while_and", ticks, 5)

    for color in ["red", "blue"]:
        /team add arena_{color}
        /team modify arena_{color} color {color}


def test_match():
    check("match_literal", classify(0), 0)
    check("match_range_low", classify(5), 1)
    check("match_range_high", classify(42), 2)
    check("match_wildcard", classify(500), 3)
    check("match_negative", classify(-3), 3)

    phase = 2
    log = 0
    match phase:
        case 1:
            log = 10
        case 2:
            log = 20
            log += 1
        case _:
            log = 99
    check("match_multi_statement", log, 21)


def test_storage():
    config = {"count": 1, "nested": {"value": 2}, "name": "arena"}
    config.count = 3
    config.nested.value += 4
    check("storage_attr", config.count, 3)
    check("storage_nested_compound", config.nested.value, 6)

    items = [3, 5, 8]
    i = 1
    check("storage_dynamic_index", items[i], 5)
    items[i] = config.count + 10
    check("storage_index_assign", items[1], 13)

    key = "count"
    check("storage_dynamic_key", config[key], 3)
    check("len_list", len(items), 3)
    check("len_string", len(key), 5)
    check("len_map", len(config), 3)

    storage.set("events", [10, 20, 30])
    storage.append("events", 40)
    storage.prepend("events", 0)
    storage.insert("events", 2, 15)
    event_value = 0
    storage.read_score("event_value", storage.index("events", 2))
    check("storage_insert", event_value, 15)
    event_count = 0
    storage.read_score("event_count", "events")
    check("storage_list_size", event_count, 6)

    storage.set("status", {"ready": True, "count": 3})
    storage.merge("status", {"extra": 7})
    storage.copy("status_copy", "status")
    storage.remove("status.extra")
    copied = 0
    storage.read_score("copied", "status_copy.extra")
    check("storage_copy", copied, 7)
    removed = 1
    storage.read_score("removed", "status.extra")
    check("storage_remove", removed, 0)

    storage.set(storage.path("players", "current"), "alex")
    name_length = 0
    storage.read_score("name_length", storage.path("players", "current"))
    check("storage_path_helper", name_length, 4)


def test_helpers():
    points = 0
    backup = 0
    score.set("points", 10)
    score.add("points", 5)
    score.remove("points", 2)
    score.copy("backup", "points")
    score.operation("points", "+=", "backup")
    check("score_helpers", points, 26)
    score.reset("backup")
    check("score_reset", backup, 0)

    roll = 0
    coin = 5
    random.int("roll", 1, 6)
    random.bool("coin")
    in_range = 0
    if roll >= 1 and roll <= 6 and coin >= 0 and coin <= 1:
        in_range = 1
    check("random_helpers", in_range, 1)

    timer.set("cooldown", 2)
    timer.tick("cooldown")
    timer.tick("cooldown")
    cooldown_done = 0
    timer.done("cooldown")
    check("timer_done", cooldown_done, 1)

    schedule.once("scheduled_check", "2t")
    schedule.once("cancelled_check", "4t", "replace")
    schedule.clear("cancelled_check")

    bossbar.add("arena", {"text": "Arena", "color": "gold"})
    bossbar.set_max("arena", 100)
    bossbar.set_value("arena", 25)
    bossbar.set_color("arena", "red")
    bossbar.set_style("arena", "notched_10")
    bossbar.set_name("arena", text.colored("Arena Wave", "red"))
    bossbar.set_visible("arena", True)
    bossbar.set_players("arena", selector.all_players())
    bar_value = 0
    store result score bar_value temp:
        /bossbar get cobble_showcase:arena value
    check("bossbar_helpers", bar_value, 25)
    bossbar.remove("arena")

    score.objective.add("arena_kills", "dummy", "Kills")
    score.objective.display("sidebar", "arena_kills")
    score.objective.remove("arena_kills")

    text.tellraw("@a", text.colored("Self-test running", "aqua"))
    text.title("@a", {"text": "Arena", "bold": True})
    text.subtitle("@a", "Self-test")
    text.actionbar("@a", text.score("@s", "arena_hits"))
    text.tellraw(selector.all_players(), text.selector(selector.nearest_player()))


def test_entities():
    build_arena()

    seen = 0
    as @ArenaMarkers:
        seen += 1
        /tag @s add visited
    check("as_block_per_entity", seen, 3)

    visited = 0
    store result score visited temp:
        /execute if entity @e[tag=visited]
    check("store_block", visited, 3)

    /scoreboard objectives add arena_hits dummy
    as @ArenaMarkers:
        /scoreboard players set @s arena_hits 0
    # Each marker re-checks its own score, then runs the whole body once.
    as @ArenaMarkers if score @s arena_hits matches 0:
        /scoreboard players add @s arena_hits 1
        /scoreboard players add @s arena_hits 1
    twice = 0
    as @ArenaMarkers if score @s arena_hits matches 2:
        twice += 1
    check("execute_body_once_per_entity", twice, 3)

    near = 0
    positioned 0 100 0 as @e[type=minecraft:marker,tag=arena_marker,distance=..0.5]:
        near += 1
    check("positioned_distance", near, 1)

    in minecraft:overworld positioned 0 100 0 align xyz rotated 0 0 anchored feet:
        /summon minecraft:marker ^ ^ ^1 {Tags:["arena_probe"]}
    probe = 0
    positioned 0.0 100 1.0 as @e[type=minecraft:marker,tag=arena_probe,distance=..0.1]:
        probe += 1
    check("modifier_chain", probe, 1)

    guards = 0
    asat @Guards:
        /tp @s ~ ~1 ~
        guards += 1
    positioned 0 102 2 if entity @e[tag=arena_guard,distance=..0.1]:
        guards += 1
    check("asat_and_template", guards, 2)

    gate = 0
    unless entity @e[tag=no_such_tag]:
        gate += 1
    if predicate cobble_showcase:checks/always:
        gate += 1
    as @ArenaMarkers if seen == 3:
        gate += 1
    as @ArenaMarkers unless seen == 3:
        gate += 100
    check("execute_conditions", gate, 5)

    tag_all("@e[tag=arena_marker]", "macro_tagged")
    macro_tagged = 0
    as @e[tag=macro_tagged]:
        macro_tagged += 1
    check("macro_selector", macro_tagged, 3)

    entity.tag_add(selector.tagged("arena_guard"), "guard_tagged")
    entity.effect_give("@e[tag=arena_guard]", "minecraft:glowing", 30, 0, True)
    entity.attribute_base_set("@e[tag=arena_guard,limit=1]", "minecraft:max_health", 40)
    max_health = 0
    store result score max_health temp:
        entity.attribute_get("@e[tag=arena_guard,limit=1]", "minecraft:max_health")
    check("attribute_helpers", max_health, 40)
    entity.teleport(selector.tagged("guard_tagged"), position.absolute(0, 105, 0))
    teleported = 0
    positioned 0 105 0 if entity @e[tag=guard_tagged,distance=..0.1]:
        teleported = 1
    check("teleport_helper", teleported, 1)
    entity.effect_clear("@e[tag=arena_guard]", "minecraft:glowing")
    entity.tag_remove("@e[tag=arena_guard]", "guard_tagged")

    team.add("arena_team", "Arena Team")
    team.modify("arena_team", "color", "gold")
    team.modify("arena_team", "prefix", text.colored("[A] ", "gold"))
    team.join("arena_team", "@e[tag=arena_marker]")
    team.leave("@e[tag=arena_marker,limit=1]")
    on_team = 0
    as @e[team=arena_team]:
        on_team += 1
    check("team_helpers", on_team, 2)
    team.remove("arena_team")


def test_resources():
    global hook_calls
    hook_calls = 0
    /function #cobble_showcase:hooks/round_start
    check("function_tag", hook_calls, 2)

    floor_ok = 0
    if block 0 99 0 #cobble_showcase:arena_floor:
        floor_ok = 1
    check("block_tag", floor_ok, 1)

    typed = 0
    as @e[type=#cobble_showcase:arena_types,tag=arena_entity]:
        typed += 1
    check("entity_type_tag", typed, 4)

    /item replace entity @e[tag=arena_guard,limit=1] armor.head with minecraft:diamond
    /item modify entity @e[tag=arena_guard,limit=1] armor.head cobble_showcase:items/reward_name
    named = 0
    as @Guards if items entity @s armor.head #cobble_showcase:rewards[minecraft:custom_name]:
        named += 1
    check("item_tag_and_modifier", named, 1)

    /loot replace entity @e[tag=arena_guard,limit=1] weapon.mainhand loot cobble_showcase:chests/reward
    looted = 0
    store result score looted temp:
        /execute if items entity @e[tag=arena_guard,limit=1] weapon.mainhand minecraft:diamond
    check("loot_table", looted, 2)


def tag_all(target, tag_name):
    as {target}:
        /tag @s add {tag_name}


def round_hook_a():
    global hook_calls
    hook_calls += 1


def round_hook_b():
    global hook_calls
    hook_calls += 1


def scheduled_check():
    check("schedule_once", 1, 1)


def cancelled_check():
    check("schedule_clear", 0, 1)


def selftest():
    """Run every suite and print a summary."""
    global passed
    global failed
    passed = 0
    failed = 0
    /say SELFTEST start half={HALF} speed={SPEED} braces={{kept}}
    test_arithmetic()
    test_math()
    test_conditions()
    test_loops()
    test_match()
    test_storage()
    test_helpers()
    test_entities()
    test_resources()
    /say SELFTEST summary passed={passed} failed={failed}
