use std::fs;
use std::path::PathBuf;

use cobble::transpiler::DataPack;

fn compile_source(source: &str) -> Result<(tempfile::TempDir, PathBuf), String> {
    let temp_dir = tempfile::TempDir::new().unwrap();
    let input_file = temp_dir.path().join("main.cbl");
    let output_dir = temp_dir.path().join("output");
    fs::write(&input_file, source).unwrap();

    cobble::commands::build::build(cobble::commands::build::BuildOptions {
        input: Some(input_file),
        output: Some(output_dir.clone()),
        namespace: Some("resources".to_string()),
        pack_format: None,
        description: None,
        verbose: false,
        zip: false,
        validate: false,
        commands_json: PathBuf::from("data/commands.json"),
    })?;

    Ok((temp_dir, output_dir))
}

#[test]
fn datapack_resource_declarations_write_modern_json_layout() {
    let (_temp, output_dir) = compile_source(
        r##"
datapack.function_tag("utility", ["resources:setup"])
datapack.block_tag("solid_blocks", ["minecraft:stone"])
datapack.item_tag("reward_items", ["minecraft:diamond"])
datapack.entity_type_tag("targets", ["minecraft:zombie"])
datapack.predicate("is_sneaking", {
    "condition": "minecraft:entity_properties",
    "entity": "this",
    "predicate": {"flags": {"is_sneaking": True}}
})
datapack.advancement("root", {"criteria": {"tick": {"trigger": "minecraft:tick"}}})
datapack.loot_table("empty", {"type": "minecraft:empty"})
datapack.recipe("stonecutting/test", {
    "type": "minecraft:stonecutting",
    "ingredient": "minecraft:stone",
    "result": {"id": "minecraft:stone"}
})
datapack.item_modifier("set_name", {"function": "minecraft:set_name", "name": "Test"})
datapack.dialog("notice", {"type": "minecraft:notice", "title": {"text": "Notice"}})

def setup():
    /say setup
"##,
    )
    .unwrap();

    let namespace_dir = output_dir.join("data/resources");
    assert!(namespace_dir.join("tags/function/utility.json").exists());
    assert!(namespace_dir.join("tags/block/solid_blocks.json").exists());
    assert!(namespace_dir.join("tags/item/reward_items.json").exists());
    assert!(namespace_dir.join("tags/entity_type/targets.json").exists());
    assert!(namespace_dir.join("predicate/is_sneaking.json").exists());
    assert!(namespace_dir.join("advancement/root.json").exists());
    assert!(namespace_dir.join("loot_table/empty.json").exists());
    assert!(namespace_dir.join("recipe/stonecutting/test.json").exists());
    assert!(namespace_dir.join("item_modifier/set_name.json").exists());
    assert!(namespace_dir.join("dialog/notice.json").exists());

    let predicate = fs::read_to_string(namespace_dir.join("predicate/is_sneaking.json")).unwrap();
    assert!(predicate.contains(r#""condition": "minecraft:entity_properties""#));
    assert!(predicate.contains(r#""is_sneaking": true"#));

    let tag = fs::read_to_string(namespace_dir.join("tags/function/utility.json")).unwrap();
    assert!(tag.contains(r#""values""#));
    assert!(tag.contains(r#""resources:setup""#));
}

#[test]
fn duplicate_datapack_resource_ids_fail() {
    let error = compile_source(
        r#"
datapack.predicate("same", {"condition": "minecraft:random_chance", "chance": 0.5})
datapack.predicate("same", {"condition": "minecraft:random_chance", "chance": 0.25})
"#,
    )
    .unwrap_err();

    assert!(error.contains("Duplicate data pack resource"));
    assert!(error.contains("predicate/same"));
}

#[test]
fn datapack_resource_names_reject_invalid_paths() {
    let error = compile_source(
        r#"
datapack.predicate("Bad/Name", {"condition": "minecraft:random_chance", "chance": 1})
"#,
    )
    .unwrap_err();

    assert!(error.contains("Invalid resource name"));
    assert!(error.contains("lowercase resource paths"));
}

#[test]
fn removed_datapack_resources_do_not_survive_rebuilds() {
    let temp_dir = tempfile::TempDir::new().unwrap();
    let input_file = temp_dir.path().join("main.cbl");
    let output_dir = temp_dir.path().join("output");

    fs::write(
        &input_file,
        r#"
datapack.predicate("old", {"condition": "minecraft:random_chance", "chance": 1})

def setup():
    /say first
"#,
    )
    .unwrap();

    let build_once = || {
        cobble::commands::build::build(cobble::commands::build::BuildOptions {
            input: Some(input_file.clone()),
            output: Some(output_dir.clone()),
            namespace: Some("resources".to_string()),
            pack_format: None,
            description: None,
            verbose: false,
            zip: false,
            validate: false,
            commands_json: PathBuf::from("data/commands.json"),
        })
    };

    build_once().unwrap();
    assert!(output_dir
        .join("data/resources/predicate/old.json")
        .exists());
    assert!(output_dir.join(".cobble/source_map.json").exists());

    fs::create_dir_all(output_dir.join("data/resources/functions")).unwrap();
    fs::write(
        output_dir.join("data/resources/functions/stale.mcfunction"),
        "say stale\n",
    )
    .unwrap();
    fs::create_dir_all(output_dir.join("data/resources/advancements")).unwrap();
    fs::write(
        output_dir.join("data/resources/advancements/stale.json"),
        "{}\n",
    )
    .unwrap();
    fs::create_dir_all(output_dir.join("data/minecraft/tags/functions")).unwrap();
    fs::write(
        output_dir.join("data/minecraft/tags/functions/load.json"),
        "{}\n",
    )
    .unwrap();

    fs::write(
        &input_file,
        r#"
def setup():
    pass
"#,
    )
    .unwrap();

    build_once().unwrap();
    assert!(!output_dir
        .join("data/resources/predicate/old.json")
        .exists());
    assert!(!output_dir.join(".cobble/source_map.json").exists());
    assert!(!output_dir
        .join("data/resources/functions/stale.mcfunction")
        .exists());
    assert!(!output_dir
        .join("data/resources/advancements/stale.json")
        .exists());
    assert!(!output_dir
        .join("data/minecraft/tags/functions/load.json")
        .exists());
}

#[test]
fn namespace_changes_clean_previous_generated_namespace() {
    let temp_dir = tempfile::TempDir::new().unwrap();
    let input_file = temp_dir.path().join("main.cbl");
    let output_dir = temp_dir.path().join("output");
    fs::write(&input_file, "def main():\n    /say hi\n").unwrap();

    for namespace in ["old", "new"] {
        cobble::commands::build::build(cobble::commands::build::BuildOptions {
            input: Some(input_file.clone()),
            output: Some(output_dir.clone()),
            namespace: Some(namespace.to_string()),
            pack_format: None,
            description: None,
            verbose: false,
            zip: false,
            validate: false,
            commands_json: PathBuf::from("data/commands.json"),
        })
        .unwrap();
    }

    assert!(!output_dir
        .join("data/old/function/main.mcfunction")
        .exists());
    assert!(output_dir
        .join("data/new/function/main.mcfunction")
        .exists());
}

#[test]
fn direct_datapack_tags_are_written() {
    let temp_dir = tempfile::TempDir::new().unwrap();
    let output_dir = temp_dir.path().join("output");
    let mut data_pack = DataPack::new("resources".to_string(), output_dir.clone());
    data_pack.add_tag("utility".to_string(), vec!["resources:setup".to_string()]);
    data_pack.add_tag(
        "minecraft:load".to_string(),
        vec!["resources:setup".to_string()],
    );

    data_pack.write().unwrap();

    assert!(output_dir
        .join("data/resources/tags/function/utility.json")
        .exists());
    assert!(output_dir
        .join("data/minecraft/tags/function/load.json")
        .exists());
}
