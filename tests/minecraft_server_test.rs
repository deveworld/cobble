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

# Data pack resources in the target version's shapes: a server whose packs
# fail to load refuses to start, so any format drift fails this test.
datapack.predicate("checks/always", {"type": "minecraft:all_of", "terms": [{"type": "minecraft:random_chance", "chance": 1}, {"type": "minecraft:inverted", "term": {"type": "minecraft:random_chance", "chance": 0}}]})
datapack.item_modifier("items/double", {"type": "minecraft:set_count", "count": 2, "condition": {"type": "minecraft:random_chance", "chance": 1}})
datapack.loot_table("chests/smoke", {"type": "minecraft:chest", "pools": [{"rolls": 1, "condition": {"type": "minecraft:random_chance", "chance": 1}, "entries": [{"type": "minecraft:item", "name": "minecraft:diamond", "modifier": {"type": "minecraft:set_count", "count": 2}}, {"type": "minecraft:tag", "items": "#minecraft:logs", "expand": True}]}]})
datapack.advancement("smoke/root", {"display": {"icon": {"id": "minecraft:diamond"}, "title": "Smoke", "description": "Smoke", "background": "minecraft:gui/advancements/backgrounds/stone"}, "criteria": {"tick": {"trigger": "minecraft:tick"}}})
datapack.recipe("smoke_smelt", {"type": "minecraft:smelting", "ingredient": "minecraft:cobblestone", "result": {"id": "minecraft:stone"}, "cookingtime": 200})
datapack.dialog("notice", {"type": "minecraft:notice", "title": {"text": "Smoke"}})

def init():
    /say Cobble quiet load ok
    /execute if predicate cobble_server_smoke:checks/always run say predicate ok
    /data modify storage cobble:server_smoke status set value "loaded"
    /version
    /waypoint list
    /stopwatch create cobble:server_smoke
    /stopwatch query cobble:server_smoke 1.0

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
        /tellraw @a [{"text":"runtime array len {runtime_values_len}","color":"green"},{"text":" key {map_key}"}]
        /say runtime len ok
        /say runtime json array ok
    pow_base = 3
    pow_exp = 4
    pow_result = pow_base ^ pow_exp
    pow_zero = pow_base ^ (pow_exp - 4)
    if pow_result == 81 and pow_zero == 1:
        /say runtime power ok
    compound_score = 10
    compound_score += 5
    compound_score -= 3
    compound_score *= 2
    compound_score /= 4
    compound_score %= 5
    compound_base = 2
    compound_base ^= pow_exp
    if compound_score == 1 and compound_base == 16:
        /say compound assignment ok

    /data get storage cobble:server_smoke status
    /return run say return command ok

stdlib.addEventListener(event.LOAD, init)
"##,
    )
    .unwrap();
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
    wait_for_log(child, &log_path, "Cobble quiet load ok", COMMAND_TIMEOUT)?;
    wait_for_log(child, &log_path, "predicate ok", COMMAND_TIMEOUT)?;

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
    wait_for_log(child, &log_path, "return command ok", COMMAND_TIMEOUT)?;

    run_showcase_selftest(child, &log_path)?;

    send_command(child, "reload")?;
    wait_for_log_count(child, &log_path, "Cobble quiet load ok", 2, COMMAND_TIMEOUT)?;

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
/// `PASS <label>` or `FAIL <label> expected <x> got <y>`; the totals use
/// runtime values, which `/say` turns into `tellraw`, so they are read back as
/// scores instead.
fn run_showcase_selftest(child: &mut Child, log_path: &Path) -> Result<(), String> {
    send_command(child, &format!("function {SHOWCASE_NAMESPACE}:selftest"))?;
    // The last check is scheduled 2 ticks out; a cleared schedule would
    // report a failure 4 ticks out, so give it time to show up.
    wait_for_log(child, log_path, "PASS schedule_once", COMMAND_TIMEOUT)?;
    thread::sleep(Duration::from_millis(500));

    send_command(child, "scoreboard players get passed temp")?;
    send_command(child, "scoreboard players get failed temp")?;
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
    let passed = temp_score(&log, "passed")?;
    let failed = temp_score(&log, "failed")?;
    if failed != 0 || passed == 0 || passed != pass_lines {
        return Err(format!(
            "showcase self-test totals are off: passed={passed} failed={failed}, {pass_lines} PASS lines"
        ));
    }
    eprintln!("showcase self-test: {passed} passed, 0 failed");
    Ok(())
}

fn temp_score(log: &str, holder: &str) -> Result<usize, String> {
    let needle = format!("{holder} has ");
    log.lines()
        .rev()
        .find_map(|line| {
            let (_, rest) = line.split_once(&needle)?;
            rest.strip_suffix(" [temp]")?.parse().ok()
        })
        .ok_or_else(|| format!("no `{holder}` score in the server log"))
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
