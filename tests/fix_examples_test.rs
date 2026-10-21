//! Regression tests for gaps found while writing the `lobby`, `parkour`, and
//! `magic_items` example packs:
//! - a `{var}` inside a styled text component lost the component's styling
//! - a `:` inside SNBT (`{flag:"on"}`) ended an execute header early
//! - `{CONST}`, `{param}`, and selector aliases were left verbatim inside
//!   execute headers (`as @a[distance=..{R}]`, `if entity @Spawn`)
//! - alias expansion matched name prefixes (`@Spawn` inside `@SpawnPoint`)
//! - tag values could not reference other tags (`#minecraft:skeletons`)
//! - literal-only comparison operands (`step - 1` in an unrolled loop) were
//!   computed at runtime
//! - there was no way to declare dialog tags

mod common;

use common::mcsim::{compile, load_functions, Sim};
use std::fs;

#[test]
fn styled_text_components_keep_their_style_for_variables() {
    let mut sim = Sim::load(
        r#"
def main():
    secs = 7
    /tellraw @a {"text":"Ready in {secs}s","color":"red","bold":true}
"#,
        &[],
    );
    sim.call("main");
    assert_eq!(
        sim.function_text("main").lines().last().unwrap(),
        r#"tellraw @a {"bold":true,"color":"red","extra":[{"score":{"name":"secs","objective":"temp"}},{"text":"s"}],"text":"Ready in "}"#
    );
}

#[test]
fn snbt_strings_stay_inside_execute_headers() {
    let (_temp, output_dir) = compile(
        r#"
def main():
    if items entity @s weapon.mainhand *[minecraft:custom_data~{magic_item:"blink"}]:
        /say blink
    if data storage sim:global {flag: "on"} as @a:
        /say flag
"#,
    )
    .unwrap_or_else(|error| panic!("build failed: {error}"));
    assert_eq!(
        load_functions(&output_dir)["main"],
        [
            r#"execute if items entity @s weapon.mainhand *[minecraft:custom_data~{magic_item:"blink"}] run say blink"#,
            r#"execute if data storage sim:global {flag: "on"} as @a run say flag"#,
        ]
    );
}

#[test]
fn execute_headers_resolve_constants_params_and_aliases() {
    let (_temp, output_dir) = compile(
        r#"
const RADIUS = 24
@Spawn = @e[type=minecraft:marker,tag=spawn,limit=1]
@SpawnPoint = @e[type=minecraft:marker,tag=spawn_point,limit=1]

def place(x, team):
    positioned {x} 100 0 as @a[team={team}]:
        /setblock ~ ~ ~ minecraft:stone

def main():
    at @Spawn as @a[distance=..{RADIUS}]:
        /effect give @s minecraft:regeneration 3 0 true
    if entity @Spawn unless entity @SpawnPoint:
        /tp @s @Spawn
"#,
    )
    .unwrap_or_else(|error| panic!("build failed: {error}"));
    let functions = load_functions(&output_dir);
    assert_eq!(
        functions["main"],
        [
            "execute at @e[type=minecraft:marker,tag=spawn,limit=1] as @a[distance=..24] run effect give @s minecraft:regeneration 3 0 true",
            "execute if entity @e[type=minecraft:marker,tag=spawn,limit=1] unless entity @e[type=minecraft:marker,tag=spawn_point,limit=1] run tp @s @e[type=minecraft:marker,tag=spawn,limit=1]",
        ]
    );
    assert_eq!(
        functions["place"],
        ["$execute positioned $(x) 100 0 as @a[team=$(team)] run setblock ~ ~ ~ minecraft:stone"]
    );
}

#[test]
fn tag_values_can_reference_other_tags() {
    let (_temp, output_dir) = compile(
        r##"
from stdlib import datapack

datapack.entity_type_tag("targets", ["#minecraft:skeletons", "minecraft:creeper"])
datapack.block_tag("passable", ["#minecraft:air", {"id": "#minecraft:small_flowers", "required": False}])
"##,
    )
    .unwrap_or_else(|error| panic!("build failed: {error}"));
    let targets =
        fs::read_to_string(output_dir.join("data/sim/tags/entity_type/targets.json")).unwrap();
    assert!(targets.contains(r##""#minecraft:skeletons""##), "{targets}");
    let passable =
        fs::read_to_string(output_dir.join("data/sim/tags/block/passable.json")).unwrap();
    assert!(
        passable.contains(r##""#minecraft:small_flowers""##),
        "{passable}"
    );

    let error =
        compile("from stdlib import datapack\ndatapack.item_tag(\"bad\", [\"#Minecraft:logs\"])\n")
            .unwrap_err();
    assert!(error.contains("datapack-resource-id"), "{error}");
}

#[test]
fn literal_comparison_operands_fold_at_compile_time() {
    let (_temp, output_dir) = compile(
        r#"
def main():
    reach = 0
    for step in range(1, 4):
        if reach == step - 1:
            reach = step
"#,
    )
    .unwrap_or_else(|error| panic!("build failed: {error}"));
    assert_eq!(
        load_functions(&output_dir)["main"],
        [
            "scoreboard players set reach temp 0",
            "execute if score reach temp matches 0 run scoreboard players set reach temp 1",
            "execute if score reach temp matches 1 run scoreboard players set reach temp 2",
            "execute if score reach temp matches 2 run scoreboard players set reach temp 3",
        ]
    );
}

#[test]
fn dialog_tags_are_declared_like_other_tags() {
    let (_temp, output_dir) = compile(
        r#"
from stdlib import datapack

datapack.dialog("menu", {"type": "minecraft:notice", "title": {"text": "Menu"}})
datapack.dialog_tag("minecraft:pause_screen_additions", ["sim:menu"])
"#,
    )
    .unwrap_or_else(|error| panic!("build failed: {error}"));
    let tag = fs::read_to_string(
        output_dir.join("data/minecraft/tags/dialog/pause_screen_additions.json"),
    )
    .unwrap();
    assert!(tag.contains(r#""sim:menu""#), "{tag}");
}
