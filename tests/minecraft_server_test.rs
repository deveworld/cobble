use std::fs;
use std::io::Write;
use std::net::TcpListener;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::thread;
use std::time::{Duration, Instant};

const MINECRAFT_VERSION: &str = "26.3";
const PURPUR_BUILD: &str = "2652";
const NAMESPACE: &str = "cobble_server_smoke";
const SHOWCASE_NAMESPACE: &str = "cobble_showcase";
/// Two packs built from the same source with different step sizes, so a
/// shared objective would show up as mixed totals.
const ISOLATION_PACKS: [(&str, usize); 2] = [("cobble_iso_a", 1), ("cobble_iso_b", 10)];
const STARTUP_TIMEOUT: Duration = Duration::from_secs(90);
const COMMAND_TIMEOUT: Duration = Duration::from_secs(30);
const SHUTDOWN_TIMEOUT: Duration = Duration::from_secs(30);

#[test]
#[ignore = "requires Java, network/Purpur jar, and accepted Minecraft EULA"]
fn purpur_server_loads_and_runs_generated_datapack() {
    if std::env::var("COBBLE_MINECRAFT_EULA_ACCEPTED").as_deref() != Ok("1") {
        panic!(
            "set COBBLE_MINECRAFT_EULA_ACCEPTED=1 to confirm Minecraft EULA acceptance before running this test"
        );
    }

    let repo_root = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let jar = purpur_jar(&repo_root);
    let temp = tempfile::TempDir::new().unwrap();
    let project_dir = temp.path().join("project");
    let server_dir = temp.path().join("server");
    let datapacks_dir = server_dir.join("world/datapacks");

    write_project(&project_dir);
    fs::create_dir_all(&datapacks_dir).unwrap();
    build_datapack(
        &repo_root,
        &project_dir.join("src/main.cbl"),
        NAMESPACE,
        "Cobble Purpur server smoke test",
        &datapacks_dir.join(NAMESPACE),
    );
    build_datapack(
        &repo_root,
        &repo_root.join("tests/fixtures/projects/showcase/src/main.cbl"),
        SHOWCASE_NAMESPACE,
        "Cobble language showcase with an in-game self-test suite",
        &datapacks_dir.join(SHOWCASE_NAMESPACE),
    );
    for (namespace, step) in ISOLATION_PACKS {
        let entry = temp.path().join(namespace).join("main.cbl");
        fs::create_dir_all(entry.parent().unwrap()).unwrap();
        fs::write(&entry, isolation_source(step)).unwrap();
        build_datapack(
            &repo_root,
            &entry,
            namespace,
            "Cobble pack isolation check",
            &datapacks_dir.join(namespace),
        );
    }

    fs::write(
        server_dir.join("eula.txt"),
        "# Test-only server. EULA acceptance is guarded by COBBLE_MINECRAFT_EULA_ACCEPTED.\neula=true\n",
    )
    .unwrap();
    write_server_properties(&server_dir, free_port());
    seed_purpur_runtime_cache(&repo_root, &server_dir);

    let mut child = spawn_server(&jar, &server_dir);
    let result = run_server_smoke(&mut child, &server_dir);
    stop_server(&mut child);
    if result.is_ok() {
        persist_purpur_runtime_cache(&repo_root, &server_dir);
    }

    if let Err(error) = result {
        let log = read_log(&server_dir.join("logs/latest.log"));
        let console_log = read_log(&server_dir.join("server-console.log"));
        panic!("{error}\n\nserver-console.log:\n{console_log}\n\nlatest.log:\n{log}");
    }
}

fn purpur_jar(repo_root: &Path) -> PathBuf {
    if let Ok(path) = std::env::var("COBBLE_PURPUR_JAR") {
        let path = PathBuf::from(path);
        assert!(
            path.exists(),
            "COBBLE_PURPUR_JAR points to a missing file: {}",
            path.display()
        );
        return path;
    }

    let cache_dir = repo_root.join("target/minecraft-server-test");
    fs::create_dir_all(&cache_dir).unwrap();
    let jar = cache_dir.join(format!("purpur-{MINECRAFT_VERSION}-{PURPUR_BUILD}.jar"));
    if jar.exists() {
        return jar;
    }

    let partial = jar.with_extension("jar.part");
    let _ = fs::remove_file(&partial);
    let url =
        format!("https://api.purpurmc.org/v2/purpur/{MINECRAFT_VERSION}/{PURPUR_BUILD}/download");
    let status = Command::new("curl")
        .args(["-fL", "--retry", "3", "-o"])
        .arg(&partial)
        .arg(&url)
        .status()
        .expect("failed to execute curl while downloading Purpur");
    assert!(
        status.success(),
        "failed to download Purpur {MINECRAFT_VERSION} build {PURPUR_BUILD} from {url}"
    );
    fs::rename(partial, &jar).unwrap();
    jar
}

