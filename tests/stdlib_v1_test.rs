use std::fs;
use std::path::{Path, PathBuf};

fn compile_source(source: &str) -> Result<(tempfile::TempDir, PathBuf), String> {
    let temp_dir = tempfile::TempDir::new().unwrap();
    let input_file = temp_dir.path().join("main.cbl");
    let output_dir = temp_dir.path().join("output");
    fs::write(&input_file, source).unwrap();

    cobble::commands::build::build(cobble::commands::build::BuildOptions {
        input: Some(input_file),
        output: Some(output_dir.clone()),
        namespace: Some("stdlib_v1".to_string()),
        pack_format: None,
        description: None,
        verbose: false,
        quiet: false,
        zip: false,
        validate: false,
        dry_run: false,
        commands_json: PathBuf::from("data/commands.json"),
    })?;

    Ok((temp_dir, output_dir))
}

fn read_function(output_dir: &Path, name: &str) -> String {
    fs::read_to_string(
        output_dir
            .join("data/stdlib_v1/function")
            .join(format!("{}.mcfunction", name)),
    )
    .unwrap()
}

#[test]
fn stdlib_v1_generates_common_helpers() {
    let (_temp, output_dir) = compile_source(
        r#"
def use_stdlib():
    text.tellraw("@a", {"text": "Hello", "color": "green"})
    text.tellraw("@a", text.plain("Plain"))
    text.tellraw("@a", text.colored("Gold", "gold"))
    text.tellraw("@a", text.score("@s", "points"))
    text.tellraw("@a", text.selector("@p"))
    text.title("@a", "Ready")
    text.subtitle("@a", {"text": "Go", "bold": True})
    text.actionbar("@a", "Running")
    score.set("points", 10)
    score.add("points", 5)
    score.remove("points", 2)
    score.copy("backup", "points")
    score.operation("points", "+=", "backup")
    random.int("roll", 1, 6)
    random.bool("coin")
    timer.set("cooldown", 20)
    timer.tick("cooldown")
    timer.done("cooldown")
    timer.reset("cooldown")
    storage.set("status", {"ready": True, "count": 3})
    storage.merge("status", {"extra": "ok"})
    storage.copy("status_copy", "status")
    storage.remove("status.extra")
"#,
    )
    .unwrap();

    let content = read_function(&output_dir, "use_stdlib");
    assert!(content.contains(r#"tellraw @a {"color":"green","text":"Hello"}"#));
    assert!(content.contains(r#"tellraw @a {"text":"Plain"}"#));
    assert!(content.contains(r#"tellraw @a {"color":"gold","text":"Gold"}"#));
    assert!(content.contains(r#"tellraw @a {"score":{"name":"@s","objective":"points"}}"#));
    assert!(content.contains(r#"tellraw @a {"selector":"@p"}"#));
    assert!(content.contains(r#"title @a title {"text":"Ready"}"#));
    assert!(content.contains(r#"title @a subtitle {"bold":true,"text":"Go"}"#));
    assert!(content.contains(r#"title @a actionbar {"text":"Running"}"#));
    assert!(content.contains("scoreboard players set points temp 10"));
    assert!(content.contains("scoreboard players add points temp 5"));
    assert!(content.contains("scoreboard players remove points temp 2"));
    assert!(content.contains("scoreboard players operation backup temp = points temp"));
    assert!(content.contains("scoreboard players operation points temp += backup temp"));
    assert!(content.contains("execute store result score roll temp run random value 1..6"));
    assert!(content.contains("execute store result score coin temp run random value 0..1"));
    assert!(content.contains("scoreboard players set cooldown temp 20"));
    assert!(content.contains(
        "execute if score cooldown temp matches 1.. run scoreboard players remove cooldown temp 1"
    ));
    assert!(content.contains("scoreboard players set cooldown_done temp 0"));
    assert!(content.contains(
        "execute if score cooldown temp matches ..0 run scoreboard players set cooldown_done temp 1"
    ));
    assert!(content.contains("scoreboard players reset cooldown temp"));
    assert!(content
        .contains("data modify storage stdlib_v1:global status set value {ready:1b,count:3}"));
    assert!(
        content.contains("data modify storage stdlib_v1:global status merge value {extra:\"ok\"}")
    );
    assert!(content.contains(
        "data modify storage stdlib_v1:global status_copy set from storage stdlib_v1:global status"
    ));
    assert!(content.contains("data remove storage stdlib_v1:global status.extra"));
}

#[test]
fn stdlib_v1_1_generates_project_workflow_helpers() {
    let (_temp, output_dir) = compile_source(
        r#"
def helpers():
    score.objective.add("points", "dummy", "Points")
    score.objective.display("sidebar", "points")
    score.objective.remove("old_points")
    storage.append("items", "diamond")
    storage.prepend("items", "emerald")
    storage.insert("items", 1, "gold_ingot")
    storage.get("items")
    storage.read_score("item_count", "items", 1)
    storage.copy_from("health", "entity", "@s", "Health")
    schedule.once("helpers", "5s", "replace")
    schedule.clear("helpers")
    bossbar.add("timer", "Timer")
    bossbar.set_max("timer", 200)
    bossbar.set_value("timer", 50)
    bossbar.set_name("timer", {"text": "Timer", "color": "gold"})
    bossbar.set_color("timer", "yellow")
    bossbar.set_style("timer", "progress")
    bossbar.set_visible("timer", True)
    bossbar.set_players("timer", "@a")
    team.add("runners", "Runners")
    team.join("runners", "@a")
    team.modify("runners", "color", "green")
    team.modify("runners", "prefix", "[Run] ")
    team.leave("@a")
    entity.tag_add("@s", "runner")
    entity.tag_remove("@s", "runner")
    entity.effect_give("@s", "minecraft:speed", 10, 1, True)
    entity.effect_clear("@s", "minecraft:speed")
    entity.attribute_get("@s", "minecraft:max_health", 1)
    entity.attribute_base_set("@s", "minecraft:max_health", 20)
"#,
    )
    .unwrap();

    let content = read_function(&output_dir, "helpers");
    assert!(content.contains(r#"scoreboard objectives add points dummy {"text":"Points"}"#));
    assert!(content.contains("scoreboard objectives setdisplay sidebar points"));
    assert!(content.contains("scoreboard objectives remove old_points"));
    assert!(
        content.contains(r#"data modify storage stdlib_v1:global items append value "diamond""#)
    );
    assert!(
        content.contains(r#"data modify storage stdlib_v1:global items prepend value "emerald""#)
    );
    assert!(content
        .contains(r#"data modify storage stdlib_v1:global items insert 1 value "gold_ingot""#));
    assert!(content.contains("data get storage stdlib_v1:global items"));
    assert!(content.contains(
        "execute store result score item_count temp run data get storage stdlib_v1:global items 1"
    ));
    assert!(
        content.contains("data modify storage stdlib_v1:global health set from entity @s Health")
    );
    assert!(content.contains("schedule function stdlib_v1:helpers 5s replace"));
    assert!(content.contains("schedule clear stdlib_v1:helpers"));
    assert!(content.contains(r#"bossbar add stdlib_v1:timer {"text":"Timer"}"#));
    assert!(content.contains("bossbar set stdlib_v1:timer max 200"));
    assert!(content.contains("bossbar set stdlib_v1:timer value 50"));
    assert!(content.contains(r#"bossbar set stdlib_v1:timer name {"color":"gold","text":"Timer"}"#));
    assert!(content.contains("bossbar set stdlib_v1:timer color yellow"));
    assert!(content.contains("bossbar set stdlib_v1:timer style progress"));
    assert!(content.contains("bossbar set stdlib_v1:timer visible true"));
    assert!(content.contains("bossbar set stdlib_v1:timer players @a"));
    assert!(content.contains(r#"team add runners {"text":"Runners"}"#));
    assert!(content.contains("team join runners @a"));
    assert!(content.contains("team modify runners color green"));
    assert!(content.contains(r#"team modify runners prefix {"text":"[Run] "}"#));
    assert!(content.contains("team leave @a"));
    assert!(content.contains("tag @s add runner"));
    assert!(content.contains("tag @s remove runner"));
    assert!(content.contains("effect give @s minecraft:speed 10 1 true"));
    assert!(content.contains("effect clear @s minecraft:speed"));
    assert!(content.contains("attribute @s minecraft:max_health get 1"));
    assert!(content.contains("attribute @s minecraft:max_health base set 20"));
}

#[test]
fn stdlib_bossbar_helpers_reject_invalid_enum_values() {
    let color_error = compile_source(
        r#"
def bad_color():
    bossbar.set_color("timer", "orange")
"#,
    )
    .unwrap_err();
    assert!(color_error.contains("Invalid bossbar color: orange"));

    let style_error = compile_source(
        r#"
def bad_style():
    bossbar.set_style("timer", "notched_7")
"#,
    )
    .unwrap_err();
    assert!(style_error.contains("Invalid bossbar style: notched_7"));
}

#[test]
fn text_component_helpers_validate_color_and_must_be_used_as_values() {
    let color_error = compile_source(
        r#"
def bad_color():
    text.tellraw("@a", text.colored("Oops", "orange"))
"#,
    )
    .unwrap_err();
    assert!(color_error.contains("Invalid text color 'orange'"));

    let standalone_error = compile_source(
        r#"
def standalone():
    text.plain("Only a component")
"#,
    )
    .unwrap_err();
    assert!(standalone_error.contains("unsupported-function-call-expression"));
    assert!(standalone_error
        .contains("`text.plain` returns a value and cannot be used as a standalone statement"));
}

#[test]
fn math_sqrt_no_longer_emits_placeholder_runtime_warning() {
    let (_temp, output_dir) = compile_source(
        r#"
def use_math():
    root = math.sqrt(100)
    lower = math.min(3, 4)
"#,
    )
    .unwrap();

    let helper = read_function(&output_dir, "_cobble_math_sqrt");
    assert!(!helper.contains("implementation placeholder"));
    assert!(helper.contains("scoreboard players operation #sqrt_square math *= #sqrt_mid math"));

    let content = read_function(&output_dir, "use_math");
    assert!(content.contains("scoreboard players set #math_input math 100"));
    assert!(!content.contains("scoreboard players set #math_input temp 100"));
    assert!(content.contains("function stdlib_v1:_cobble_math_sqrt"));
    assert!(content.contains("scoreboard players operation root temp = #math_result math"));
    assert!(content.contains("scoreboard players set #math_input math 3"));
    assert!(content.contains("scoreboard players set #math_input2 math 4"));
    assert!(content.contains("function stdlib_v1:_cobble_math_min"));
    assert!(content.contains("scoreboard players operation lower temp = #math_result math"));
}

#[test]
fn stdlib_v1_output_validates_when_command_tree_is_available() {
    let commands_json = Path::new("data/commands.json");
    if !commands_json.exists() {
        eprintln!("Skipping stdlib validation test: data/commands.json not found");
        return;
    }

    let (_temp, output_dir) = compile_source(
        r#"
def use_stdlib():
    text.tellraw("@a", {"text": "Hello", "color": "green"})
    text.title("@a", "Ready")
    text.subtitle("@a", {"text": "Go", "bold": True})
    text.actionbar("@a", "Running")
    score.set("points", 1)
    score.add("points", 2)
    score.remove("points", 1)
    score.copy("backup", "points")
    score.operation("points", "+=", "backup")
    random.int("roll", 1, 2)
    random.bool("coin")
    timer.set("cooldown", 20)
    timer.tick("points")
    timer.done("cooldown")
    timer.reset("cooldown")
    storage.set("status", {"ready": True})
    storage.merge("status", {"extra": "ok"})
    storage.copy("status_copy", "status")
    storage.remove("status.extra")
    score.objective.add("points", "dummy", "Points")
    score.objective.display("sidebar", "points")
    storage.append("items", "diamond")
    storage.prepend("items", "emerald")
    storage.insert("items", 1, "gold_ingot")
    storage.get("items")
    storage.read_score("item_count", "items", 1)
    storage.copy_from("health", "entity", "@s", "Health")
    schedule.once("use_stdlib", "5s", "replace")
    schedule.clear("use_stdlib")
    bossbar.add("timer", "Timer")
    bossbar.set_max("timer", 200)
    bossbar.set_value("timer", 50)
    bossbar.set_name("timer", {"text": "Timer", "color": "gold"})
    bossbar.set_color("timer", "yellow")
    bossbar.set_style("timer", "progress")
    bossbar.set_visible("timer", True)
    bossbar.set_players("timer", "@a")
    team.add("runners", "Runners")
    team.join("runners", "@a")
    team.modify("runners", "color", "green")
    team.modify("runners", "prefix", "[Run] ")
    team.leave("@a")
    entity.tag_add("@s", "runner")
    entity.tag_remove("@s", "runner")
    entity.effect_give("@s", "minecraft:speed", 10, 1, True)
    entity.effect_clear("@s", "minecraft:speed")
    entity.attribute_get("@s", "minecraft:max_health", 1)
    entity.attribute_base_set("@s", "minecraft:max_health", 20)
"#,
    )
    .unwrap();

    let validator = cobble::validator::CommandValidator::from_file(commands_json).unwrap();
    let report = validator.validate_datapack(&output_dir);
    assert!(
        report.errors.is_empty(),
        "Validation errors: {:?}",
        report.errors
    );
}
