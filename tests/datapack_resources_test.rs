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
        quiet: false,
        zip: false,
        validate: false,
        dry_run: false,
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
fn build_writes_cobble_manifest_metadata() {
    let (_temp, output_dir) = compile_source(
        r##"
datapack.function_tag("minecraft:load", ["resources:setup"])
datapack.predicate("always", {
    "condition": "minecraft:random_chance",
    "chance": 1
})

def setup():
    /say setup
"##,
    )
    .unwrap();

    let manifest_path = output_dir.join(".cobble/build_manifest.json");
    assert!(manifest_path.exists());
    let manifest: serde_json::Value =
        serde_json::from_str(&fs::read_to_string(manifest_path).unwrap()).unwrap();

    assert_eq!(manifest["version"], 1);
    assert_eq!(manifest["cobble_version"], env!("CARGO_PKG_VERSION"));
    assert_eq!(manifest["minecraft_version"], "26.1.2");
    assert_eq!(manifest["pack_format_text"], "101.1");
    assert_eq!(manifest["namespace"], "resources");
    assert_eq!(
        manifest["input"]["compiled_files"]
            .as_array()
            .unwrap()
            .len(),
        1
    );
    assert_eq!(manifest["generated"]["functions"], 1);
    assert_eq!(manifest["generated"]["commands"], 1);
    assert_eq!(manifest["generated"]["source_map_entries"], 1);
    assert_eq!(manifest["generated"]["json_function_tags"], 1);
    assert_eq!(manifest["generated"]["predicates"], 1);
    assert_eq!(manifest["generated"]["json_resources"], 2);
    assert_eq!(manifest["generated"]["total_json_resources"], 2);
    assert!(manifest["generated_namespaces"]
        .as_array()
        .unwrap()
        .contains(&serde_json::Value::String("minecraft".to_string())));
    let resources = manifest["resources"].as_array().unwrap();
    assert!(resources.iter().any(|resource| {
        resource["kind"] == "function_tag"
            && resource["namespace"] == "minecraft"
            && resource["path"] == "load"
    }));
    assert!(resources.iter().any(|resource| {
        resource["kind"] == "predicate"
            && resource["namespace"] == "resources"
            && resource["path"] == "always"
    }));
    assert!(manifest["validation"].is_null());
}

#[test]
fn datapack_resource_declarations_support_explicit_namespaces() {
    let (_temp, output_dir) = compile_source(
        r#"
datapack.function_tag("minecraft:load", ["resources:setup"])
datapack.predicate("other_ns:checks/is_ready", {
    "condition": "minecraft:random_chance",
    "chance": 1
})

def setup():
    /say setup
"#,
    )
    .unwrap();

    assert!(output_dir
        .join("data/minecraft/tags/function/load.json")
        .exists());
    assert!(output_dir
        .join("data/other_ns/predicate/checks/is_ready.json")
        .exists());
}

#[test]
fn datapack_json_resources_serialize_none_as_json_null() {
    let (_temp, output_dir) = compile_source(
        r#"
datapack.predicate("maybe", {
    "condition": "minecraft:random_chance",
    "chance": 1,
    "comment": None
})

def setup():
    /say setup
"#,
    )
    .unwrap();

    let predicate: serde_json::Value = serde_json::from_str(
        &fs::read_to_string(output_dir.join("data/resources/predicate/maybe.json")).unwrap(),
    )
    .unwrap();

    assert!(predicate["comment"].is_null());
}

#[test]
fn datapack_function_tags_merge_with_stdlib_event_tags() {
    let (_temp, output_dir) = compile_source(
        r#"
import stdlib
from stdlib import event

datapack.function_tag("minecraft:load", ["resources:extra_load"])

def load():
    /say load

def extra_load():
    /say extra

stdlib.addEventListener(event.LOAD, load)
"#,
    )
    .unwrap();

    let tag: serde_json::Value = serde_json::from_str(
        &fs::read_to_string(output_dir.join("data/minecraft/tags/function/load.json")).unwrap(),
    )
    .unwrap();
    let values = tag["values"].as_array().unwrap().clone();

    assert_eq!(
        values,
        vec![
            serde_json::json!("resources:load"),
            serde_json::json!("resources:extra_load")
        ]
    );

    let manifest: serde_json::Value = serde_json::from_str(
        &fs::read_to_string(output_dir.join(".cobble/build_manifest.json")).unwrap(),
    )
    .unwrap();
    let function_tag_resources: Vec<_> = manifest["resources"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|resource| resource["kind"] == "function_tag")
        .collect();

    assert_eq!(manifest["generated"]["function_tags"], 1);
    assert_eq!(function_tag_resources.len(), 1);
    assert_eq!(function_tag_resources[0]["namespace"], "minecraft");
    assert_eq!(function_tag_resources[0]["path"], "load");
}

#[test]
fn datapack_json_resources_require_object_values() {
    let error = compile_source(
        r#"
datapack.predicate("bad", ["not", "an", "object"])
"#,
    )
    .unwrap_err();

    assert!(error.contains("datapack.predicate() JSON value must be an object"));
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
    assert!(error.contains("invalid overwrite"));
    assert!(error.contains("first declaration: main.cbl:2:1"));
    assert!(error.contains("second declaration: main.cbl:3:1"));
}