fn write_project(project_dir: &Path) {
    let src_dir = project_dir.join("src");
    fs::create_dir_all(&src_dir).unwrap();
    fs::write(
        src_dir.join("helpers.cbl"),
        r#"def imported_ready(value):
    return value
"#,
    )
    .unwrap();
    fs::write(
        src_dir.join("main.cbl"),
        r##"import stdlib
from helpers import imported_ready
from stdlib import event

counter = 0
smoke_loop_flag = 0
choice_results = []

# Data pack resources in the target version's shapes: a server whose packs
# fail to load refuses to start, so any format drift fails this test.
datapack.predicate("checks/always", {"type": "minecraft:all_of", "terms": [{"type": "minecraft:random_chance", "chance": 1}, {"type": "minecraft:inverted", "term": {"type": "minecraft:random_chance", "chance": 0}}]})
datapack.item_modifier("items/double", {"type": "minecraft:set_count", "count": 2, "condition": {"type": "minecraft:random_chance", "chance": 1}})
datapack.loot_table("chests/smoke", {"type": "minecraft:chest", "pools": [{"rolls": 1, "condition": {"type": "minecraft:random_chance", "chance": 1}, "entries": [{"type": "minecraft:item", "name": "minecraft:diamond", "modifier": {"type": "minecraft:set_count", "count": 2}}, {"type": "minecraft:tag", "items": "#minecraft:logs", "expand": True}]}]})
datapack.advancement("smoke/root", {"display": {"icon": {"id": "minecraft:diamond"}, "title": "Smoke", "description": "Smoke", "background": "minecraft:gui/advancements/backgrounds/stone"}, "criteria": {"tick": {"trigger": "minecraft:tick"}}})
datapack.recipe("smoke_smelt", {"type": "minecraft:smelting", "ingredient": "minecraft:cobblestone", "result": {"id": "minecraft:stone"}, "cookingtime": 200})
datapack.dialog("notice", {"type": "minecraft:notice", "title": {"text": "Smoke"}})

# `a_early` sorts before `init`, which creates the pack objective; the setup
# handler must still run first.
datapack.function_tag("minecraft:load", ["cobble_server_smoke:a_early"])

define @ProbeMarker = @e[type=minecraft:marker,tag=cobble_tagged]
create {"Tags": ["cobble_probe_extra"]}
end

def a_early():
    /scoreboard players add early_hits temp 1
    /execute if score early_hits temp matches 1.. run say early load ok

def hover_container(value):
    /tellraw @a {text:'hover',hover_event:{action:'show_entity',id:'minecraft:pig',uuid:[I;0,0,0,1],name:f'{value}'}}

def hover_container_check():
    positioned 0 64 0:
        hover_container(["a", "b"])
        # Decode the escaped argument as the hover macro does. No client
        # needs to be connected to observe whether its text is preserved.
        /function cobble_server_smoke:_cobble_esc_paste_done with storage cobble_server_smoke:global _cobble_esc
        /execute if data storage cobble_server_smoke:global _cobble_esc{rt:'["a", "b"]'} run say hover container ok

def init():
    /say Cobble quiet load ok
    /execute if predicate cobble_server_smoke:checks/always run say predicate ok
    /data modify storage cobble:server_smoke status set value "loaded"
    /version
    /waypoint list
    /stopwatch create cobble:server_smoke
    /stopwatch query cobble:server_smoke 1.0

def numeric_twice(value):
    return value + value

def random_pick(low, high):
    return random.randint(low, high)

def return_payload(depth):
    if depth == 0:
        return {"name": "Alex", "items": [1, 2]}
    return return_payload(depth - 1)

def change_visibility(visible):
    bossbar.set_visible("runtime_ui", visible)

def formatted_header(value):
    as @e[tag=format_probe,name=f"Name {value + 1}"]:
        value += 1
    return value

def scheduled_consistency_probe():
    /say dynamic namespaced schedule ok

def parameter_loop(value, count):
    for value in range(count):
        if value == 2:
            break
    return value

def show_component(component):
    text.tellraw("@a", component)

def consistency_probe():
    payload = return_payload(2)
    message = f"{payload.name}: {len(payload.items) + 1}"
    half = 1 / 2
    integer_root = math.sqrt(2)
    /execute if data storage cobble_server_smoke:global locals.consistency_probe{message:"Alex: 3",half:0.5f,integer_root:1.4142135f} run say storage returns and expressions ok
    amount = 12
    bossbar.add("runtime_ui", "Runtime")
    bossbar.set_max("runtime_ui", amount + 3)
    bossbar.set_value("runtime_ui", amount)
    visible = False
    bossbar.set_visible("runtime_ui", visible)
    change_visibility(True)
    /execute store result storage cobble_server_smoke:global ui_value int 1 run bossbar get cobble_server_smoke:runtime_ui value
    /execute if data storage cobble_server_smoke:global {ui_value:12} run say runtime helper arguments ok
    timer.set("probe_timer", amount - 11)
    timer.tick("probe_timer")
    ready = timer.done("probe_timer")
    path = "probe_status"
    storage.set(path, payload)
    saved = storage.get(path)
    storage.set("probe_list", [10, 20])
    index = 1
    indexed = storage.get(storage.index("probe_list", index))
    /execute if data storage cobble_server_smoke:global locals.consistency_probe{ready:1,indexed:20,saved:{name:"Alex",items:[1,2]}} run say value getters ok
    total = 0
    for i in range(6):
        if i == 1:
            continue
        if i == 4:
            break
        total += i
    count = 0
    while True:
        count += 1
        if count < 3:
            continue
        break
    /execute if data storage cobble_server_smoke:global locals.consistency_probe{total:5,count:3} run say loop control ok
    /summon minecraft:marker 0 64 0 {Tags:["format_probe"],CustomName:"Name 3"}
    header_value = formatted_header(2)
    /execute if data storage cobble_server_smoke:global locals.consistency_probe{header_value:3} run say formatted header arguments ok
    /kill @e[tag=format_probe]
    full_id = "cobble_server_smoke:runtime_ui"
    bossbar.set_value(full_id, 13)
    /execute store result storage cobble_server_smoke:global ui_value int 1 run bossbar get cobble_server_smoke:runtime_ui value
    /execute if data storage cobble_server_smoke:global {ui_value:13} run say dynamic namespaced bossbar ok
    scheduled = "cobble_server_smoke:scheduled_consistency_probe"
    schedule.once(scheduled, "1t")
    selected = parameter_loop(9, 5)
    joined = return_payload(0).name + return_payload(0).name
    destination = position.relative(0, 1, 0)
    target = selector.all_players()
    component = text.colored(f"{joined}", "gold")
    show_component(component)
    bossbar.set_name(full_id, component)
    plain_component = text.plain("Quotes: \"hello\" and \\")
    show_component(plain_component)
    /execute if data storage cobble_server_smoke:global locals.consistency_probe{selected:2,joined:"AlexAlex",destination:"~ ~1 ~",target:"@a",component:{text:"AlexAlex",color:"gold"}} run say composed values and parameter loops ok
    bossbar.remove(full_id)

def random_probe():
    low = 1
    high = 6
    fixed = random.randint(4, 4)
    smallest = random_pick(-2147483648, -2147483648)
    largest = random_pick(2147483647, 2147483647)
    invalid = random_pick(float(1), 2)
    /execute if data storage cobble_server_smoke:global locals.random_probe{fixed:4,smallest:-2147483648,largest:2147483647,invalid:0} run say random fixed and guards ok
    passed = 0
    for index in range(32):
        value = random_pick(low, high)
        if value >= 1 and value <= 6:
            passed += 1
        positive = random_pick(0, 2147483647)
        negative = random_pick(-2147483648, -1)
        full = random_pick(-2147483648, 2147483647)
        if positive >= 0 and negative <= -1:
            passed += 1
    if passed == 64:
        /say random ranges ok

def numeric_probe():
    single = 0.6 + 0.2
    wide = double(0.1) + double(0.2)
    doubled = numeric_twice(wide)
    root = math.sqrt(double(2))
    boundary = double(16777216) + double(1)
    rounded = float(16777216) + 1
    floor = double(-5) // 2
    remainder = -5.5 % 2
    absolute_zero = abs(double(-0.0))
    /execute if data storage cobble_server_smoke:global locals.numeric_probe{single:0.8f,wide:0.30000000000000004d,doubled:0.6000000000000001d,root:1.4142135623730951d,boundary:16777217.0d,rounded:16777216.0f,floor:-3.0d,remainder:0.5f,absolute_zero:0.0d} run say numeric values ok
    if wide > double(0.3) and wide < double(0.31):
        /say numeric comparisons ok
    zero = 0.0
    invalid_single = single / zero
    invalid_wide = double(1) / double(zero)
    /execute if data storage cobble_server_smoke:global locals.numeric_probe{invalid_single:0.0f,invalid_wide:0.0d} run say numeric invalid guards ok

def ready_value(value):
    return value

def should_continue():
    return smoke_loop_flag

def return_from_if(flag):
    if flag == 1:
        return 7
    return 0

def return_from_match(value):
    match value:
        case 2:
            return 5
        case _:
            return 0

def fraction_macro(value):
    /data modify storage cobble:server_smoke fraction set value {value}

def fraction_forward(values):
    fraction_macro(values[0])

def integer_macro(value):
    /scoreboard players set fraction_integer temp {value}

def integer_forward(values):
    integer_macro(values[0])

def plain_message(value):
    /tellraw @a {value}
    /scoreboard players add plain_hits temp 1

def mixed_values(flag):
    choice_results.append("text" if flag else [1])
    choice_results.append({"k": 2} if flag else 7)

def number_mirror(given):
    a = given or 1
    n = 0
    while n < 2:
        if a > 0:
            a = 1
            a /= -1
            /scoreboard players add mirror_hits temp 1
        n += 1

def minecraft_header_return():
    if score header_n temp matches 99:
        return 1
    elif predicate cobble_server_smoke:checks/always:
        return 8
    else:
        return 0

def probe():
    global counter
    global smoke_loop_flag
    counter = counter + 1

    if counter >= 1:
        /say probe counter {counter}

    if ready_value(1):
        /say function call condition ok

    imported_value = imported_ready(4)
    if imported_value == 4 and imported_ready(1):
        /say imported function expression condition ok

    positioned ~ ~ ~ if ready_value(1):
        /say execute function call condition ok
    positioned ~ ~ ~ unless ready_value(0):
        /say execute function call unless ok

    storage_target = {"count": 1, "nested": {"value": 2}}
    storage_target.count = 3
    storage_target.nested.value += 4
    if storage_target.count == 3 and storage_target.nested.value == 6:
        /say storage assignment target server ok

    returned_from_if = return_from_if(1)
    returned_from_match = return_from_match(2)
    if returned_from_if == 7 and returned_from_match == 5:
        /say cobble return propagation server ok

    match counter:
        case 1:
            /say probe first
        case _:
            /say probe later

    for i in range(2):
        /say probe loop {i}

    i = 1
    items = [3, 5, 8]
    picked = items[i]
    if picked == 5:
        /say dynamic picked 5

    storage.set("events", [10, 20, 30])
    picked_again = 0
    storage.read_score("picked_again", storage.index("events", i))
    if picked_again == 20:
        /say storage index ok

    runtime_values = [picked, picked_again]
    runtime_state = {"picked": picked, "again": picked_again}
    runtime_picked = runtime_state.picked
    if runtime_picked == 5:
        /say runtime literal ok
    map_key = "picked"
    dynamic_map_value = runtime_state[map_key]
    if dynamic_map_value == 5:
        /say dynamic map key ok

    loop_i = 0
    while loop_i < 2:
        loop_pick = items[loop_i]
        loop_i = loop_i + 1
    if loop_pick == 5:
        /say dynamic while ok
    smoke_loop_flag = 1
    while should_continue():
        /say while function call condition ok
        smoke_loop_flag = 0

    limit = 3
    dynamic_total = 0
    for dyn_i in range(limit):
        dynamic_total = dynamic_total + dyn_i
    if dynamic_total == 3:
        /say dynamic for ok
    step_size = 2
    stepped_total = 0
    for step_i in range(0, limit, step_size):
        stepped_total = stepped_total + step_i
    if stepped_total == 2:
        /say dynamic for step ok
    list_step = -1
    list_total = 0
    for list_value in runtime_values by list_step:
        list_total = list_total + list_value
    if list_total == 25:
        /say runtime list for ok
    literal_loop_total = 0
    for literal_value in [picked, picked_again, ready_value(1)]:
        literal_loop_total = literal_loop_total + literal_value
    if literal_loop_total == 26:
        /say runtime literal array for ok
    groups = [[2, 3], [7, 11]]
    group_i = 1
    group_total = 0
    for group_value in groups[group_i]:
        group_total = group_total + group_value
    if group_total == 18:
        /say dynamic list path for ok
    runtime_values_len = len(runtime_values)
    map_key_len = len(map_key)
    if runtime_values_len == 2 and map_key_len == 6:
        /tellraw @a [{"text":f"runtime array len {runtime_values_len}","color":"green"},{"text":f" key {map_key}"}]
        /say runtime len ok
        /say runtime json array ok
    pow_base = 3
    pow_exp = 4
    pow_result = pow_base ** pow_exp
    pow_zero = pow_base ** (pow_exp - 4)
    if pow_result == 81 and pow_zero == 1:
        /say runtime power ok
    compound_score = 10
    compound_score += 5
    compound_score -= 3
    compound_score *= 2
    compound_score /= 4
    compound_score %= 5
    compound_base = 2
    compound_base **= pow_exp
    if compound_score == 1 and compound_base == 16:
        /say compound assignment ok

    mixed_fractions = [compound_score, 0.6]
    fraction_macro(mixed_fractions[1])
    /data modify storage cobble:server_smoke fraction_roundtrip set compute default float {type:"minecraft:storage",storage:"cobble:server_smoke",path:"fraction"}
    /execute if data storage cobble:server_smoke {fraction_roundtrip:0.6f} run say mixed fraction precision ok
    fraction_forward([0.6])
    /data modify storage cobble:server_smoke fraction_roundtrip set compute default float {type:"minecraft:storage",storage:"cobble:server_smoke",path:"fraction"}
    /execute if data storage cobble:server_smoke {fraction_roundtrip:0.6f} run say forwarded fraction precision ok
    integer_forward([1.5])
    /execute if score fraction_integer temp matches 1 run say raw integer fraction ok
    integer_forward([-0.6])
    /execute if score fraction_integer temp matches -1 run say raw integer negative fraction ok
    integer_forward(["15"])
    /execute if score fraction_integer temp matches 15 run say raw integer text ok

    plain_message("say \"hi\" \\ end")
    plain_message("한글 \\")
    /execute if score plain_hits temp matches 2 run say plain message escaping ok
    mixed_values(1)
    mixed_values(0)
    /execute if data storage cobble_server_smoke:global {vars:{choice_results:["text",{k:2},[1],7]}} run say mixed value choice ok
    selected = "abc" if ready_value(1) else [4, 5]
    if selected == "abc" and len(selected) == 3:
        /say selected value operations ok
    number_mirror(3)
    number_mirror(0)
    /execute if score mirror_hits temp matches 2 run say numeric storage mirror ok

    header_n = 0
    while predicate cobble_server_smoke:checks/always and header_n in [0, 1]:
        header_n += 1
    while score header_n temp matches ..3:
        header_n += 1
    header_result = minecraft_header_return()
    if header_n == 4 and header_result == 8:
        /say minecraft elif while ok

    # `say` with a variable keeps say's own output: the sender prefix and
    # selectors shown as names.
    # The console runs at the world spawn, whose chunk may not be loaded; the
    # showcase pack force-loads the chunks around 0 0.
    positioned 0 64 0:
        /summon minecraft:marker ~ ~ ~ {Tags:["cobble_probe"]}
        create @ProbeMarker
    /say probe selector {counter} @e[type=minecraft:marker,tag=cobble_probe,limit=1]
    /execute if entity @e[type=minecraft:marker,tag=cobble_tagged,tag=cobble_probe_extra] run say template tags ok
    unless entity @e[tag=cobble_missing_a] or entity @e[tag=cobble_missing_b]:
        /say unless or ok

    /data get storage cobble:server_smoke status
    /return run say return command ok

stdlib.addEventListener(event.LOAD, init)
"##,
    )
    .unwrap();
}

