use std::fs;
use std::io::Write;
use std::net::TcpListener;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::thread;
use std::time::{Duration, Instant};

const MINECRAFT_VERSION: &str = "26.1.2";
const PURPUR_BUILD: &str = "2589";
const NAMESPACE: &str = "cobble_server_smoke";
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
    let datapack_dir = server_dir.join("world/datapacks").join(NAMESPACE);

    write_project(&project_dir);
    fs::create_dir_all(&datapack_dir).unwrap();
    build_datapack(&repo_root, &project_dir, &datapack_dir);

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
        src_dir.join("main.cbl"),
        r#"import stdlib
from stdlib import event

counter = 0

def init():
    /say Cobble quiet load ok
    /data modify storage cobble:server_smoke status set value "loaded"
    /version
    /waypoint list
    /stopwatch create cobble:server_smoke
    /stopwatch query cobble:server_smoke 1.0

def probe():
    global counter
    counter = counter + 1

    if counter >= 1:
        /say probe counter {counter}

    match counter:
        case 1:
            /say probe first
        case _:
            /say probe later

    for i in range(2):
        /say probe loop {i}

    /data get storage cobble:server_smoke status
    /return run say return command ok

stdlib.addEventListener(event.LOAD, init)
"#,
    )
    .unwrap();
}

fn build_datapack(repo_root: &Path, project_dir: &Path, datapack_dir: &Path) {
    let commands_json = repo_root.join("data/commands.json");
    cobble::commands::build::build(cobble::commands::build::BuildOptions {
        input: Some(project_dir.join("src/main.cbl")),
        output: Some(datapack_dir.to_path_buf()),
        namespace: Some(NAMESPACE.to_string()),
        pack_format: Some("101.1".to_string()),
        description: Some("Cobble Purpur server smoke test".to_string()),
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

    send_command(child, "datapack list")?;
    wait_for_log(
        child,
        &log_path,
        &format!("file/{NAMESPACE}"),
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
    wait_for_log(child, &log_path, "return command ok", COMMAND_TIMEOUT)?;

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