#[test]
fn duplicate_identical_datapack_resource_ids_report_exact_duplicate() {
    let error = compile_source(
        r#"
datapack.predicate("same", {"condition": "minecraft:random_chance", "chance": 0.5})
datapack.predicate("same", {"condition": "minecraft:random_chance", "chance": 0.5})
"#,
    )
    .unwrap_err();

    assert!(error.contains("Duplicate data pack resource"));
    assert!(error.contains("predicate/same"));
    assert!(error.contains("exact duplicate"));
    assert!(error.contains("first declaration: main.cbl:2:1"));
    assert!(error.contains("second declaration: main.cbl:3:1"));
}

#[test]
fn duplicate_datapack_tags_report_tag_declaration_conflict() {
    let error = compile_source(
        r#"
datapack.function_tag("minecraft:load", ["resources:setup"])
datapack.function_tag("minecraft:load", ["resources:other"])
"#,
    )
    .unwrap_err();

    assert!(error.contains("Duplicate data pack resource"));
    assert!(error.contains("minecraft:tags/function/load"));
    assert!(error.contains("invalid duplicate tag declaration"));
    assert!(error.contains("first declaration: main.cbl:2:1"));
    assert!(error.contains("second declaration: main.cbl:3:1"));
}

#[test]
fn duplicate_datapack_resources_across_imports_fail() {
    let temp_dir = tempfile::TempDir::new().unwrap();
    let source_dir = temp_dir.path().join("src");
    let output_dir = temp_dir.path().join("output");
    fs::create_dir_all(&source_dir).unwrap();
    fs::write(
        source_dir.join("main.cbl"),
        r#"
import extra
datapack.predicate("same", {"condition": "minecraft:random_chance", "chance": 0.5})
"#,
    )
    .unwrap();
    fs::write(
        source_dir.join("extra.cbl"),
        r#"
datapack.predicate("same", {"condition": "minecraft:random_chance", "chance": 0.25})
"#,
    )
    .unwrap();

    let error = cobble::commands::build::build(cobble::commands::build::BuildOptions {
        input: Some(source_dir.join("main.cbl")),
        output: Some(output_dir),
        namespace: Some("resources".to_string()),
        pack_format: None,
        description: None,
        verbose: false,
        quiet: false,
        zip: false,
        validate: false,
        dry_run: false,
        commands_json: PathBuf::from("data/commands.json"),
    })
    .unwrap_err();

    assert!(error.contains("Duplicate data pack resource"));
    assert!(error.contains("predicate/same"));
    assert!(error.contains("invalid overwrite"));
    assert!(error.contains("first declaration: extra.cbl:2:1"));
    assert!(error.contains("second declaration: main.cbl:3:1"));
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
    assert!(error.contains("uppercase character 'B' at position 1"));
    assert!(error.contains("lowercase resource paths"));

    let namespace_error = compile_source(
        r#"
datapack.function_tag("minecraft/load", ["resources:setup"])
"#,
    )
    .unwrap_err();

    assert!(namespace_error.contains("'minecraft' looks like a namespace"));
    assert!(namespace_error.contains("minecraft:load"));

    let tag_value_error = compile_source(
        r#"
datapack.item_tag("rewards", ["minecraft/diamond"])
"#,
    )
    .unwrap_err();

    assert!(tag_value_error.contains("Invalid tag value"));
    assert!(tag_value_error.contains("minecraft:diamond"));

    let non_string_tag_value_error = compile_source(
        r#"
datapack.item_tag("rewards", [1])
"#,
    )
    .unwrap_err();

    assert!(non_string_tag_value_error.contains("Tag values must be string resource IDs"));
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
            quiet: false,
            zip: false,
            validate: false,
            dry_run: false,
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
            quiet: false,
            zip: false,
            validate: false,
            dry_run: false,
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
fn namespace_changes_clean_previous_function_dir_when_namespace_is_still_used_for_resources() {
    let temp_dir = tempfile::TempDir::new().unwrap();
    let input_file = temp_dir.path().join("main.cbl");
    let output_dir = temp_dir.path().join("output");

    fs::write(&input_file, "def main():\n    /say old\n").unwrap();
    cobble::commands::build::build(cobble::commands::build::BuildOptions {
        input: Some(input_file.clone()),
        output: Some(output_dir.clone()),
        namespace: Some("old".to_string()),
        pack_format: None,
        description: None,
        verbose: false,
        quiet: false,
        zip: false,
        validate: false,
        dry_run: false,
        commands_json: PathBuf::from("data/commands.json"),
    })
    .unwrap();

    assert!(output_dir
        .join("data/old/function/main.mcfunction")
        .exists());

    fs::write(
        &input_file,
        r#"
datapack.predicate("old:checks/ready", {
    "condition": "minecraft:random_chance",
    "chance": 1
})

def main():
    /say new
"#,
    )
    .unwrap();

    cobble::commands::build::build(cobble::commands::build::BuildOptions {
        input: Some(input_file),
        output: Some(output_dir.clone()),
        namespace: Some("new".to_string()),
        pack_format: None,
        description: None,
        verbose: false,
        quiet: false,
        zip: false,
        validate: false,
        dry_run: false,
        commands_json: PathBuf::from("data/commands.json"),
    })
    .unwrap();

    assert!(!output_dir
        .join("data/old/function/main.mcfunction")
        .exists());
    assert!(output_dir
        .join("data/old/predicate/checks/ready.json")
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
        "utility/nested".to_string(),
        vec!["resources:nested_setup".to_string()],
    );
    data_pack.add_tag(
        "minecraft:load".to_string(),
        vec!["resources:setup".to_string()],
    );
    data_pack.add_tag(
        "other_ns:utility/nested".to_string(),
        vec!["resources:setup".to_string()],
    );

    data_pack.write().unwrap();

    assert!(output_dir
        .join("data/resources/tags/function/utility.json")
        .exists());
    assert!(output_dir
        .join("data/resources/tags/function/utility/nested.json")
        .exists());
    assert!(output_dir
        .join("data/minecraft/tags/function/load.json")
        .exists());
    assert!(output_dir
        .join("data/other_ns/tags/function/utility/nested.json")
        .exists());
}

#[test]
fn direct_datapack_resource_writers_create_nested_paths() {
    let temp_dir = tempfile::TempDir::new().unwrap();
    let output_dir = temp_dir.path().join("output");
    let mut data_pack = DataPack::new("resources".to_string(), output_dir.clone());

    data_pack.add_advancement("story/root".to_string(), "{}".to_string());
    data_pack.add_loot_table("chests/reward".to_string(), "{}".to_string());
    data_pack.add_recipe("stonecutting/test".to_string(), "{}".to_string());
    data_pack.add_predicate("checks/ready".to_string(), "{}".to_string());
    data_pack.add_item_modifier("items/set_name".to_string(), "{}".to_string());

    data_pack.write().unwrap();

    let namespace_dir = output_dir.join("data/resources");
    assert!(namespace_dir.join("advancement/story/root.json").exists());
    assert!(namespace_dir.join("loot_table/chests/reward.json").exists());
    assert!(namespace_dir.join("recipe/stonecutting/test.json").exists());
    assert!(namespace_dir.join("predicate/checks/ready.json").exists());
    assert!(namespace_dir
        .join("item_modifier/items/set_name.json")
        .exists());
}

#[test]
fn direct_datapack_writer_rejects_traversal_namespace_without_deleting_functions() {
    let temp_dir = tempfile::TempDir::new().unwrap();
    let output_dir = temp_dir.path().join("project");
    let victim_function_dir = temp_dir.path().join("victim").join("function");
    fs::create_dir_all(output_dir.join("data")).unwrap();
    fs::create_dir_all(&victim_function_dir).unwrap();
    fs::write(
        victim_function_dir.join("important.mcfunction"),
        "say keep\n",
    )
    .unwrap();

    let mut data_pack = DataPack::new("../../victim".to_string(), output_dir);
    data_pack.add_function("main".to_string(), vec!["say generated".to_string()]);

    let error = data_pack.write().unwrap_err();

    assert!(error.to_string().contains("Invalid data pack namespace"));
    assert_eq!(
        fs::read_to_string(victim_function_dir.join("important.mcfunction")).unwrap(),
        "say keep\n"
    );
}

#[test]
fn direct_datapack_writer_rejects_traversal_json_resource_namespace_without_cleanup() {
    let temp_dir = tempfile::TempDir::new().unwrap();
    let output_dir = temp_dir.path().join("project");
    let victim_predicate_dir = temp_dir.path().join("victim").join("predicate");
    fs::create_dir_all(output_dir.join("data")).unwrap();
    fs::create_dir_all(&victim_predicate_dir).unwrap();
    fs::write(victim_predicate_dir.join("important.json"), "{}\n").unwrap();

    let mut data_pack = DataPack::new("resources".to_string(), output_dir);
    data_pack
        .add_json_resource_in_namespace(
            "../../victim".to_string(),
            "predicate/generated".to_string(),
            "{}".to_string(),
        )
        .unwrap();

    let error = data_pack.write().unwrap_err();

    assert!(error
        .to_string()
        .contains("Invalid JSON resource namespace"));
    assert_eq!(
        fs::read_to_string(victim_predicate_dir.join("important.json")).unwrap(),
        "{}\n"
    );
}

#[test]
fn direct_datapack_writer_rejects_traversal_tag_paths() {
    let temp_dir = tempfile::TempDir::new().unwrap();
    let output_dir = temp_dir.path().join("output");
    let mut data_pack = DataPack::new("resources".to_string(), output_dir);
    data_pack.add_tag("../victim".to_string(), vec!["resources:setup".to_string()]);

    let error = data_pack.write().unwrap_err();

    assert!(error.to_string().contains("Invalid function tag path"));
}