/// Both isolation packs use the same variable names; `visits` persists across
/// `/reload` and `round` resets.
fn isolation_source(step: usize) -> String {
    format!(
        r#"persist visits = 0
round = 0

def bump():
    """Adds this pack's step to both counters."""
    global visits
    global round
    visits = visits + {step}
    round = round + {step}
"#
    )
}

fn build_datapack(
    repo_root: &Path,
    entry: &Path,
    namespace: &str,
    description: &str,
    datapack_dir: &Path,
) {
    let commands_json = repo_root.join("data/commands.json");
    cobble::commands::build::build(cobble::commands::build::BuildOptions {
        input: Some(entry.to_path_buf()),
        output: Some(datapack_dir.to_path_buf()),
        namespace: Some(namespace.to_string()),
        pack_format: Some("121.0".to_string()),
        description: Some(description.to_string()),
        verbose: false,
        quiet: false,
        zip: false,
        experimental_resource_pack: false,
        validate: commands_json.exists(),
        dry_run: false,
        commands_json,
    })
    .unwrap();
}

fn write_server_properties(server_dir: &Path, port: u16) {
    fs::create_dir_all(server_dir).unwrap();
    fs::write(
        server_dir.join("server.properties"),
        format!(
            r#"allow-flight=false
difficulty=peaceful
enable-jmx-monitoring=false
enable-query=false
enable-rcon=false
enable-status=false
enforce-secure-profile=true
enforce-whitelist=false
force-gamemode=false
function-permission-level=4
gamemode=creative
generate-structures=true
hardcore=false
hide-online-players=false
initial-disabled-packs=
initial-enabled-packs=vanilla
level-name=world
level-type=minecraft\:normal
log-ips=false
max-players=2
max-tick-time=60000
motd=Cobble Purpur Server Test
network-compression-threshold=256
online-mode=false
op-permission-level=4
pause-when-empty-seconds=-1
player-idle-timeout=0
query.port={port}
rate-limit=0
server-ip=127.0.0.1
server-port={port}
simulation-distance=2
spawn-protection=0
sync-chunk-writes=true
use-native-transport=true
view-distance=2
white-list=false
"#
        ),
    )
    .unwrap();
}

fn seed_purpur_runtime_cache(repo_root: &Path, server_dir: &Path) {
    if let Ok(cache_dir) = std::env::var("COBBLE_PURPUR_SERVER_CACHE_DIR") {
        copy_purpur_runtime_cache(Path::new(&cache_dir), server_dir);
    }

    let persistent_cache = repo_root.join("target/minecraft-server-test/runtime-cache");
    copy_purpur_runtime_cache(&persistent_cache, server_dir);
}

fn persist_purpur_runtime_cache(repo_root: &Path, server_dir: &Path) {
    let persistent_cache = repo_root.join("target/minecraft-server-test/runtime-cache");
    copy_purpur_runtime_cache(server_dir, &persistent_cache);
}

fn copy_purpur_runtime_cache(from: &Path, to: &Path) {
    copy_if_exists(
        &from.join(format!("cache/mojang_{MINECRAFT_VERSION}.jar")),
        &to.join(format!("cache/mojang_{MINECRAFT_VERSION}.jar")),
    );
    copy_if_exists(
        &from.join(format!(
            "versions/{MINECRAFT_VERSION}/purpur-{MINECRAFT_VERSION}.jar"
        )),
        &to.join(format!(
            "versions/{MINECRAFT_VERSION}/purpur-{MINECRAFT_VERSION}.jar"
        )),
    );
}

fn copy_if_exists(from: &Path, to: &Path) {
    if !from.exists() {
        return;
    }
    if let Some(parent) = to.parent() {
        fs::create_dir_all(parent).unwrap();
    }
    fs::copy(from, to).unwrap();
}

fn spawn_server(jar: &Path, server_dir: &Path) -> Child {
    let console_log = fs::File::create(server_dir.join("server-console.log")).unwrap();
    let console_err = console_log.try_clone().unwrap();

    Command::new("java")
        .args(["-Xms512M", "-Xmx1536M", "-jar"])
        .arg(jar)
        .arg("nogui")
        .current_dir(server_dir)
        .stdin(Stdio::piped())
        .stdout(Stdio::from(console_log))
        .stderr(Stdio::from(console_err))
        .spawn()
        .expect("failed to start Purpur; is Java installed?")
}

fn run_server_smoke(child: &mut Child, server_dir: &Path) -> Result<(), String> {
    let log_path = server_dir.join("logs/latest.log");
    wait_for_log(child, &log_path, "Done (", STARTUP_TIMEOUT)?;
    // Test-world opt-in: production packs no longer change this gamerule.
    send_command(child, "gamerule max_command_sequence_length 1000000000")?;
    wait_for_log(child, &log_path, "Cobble quiet load ok", COMMAND_TIMEOUT)?;
    wait_for_log(child, &log_path, "predicate ok", COMMAND_TIMEOUT)?;
    wait_for_log(child, &log_path, "early load ok", COMMAND_TIMEOUT)?;

    send_command(child, "datapack list")?;
    wait_for_log(
        child,
        &log_path,
        &format!("file/{NAMESPACE}"),
        COMMAND_TIMEOUT,
    )?;
    wait_for_log(
        child,
        &log_path,
        &format!("file/{SHOWCASE_NAMESPACE}"),
        COMMAND_TIMEOUT,
    )?;
    for (namespace, _) in ISOLATION_PACKS {
        wait_for_log(
            child,
            &log_path,
            &format!("file/{namespace}"),
            COMMAND_TIMEOUT,
        )?;
    }

    // Entity selectors need a ticking chunk; the console's world spawn
    // need not be loaded. Give force-loading time before the marker probe.
    send_command(child, "forceload add 0 0")?;
    send_command(
        child,
        &format!("schedule function {NAMESPACE}:consistency_probe 20t"),
    )?;
    for expected in [
        "storage returns and expressions ok",
        "runtime helper arguments ok",
        "value getters ok",
        "loop control ok",
        "formatted header arguments ok",
        "dynamic namespaced bossbar ok",
        "dynamic namespaced schedule ok",
        "composed values and parameter loops ok",
    ] {
        wait_for_log(child, &log_path, expected, COMMAND_TIMEOUT)?;
    }
    send_command(child, &format!("function {NAMESPACE}:random_probe"))?;
    for expected in ["random fixed and guards ok", "random ranges ok"] {
        wait_for_log(child, &log_path, expected, COMMAND_TIMEOUT)?;
    }
    send_command(child, &format!("function {NAMESPACE}:numeric_probe"))?;
    for expected in [
        "numeric values ok",
        "numeric comparisons ok",
        "numeric invalid guards ok",
    ] {
        wait_for_log(child, &log_path, expected, COMMAND_TIMEOUT)?;
    }

    send_command(child, &format!("function {NAMESPACE}:probe"))?;
    wait_for_log(
        child,
        &log_path,
        &format!("Function {NAMESPACE}:probe returned 1"),
        COMMAND_TIMEOUT,
    )?;
    wait_for_log(child, &log_path, "probe first", COMMAND_TIMEOUT)?;
    wait_for_log(
        child,
        &log_path,
        "function call condition ok",
        COMMAND_TIMEOUT,
    )?;
    wait_for_log(
        child,
        &log_path,
        "imported function expression condition ok",
        COMMAND_TIMEOUT,
    )?;
    wait_for_log(
        child,
        &log_path,
        "execute function call condition ok",
        COMMAND_TIMEOUT,
    )?;
    wait_for_log(
        child,
        &log_path,
        "execute function call unless ok",
        COMMAND_TIMEOUT,
    )?;
    wait_for_log(
        child,
        &log_path,
        "storage assignment target server ok",
        COMMAND_TIMEOUT,
    )?;
    wait_for_log(
        child,
        &log_path,
        "cobble return propagation server ok",
        COMMAND_TIMEOUT,
    )?;
    wait_for_log(child, &log_path, "dynamic picked 5", COMMAND_TIMEOUT)?;
    wait_for_log(child, &log_path, "storage index ok", COMMAND_TIMEOUT)?;
    wait_for_log(child, &log_path, "runtime literal ok", COMMAND_TIMEOUT)?;
    wait_for_log(child, &log_path, "dynamic map key ok", COMMAND_TIMEOUT)?;
    wait_for_log(child, &log_path, "dynamic while ok", COMMAND_TIMEOUT)?;
    wait_for_log(
        child,
        &log_path,
        "while function call condition ok",
        COMMAND_TIMEOUT,
    )?;
    wait_for_log(child, &log_path, "dynamic for ok", COMMAND_TIMEOUT)?;
    wait_for_log(child, &log_path, "dynamic for step ok", COMMAND_TIMEOUT)?;
    wait_for_log(child, &log_path, "runtime list for ok", COMMAND_TIMEOUT)?;
    wait_for_log(
        child,
        &log_path,
        "runtime literal array for ok",
        COMMAND_TIMEOUT,
    )?;
    wait_for_log(
        child,
        &log_path,
        "dynamic list path for ok",
        COMMAND_TIMEOUT,
    )?;
    wait_for_log(child, &log_path, "runtime len ok", COMMAND_TIMEOUT)?;
    wait_for_log(child, &log_path, "runtime json array ok", COMMAND_TIMEOUT)?;
    wait_for_log(child, &log_path, "runtime power ok", COMMAND_TIMEOUT)?;
    wait_for_log(child, &log_path, "compound assignment ok", COMMAND_TIMEOUT)?;
    for message in [
        "mixed fraction precision ok",
        "forwarded fraction precision ok",
        "raw integer fraction ok",
        "raw integer negative fraction ok",
        "raw integer text ok",
        "plain message escaping ok",
        "mixed value choice ok",
        "selected value operations ok",
        "numeric storage mirror ok",
        "minecraft elif while ok",
    ] {
        wait_for_log(child, &log_path, message, COMMAND_TIMEOUT)?;
    }
    wait_for_log(
        child,
        &log_path,
        "[Server] probe selector 1 Marker",
        COMMAND_TIMEOUT,
    )?;
    wait_for_log(child, &log_path, "template tags ok", COMMAND_TIMEOUT)?;
    wait_for_log(child, &log_path, "unless or ok", COMMAND_TIMEOUT)?;
    wait_for_log(child, &log_path, "return command ok", COMMAND_TIMEOUT)?;

    // Force-loading is asynchronous; run after the chunk reaches entity
    // ticking status so the temporary text display can resolve its value.
    send_command(child, "forceload add 0 0")?;
    send_command(
        child,
        &format!("schedule function {NAMESPACE}:hover_container_check 20t"),
    )?;
    wait_for_log(child, &log_path, "hover container ok", COMMAND_TIMEOUT)?;

    run_showcase_selftest(child, &log_path)?;

    for (namespace, _) in ISOLATION_PACKS {
        send_command(child, &format!("function {namespace}:bump"))?;
    }
    for (namespace, step) in ISOLATION_PACKS {
        expect_variable(child, &log_path, namespace, "round", step)?;
        expect_variable(child, &log_path, namespace, "visits", step)?;
    }

    send_command(child, "reload")?;
    wait_for_log_count(child, &log_path, "Cobble quiet load ok", 2, COMMAND_TIMEOUT)?;

    // After `/reload`, plain module variables are back at 0 and `persist`
    // variables keep their values.
    for (namespace, step) in ISOLATION_PACKS {
        expect_variable(child, &log_path, namespace, "round", 0)?;
        expect_variable(child, &log_path, namespace, "visits", step)?;
    }

    send_command(child, "data get storage cobble:server_smoke status")?;
    wait_for_log(
        child,
        &log_path,
        "Storage cobble:server_smoke has the following contents: \"loaded\"",
        COMMAND_TIMEOUT,
    )?;

    let log = read_log(&log_path);
    assert_no_server_errors(&log)?;
    let console_log = read_log(&server_dir.join("server-console.log"));
    assert_no_server_errors(&console_log)?;
    Ok(())
}

/// Runs the showcase fixture's in-game self-test. Each check prints
/// `PASS <label>` or `FAIL <label> expected <x> got <y>`. The totals are read
/// back as scores, since the summary line prints before the scheduled checks
/// finish.
fn run_showcase_selftest(child: &mut Child, log_path: &Path) -> Result<(), String> {
    send_command(child, &format!("function {SHOWCASE_NAMESPACE}:selftest"))?;
    // The last check is scheduled 2 ticks out; a cleared schedule would
    // report a failure 4 ticks out, so give it time to show up.
    wait_for_log(child, log_path, "PASS schedule_once", COMMAND_TIMEOUT)?;
    thread::sleep(Duration::from_millis(500));

    let objective = format!("{SHOWCASE_NAMESPACE}.vars");
    send_command(child, &format!("scoreboard players get passed {objective}"))?;
    send_command(child, &format!("scoreboard players get failed {objective}"))?;
    wait_for_log(child, log_path, "passed has ", COMMAND_TIMEOUT)?;
    wait_for_log(child, log_path, "failed has ", COMMAND_TIMEOUT)?;

    let log = read_log(log_path);
    let failures = log
        .lines()
        .filter(|line| line.contains("] FAIL "))
        .collect::<Vec<_>>();
    if !failures.is_empty() {
        return Err(format!(
            "showcase self-test failed:\n{}",
            failures.join("\n")
        ));
    }
    let pass_lines = log.lines().filter(|line| line.contains("] PASS ")).count();
    let passed = logged_score(&log, "passed", &objective)?;
    let failed = logged_score(&log, "failed", &objective)?;
    if failed != 0 || passed == 0 || passed != pass_lines {
        return Err(format!(
            "showcase self-test totals are off: passed={passed} failed={failed}, {pass_lines} PASS lines"
        ));
    }
    eprintln!("showcase self-test: {passed} passed, 0 failed");
    Ok(())
}

/// The last `<holder> has <n> [<objective>]` value in the server log.
fn logged_score(log: &str, holder: &str, objective: &str) -> Result<usize, String> {
    let needle = format!("{holder} has ");
    let suffix = format!(" [{objective}]");
    log.lines()
        .rev()
        .find_map(|line| {
            let (_, rest) = line.split_once(&needle)?;
            rest.strip_suffix(&suffix)?.parse().ok()
        })
        .ok_or_else(|| format!("no `{holder}` score on `{objective}` in the server log"))
}

/// Reads canonical storage; integer score registers may be stale after a call.
fn expect_variable(
    child: &mut Child,
    log_path: &Path,
    namespace: &str,
    name: &str,
    expected: usize,
) -> Result<(), String> {
    let needle = format!("canonical {namespace}.{name}={expected}");
    let before = read_log(log_path).matches(&needle).count();
    send_command(
        child,
        &format!(
            "execute if data storage {namespace}:global vars{{{name}:{expected}}} run say {needle}"
        ),
    )?;
    wait_for_log_count(child, log_path, &needle, before + 1, COMMAND_TIMEOUT)
}

fn send_command(child: &mut Child, command: &str) -> Result<(), String> {
    if let Some(status) = child
        .try_wait()
        .map_err(|e| format!("failed to inspect server process: {e}"))?
    {
        return Err(format!(
            "server exited before command `{command}`: {status}"
        ));
    }

    let stdin = child
        .stdin
        .as_mut()
        .ok_or_else(|| "server stdin is not available".to_string())?;
    writeln!(stdin, "{command}").map_err(|e| format!("failed to send server command: {e}"))?;
    stdin
        .flush()
        .map_err(|e| format!("failed to flush server stdin: {e}"))
}

fn stop_server(child: &mut Child) {
    if matches!(child.try_wait(), Ok(Some(_))) {
        return;
    }
    let _ = send_command(child, "stop");
    let deadline = Instant::now() + SHUTDOWN_TIMEOUT;
    while Instant::now() < deadline {
        if matches!(child.try_wait(), Ok(Some(_))) {
            return;
        }
        thread::sleep(Duration::from_millis(100));
    }
    let _ = child.kill();
    let _ = child.wait();
}

fn wait_for_log(
    child: &mut Child,
    log_path: &Path,
    needle: &str,
    timeout: Duration,
) -> Result<(), String> {
    let deadline = Instant::now() + timeout;
    while Instant::now() < deadline {
        if read_log(log_path).contains(needle) {
            return Ok(());
        }
        if let Some(status) = child
            .try_wait()
            .map_err(|e| format!("failed to inspect server process: {e}"))?
        {
            return Err(format!(
                "server exited before log entry appeared: `{needle}` ({status})"
            ));
        }
        thread::sleep(Duration::from_millis(100));
    }
    Err(format!("timed out waiting for log entry: {needle}"))
}

fn wait_for_log_count(
    child: &mut Child,
    log_path: &Path,
    needle: &str,
    min_count: usize,
    timeout: Duration,
) -> Result<(), String> {
    let deadline = Instant::now() + timeout;
    while Instant::now() < deadline {
        if read_log(log_path).matches(needle).count() >= min_count {
            return Ok(());
        }
        if let Some(status) = child
            .try_wait()
            .map_err(|e| format!("failed to inspect server process: {e}"))?
        {
            return Err(format!(
                "server exited before {min_count} log entries appeared: `{needle}` ({status})"
            ));
        }
        thread::sleep(Duration::from_millis(100));
    }
    Err(format!(
        "timed out waiting for at least {min_count} log entries: {needle}"
    ))
}

fn read_log(log_path: &Path) -> String {
    fs::read_to_string(log_path).unwrap_or_default()
}

fn assert_no_server_errors(log: &str) -> Result<(), String> {
    let forbidden = [
        "[ERROR]",
        "Exception",
        "Failed to execute",
        "Failed to load data packs",
        "Unknown or incomplete command",
        "Unknown function",
        "Invalid command",
        "Couldn't parse",
        "Could not parse",
    ];
    let offenders = forbidden
        .iter()
        .filter(|needle| log.contains(**needle))
        .copied()
        .collect::<Vec<_>>();

    if offenders.is_empty() {
        Ok(())
    } else {
        Err(format!(
            "server log contains unexpected error markers: {}",
            offenders.join(", ")
        ))
    }
}

fn free_port() -> u16 {
    let listener = TcpListener::bind(("127.0.0.1", 0)).unwrap();
    listener.local_addr().unwrap().port()
}
