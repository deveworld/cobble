use std::fs;
#[cfg(unix)]
use std::os::unix::fs::PermissionsExt;
use std::path::PathBuf;

use cobble::transpiler::DataPack;

fn compile_source(source: &str) -> Result<(tempfile::TempDir, PathBuf), String> {
    compile_source_with_options(source, false, false)
}

fn compile_resource_pack_source(source: &str) -> Result<(tempfile::TempDir, PathBuf), String> {
    compile_source_with_options(source, true, false)
}

fn compile_source_with_options(
    source: &str,
    experimental_resource_pack: bool,
    zip: bool,
) -> Result<(tempfile::TempDir, PathBuf), String> {
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
        zip,
        experimental_resource_pack,
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
import stdlib
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
import stdlib
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
    assert_eq!(manifest["generated"]["source_map_entries"], 3);
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
import stdlib
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
import stdlib
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
fn datapack_function_tag_replace_survives_stdlib_event_tag_merge() {
    let (_temp, output_dir) = compile_source(
        r#"
import stdlib
from stdlib import event

datapack.function_tag("minecraft:load", ["resources:extra_load"], True)

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

    assert_eq!(tag["replace"], true);
    assert_eq!(
        tag["values"],
        serde_json::json!(["resources:load", "resources:extra_load"])
    );
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
import stdlib
datapack.predicate("same", {"condition": "minecraft:random_chance", "chance": 0.5})
datapack.predicate("same", {"condition": "minecraft:random_chance", "chance": 0.25})
"#,
    )
    .unwrap_err();

    assert!(error.contains("Duplicate data pack resource"));
    assert!(error.contains("predicate/same"));
    assert!(error.contains("invalid overwrite"));
    assert!(error.contains("first declaration: main.cbl:3:1"));
    assert!(error.contains("second declaration: main.cbl:4:1"));
}

#[test]
fn duplicate_identical_datapack_resource_ids_are_accepted_once() {
    let (_temp, output_dir) = compile_source(
        r#"
import stdlib
datapack.predicate("same", {"condition": "minecraft:random_chance", "chance": 0.5})
datapack.predicate("same", {"condition": "minecraft:random_chance", "chance": 0.5})
"#,
    )
    .unwrap();

    let predicate_path = output_dir.join("data/resources/predicate/same.json");
    assert!(predicate_path.exists());
    let manifest: serde_json::Value = serde_json::from_str(
        &fs::read_to_string(output_dir.join(".cobble/build_manifest.json")).unwrap(),
    )
    .unwrap();
    let predicate_resources: Vec<_> = manifest["resources"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|resource| resource["kind"] == "predicate" && resource["path"] == "same")
        .collect();
    assert_eq!(predicate_resources.len(), 1);
}

#[test]
fn duplicate_datapack_tags_are_merged_deterministically() {
    let (_temp, output_dir) = compile_source(
        r#"
import stdlib
datapack.function_tag("minecraft:load", ["resources:setup"])
datapack.function_tag("minecraft:load", ["resources:other"])
"#,
    )
    .unwrap();

    let tag_path = output_dir.join("data/minecraft/tags/function/load.json");
    let tag: serde_json::Value =
        serde_json::from_str(&fs::read_to_string(&tag_path).unwrap()).unwrap();
    let values = tag["values"].as_array().unwrap();
    assert_eq!(values.len(), 2);
    // Values are sorted lexicographically.
    assert_eq!(values[0], "resources:other");
    assert_eq!(values[1], "resources:setup");
}

#[test]
fn duplicate_datapack_tags_dedup_identical_values() {
    let (_temp, output_dir) = compile_source(
        r#"
import stdlib
datapack.function_tag("utility", ["resources:setup", "resources:tick"])
datapack.function_tag("utility", ["resources:tick", "resources:init"])
"#,
    )
    .unwrap();

    let tag_path = output_dir.join("data/resources/tags/function/utility.json");
    let tag: serde_json::Value =
        serde_json::from_str(&fs::read_to_string(&tag_path).unwrap()).unwrap();
    let values = tag["values"].as_array().unwrap();
    assert_eq!(values.len(), 3);
    assert_eq!(values[0], "resources:init");
    assert_eq!(values[1], "resources:setup");
    assert_eq!(values[2], "resources:tick");
}

#[test]
fn datapack_tag_replace_argument_merges_with_true_wins_semantics() {
    let (_temp, output_dir) = compile_source(
        r#"
import stdlib
datapack.item_tag("rewards", ["minecraft:diamond"], False)
datapack.item_tag("rewards", ["minecraft:emerald"], True)
"#,
    )
    .unwrap();

    let tag_path = output_dir.join("data/resources/tags/item/rewards.json");
    let tag: serde_json::Value =
        serde_json::from_str(&fs::read_to_string(&tag_path).unwrap()).unwrap();
    assert_eq!(tag["replace"], true);
    assert_eq!(
        tag["values"].as_array().unwrap(),
        &vec![
            serde_json::json!("minecraft:diamond"),
            serde_json::json!("minecraft:emerald"),
        ]
    );
}

#[test]
fn datapack_tag_replace_false_is_preserved_when_supplied() {
    let (_temp, output_dir) = compile_source(
        r#"
import stdlib
datapack.block_tag("solid", ["minecraft:stone"], False)
"#,
    )
    .unwrap();

    let tag_path = output_dir.join("data/resources/tags/block/solid.json");
    let tag: serde_json::Value =
        serde_json::from_str(&fs::read_to_string(&tag_path).unwrap()).unwrap();
    assert_eq!(tag["replace"], false);
    assert_eq!(
        tag["values"].as_array().unwrap(),
        &vec![serde_json::json!("minecraft:stone")]
    );
}

#[test]
fn datapack_tag_same_id_with_different_required_values_are_preserved() {
    let (_temp, output_dir) = compile_source(
        r#"
import stdlib
datapack.item_tag("rewards", [
    "minecraft:diamond",
    {"id": "minecraft:diamond", "required": False},
])
"#,
    )
    .unwrap();

    let tag_path = output_dir.join("data/resources/tags/item/rewards.json");
    let tag: serde_json::Value =
        serde_json::from_str(&fs::read_to_string(&tag_path).unwrap()).unwrap();
    let values = tag["values"].as_array().unwrap();

    assert_eq!(
        values,
        &vec![
            serde_json::json!("minecraft:diamond"),
            serde_json::json!({"id": "minecraft:diamond", "required": false}),
        ]
    );
}

#[test]
fn datapack_tag_duplicate_object_entries_with_same_id_and_required_dedupe() {
    let (_temp, output_dir) = compile_source(
        r#"
import stdlib
datapack.item_tag("rewards", [
    {"id": "minecraft:diamond", "required": False},
    {"id": "minecraft:diamond", "required": False},
])
datapack.item_tag("rewards", [
    {"id": "minecraft:diamond", "required": False},
    {"id": "minecraft:emerald", "required": True},
    "minecraft:emerald",
])
"#,
    )
    .unwrap();

    let tag_path = output_dir.join("data/resources/tags/item/rewards.json");
    let tag: serde_json::Value =
        serde_json::from_str(&fs::read_to_string(&tag_path).unwrap()).unwrap();
    let values = tag["values"].as_array().unwrap();

    assert_eq!(
        values,
        &vec![
            serde_json::json!({"id": "minecraft:diamond", "required": false}),
            serde_json::json!("minecraft:emerald"),
        ]
    );
}

#[test]
fn datapack_tag_mixed_string_and_object_entries_sort_deterministically() {
    let (_temp, output_dir) = compile_source(
        r#"
import stdlib
datapack.item_tag("rewards", [
    {"id": "minecraft:gold_ingot", "required": False},
    "minecraft:emerald",
])
datapack.item_tag("rewards", [
    {"id": "minecraft:diamond", "required": False},
    "minecraft:gold_ingot",
    "minecraft:diamond",
])
"#,
    )
    .unwrap();

    let tag_path = output_dir.join("data/resources/tags/item/rewards.json");
    let tag: serde_json::Value =
        serde_json::from_str(&fs::read_to_string(&tag_path).unwrap()).unwrap();
    let values = tag["values"].as_array().unwrap();

    assert_eq!(
        values,
        &vec![
            serde_json::json!("minecraft:diamond"),
            serde_json::json!({"id": "minecraft:diamond", "required": false}),
            serde_json::json!("minecraft:emerald"),
            serde_json::json!("minecraft:gold_ingot"),
            serde_json::json!({"id": "minecraft:gold_ingot", "required": false}),
        ]
    );
}

#[test]
fn datapack_tag_object_entries_reject_invalid_shapes() {
    let missing_id_error = compile_source(
        r#"
import stdlib
datapack.item_tag("rewards", [{"required": False}])
"#,
    )
    .unwrap_err();
    assert!(missing_id_error.contains("Tag object values must include a string \"id\""));

    let non_boolean_required_error = compile_source(
        r#"
import stdlib
datapack.item_tag("rewards", [{"id": "minecraft:diamond", "required": "no"}])
"#,
    )
    .unwrap_err();
    assert!(non_boolean_required_error.contains("Tag object value \"required\" must be a boolean"));

    let extra_field_error = compile_source(
        r#"
import stdlib
datapack.item_tag("rewards", [{"id": "minecraft:diamond", "optional": False}])
"#,
    )
    .unwrap_err();
    assert!(extra_field_error.contains("may only contain \"id\" and \"required\" fields"));
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
import stdlib
import extra
datapack.predicate("same", {"condition": "minecraft:random_chance", "chance": 0.5})
"#,
    )
    .unwrap();
    fs::write(
        source_dir.join("extra.cbl"),
        r#"
import stdlib
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
        experimental_resource_pack: false,
        validate: false,
        dry_run: false,
        commands_json: PathBuf::from("data/commands.json"),
    })
    .unwrap_err();

    assert!(error.contains("Duplicate data pack resource"));
    assert!(error.contains("predicate/same"));
    assert!(error.contains("invalid overwrite"));
    assert!(error.contains("first declaration: extra.cbl:3:1"));
    assert!(error.contains("second declaration: main.cbl:4:1"));
}

#[test]
fn datapack_resource_names_reject_invalid_paths() {
    let error = compile_source(
        r#"
import stdlib
datapack.predicate("Bad/Name", {"condition": "minecraft:random_chance", "chance": 1})
"#,
    )
    .unwrap_err();

    assert!(error.contains("Invalid resource name"));
    assert!(error.contains("uppercase character 'B' at position 1"));
    assert!(error.contains("lowercase resource paths"));

    let namespace_error = compile_source(
        r#"
import stdlib
datapack.function_tag("minecraft/load", ["resources:setup"])
"#,
    )
    .unwrap_err();

    assert!(namespace_error.contains("'minecraft' looks like a namespace"));
    assert!(namespace_error.contains("minecraft:load"));

    let tag_value_error = compile_source(
        r#"
import stdlib
datapack.item_tag("rewards", ["minecraft/diamond"])
"#,
    )
    .unwrap_err();

    assert!(tag_value_error.contains("Invalid tag value"));
    assert!(tag_value_error.contains("minecraft:diamond"));

    let non_string_tag_value_error = compile_source(
        r#"
import stdlib
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
import stdlib
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
            experimental_resource_pack: false,
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
fn resource_pack_helpers_require_experimental_opt_in() {
    let error = compile_source(
        r#"
from stdlib import resource_pack
resource_pack.item_model("resources:test_item", {"parent": "minecraft:item/generated"})
"#,
    )
    .unwrap_err();

    assert!(error.contains("resource_pack.* requires --experimental-resource-pack"));
}

#[test]
fn resource_pack_helpers_write_assets_and_manifest_metadata() {
    let (_temp, output_dir) = compile_resource_pack_source(
        r#"
from stdlib import resource_pack
resource_pack.item_model("rp:custom_sword", {
    "parent": "minecraft:item/handheld",
    "textures": {"layer0": "rp:item/custom_sword"}
})
resource_pack.block_model("custom_block", {
    "parent": "minecraft:block/cube_all",
    "textures": {"all": "resources:block/custom_block"}
})
resource_pack.lang("rp:en_us", {
    "item.rp.custom_sword": "Custom Sword",
    "block.resources.custom_block": "Custom Block"
})

def setup():
    /say resources
"#,
    )
    .unwrap();

    assert!(output_dir
        .join("assets/rp/models/item/custom_sword.json")
        .exists());
    assert!(output_dir
        .join("assets/resources/models/block/custom_block.json")
        .exists());
    assert!(output_dir.join("assets/rp/lang/en_us.json").exists());

    let lang: serde_json::Value = serde_json::from_str(
        &fs::read_to_string(output_dir.join("assets/rp/lang/en_us.json")).unwrap(),
    )
    .unwrap();
    assert_eq!(lang["item.rp.custom_sword"], "Custom Sword");

    let manifest: serde_json::Value = serde_json::from_str(
        &fs::read_to_string(output_dir.join(".cobble/build_manifest.json")).unwrap(),
    )
    .unwrap();
    assert_eq!(manifest["generated"]["resource_pack_models"], 2);
    assert_eq!(manifest["generated"]["resource_pack_langs"], 1);
    assert!(manifest["experimental_features"]
        .as_array()
        .unwrap()
        .contains(&serde_json::json!("resource_pack")));
    assert!(manifest["resources"]
        .as_array()
        .unwrap()
        .iter()
        .any(|resource| {
            resource["kind"] == "resource_pack_model"
                && resource["namespace"] == "rp"
                && resource["path"] == "item/custom_sword"
        }));
    assert!(manifest["resources"]
        .as_array()
        .unwrap()
        .iter()
        .any(|resource| {
            resource["kind"] == "resource_pack_lang"
                && resource["namespace"] == "rp"
                && resource["path"] == "en_us"
        }));

    let generated_asset_namespaces: serde_json::Value = serde_json::from_str(
        &fs::read_to_string(output_dir.join(".cobble/generated_asset_namespaces.json")).unwrap(),
    )
    .unwrap();
    assert_eq!(
        generated_asset_namespaces,
        serde_json::json!(["resources", "rp"])
    );
}

#[test]
fn resource_pack_zip_includes_assets() {
    let (temp_dir, output_dir) = compile_source_with_options(
        r#"
from stdlib import resource_pack
resource_pack.item_model("resources:test_item", {"parent": "minecraft:item/generated"})

def setup():
    /say zip
"#,
        true,
        true,
    )
    .unwrap();

    let zip_file = fs::File::open(temp_dir.path().join("resources.zip")).unwrap();
    let mut archive = zip::ZipArchive::new(zip_file).unwrap();
    let names: Vec<String> = (0..archive.len())
        .map(|index| archive.by_index(index).unwrap().name().to_string())
        .collect();

    assert!(output_dir
        .join("assets/resources/models/item/test_item.json")
        .exists());
    assert!(names
        .iter()
        .any(|name| name == "assets/resources/models/item/test_item.json"));
    assert!(names
        .iter()
        .any(|name| name == "data/resources/function/setup.mcfunction"));
}

#[test]
fn resource_pack_zip_excludes_untracked_stale_assets() {
    let temp_dir = tempfile::TempDir::new().unwrap();
    let input_file = temp_dir.path().join("main.cbl");
    let output_dir = temp_dir.path().join("output");
    let stale_asset = output_dir.join("assets/resources/textures/item/stale.png");
    fs::create_dir_all(stale_asset.parent().unwrap()).unwrap();
    fs::write(&stale_asset, b"stale").unwrap();
    fs::write(
        &input_file,
        r#"
from stdlib import resource_pack
resource_pack.item_model("resources:test_item", {"parent": "minecraft:item/generated"})

def setup():
    /say zip
"#,
    )
    .unwrap();

    cobble::commands::build::build(cobble::commands::build::BuildOptions {
        input: Some(input_file),
        output: Some(output_dir.clone()),
        namespace: Some("resources".to_string()),
        pack_format: None,
        description: None,
        verbose: false,
        quiet: false,
        zip: true,
        experimental_resource_pack: true,
        validate: false,
        dry_run: false,
        commands_json: PathBuf::from("data/commands.json"),
    })
    .unwrap();

    let zip_file = fs::File::open(temp_dir.path().join("resources.zip")).unwrap();
    let mut archive = zip::ZipArchive::new(zip_file).unwrap();
    let names: Vec<String> = (0..archive.len())
        .map(|index| archive.by_index(index).unwrap().name().to_string())
        .collect();

    assert!(stale_asset.exists());
    assert!(names
        .iter()
        .any(|name| name == "assets/resources/models/item/test_item.json"));
    assert!(!names
        .iter()
        .any(|name| name == "assets/resources/textures/item/stale.png"));
}

#[test]
fn resource_pack_asset_passthrough_copies_project_assets_and_zip() {
    let temp_dir = tempfile::TempDir::new().unwrap();
    let project_dir = temp_dir.path().join("project");
    let source_dir = project_dir.join("src");
    let output_dir = project_dir.join("output");
    let asset_path = project_dir.join("assets/resources/textures/item/custom_sword.png");
    fs::create_dir_all(&source_dir).unwrap();
    fs::create_dir_all(asset_path.parent().unwrap()).unwrap();
    fs::write(
        source_dir.join("main.cbl"),
        "def setup():\n    /say assets\n",
    )
    .unwrap();
    fs::write(&asset_path, b"fake png bytes").unwrap();
    fs::write(
        project_dir.join("cobble.toml"),
        r#"
[project]
name = "resources"
description = "Resources"
namespace = "resources"
version = "1.0.0"
pack_format = "101.1"

[build]
source = "src"
output = "output"

[experimental]
resource_pack = true
"#,
    )
    .unwrap();

    cobble::commands::build::build(cobble::commands::build::BuildOptions {
        input: Some(source_dir),
        output: Some(output_dir.clone()),
        namespace: None,
        pack_format: None,
        description: None,
        verbose: false,
        quiet: true,
        zip: true,
        experimental_resource_pack: false,
        validate: false,
        dry_run: false,
        commands_json: PathBuf::from("data/commands.json"),
    })
    .unwrap();

    let copied_asset = output_dir.join("assets/resources/textures/item/custom_sword.png");
    assert_eq!(fs::read(&copied_asset).unwrap(), b"fake png bytes");

    let manifest: serde_json::Value = serde_json::from_str(
        &fs::read_to_string(output_dir.join(".cobble/build_manifest.json")).unwrap(),
    )
    .unwrap();
    assert_eq!(manifest["generated"]["resource_pack_static_assets"], 1);
    assert!(manifest["resources"]
        .as_array()
        .unwrap()
        .iter()
        .any(|resource| {
            resource["kind"] == "resource_pack_static_asset"
                && resource["namespace"] == "resources"
                && resource["path"] == "textures/item/custom_sword.png"
        }));

    let zip_file = fs::File::open(project_dir.join("resources.zip")).unwrap();
    let mut archive = zip::ZipArchive::new(zip_file).unwrap();
    let names: Vec<String> = (0..archive.len())
        .map(|index| archive.by_index(index).unwrap().name().to_string())
        .collect();

    assert!(names
        .iter()
        .any(|name| name == "assets/resources/textures/item/custom_sword.png"));
    assert!(names
        .iter()
        .any(|name| name == "data/resources/function/setup.mcfunction"));
}

#[cfg(unix)]
#[test]
fn resource_pack_asset_passthrough_rejects_symlinked_assets() {
    use std::os::unix::fs::symlink;

    let temp_dir = tempfile::TempDir::new().unwrap();
    let project_dir = temp_dir.path().join("project");
    let source_dir = project_dir.join("src");
    let output_dir = project_dir.join("output");
    let outside_dir = temp_dir.path().join("outside");
    let symlink_path = project_dir.join("assets/resources/textures/item/leak.png");
    fs::create_dir_all(&source_dir).unwrap();
    fs::create_dir_all(&outside_dir).unwrap();
    fs::create_dir_all(symlink_path.parent().unwrap()).unwrap();
    fs::write(
        source_dir.join("main.cbl"),
        "def setup():\n    /say assets\n",
    )
    .unwrap();
    fs::write(outside_dir.join("secret.png"), b"secret").unwrap();
    symlink(outside_dir.join("secret.png"), &symlink_path).unwrap();
    fs::write(
        project_dir.join("cobble.toml"),
        r#"
[project]
name = "resources"
description = "Resources"
namespace = "resources"
version = "1.0.0"
pack_format = "101.1"

[build]
source = "src"
output = "output"

[experimental]
resource_pack = true
"#,
    )
    .unwrap();

    let error = cobble::commands::build::build(cobble::commands::build::BuildOptions {
        input: Some(source_dir),
        output: Some(output_dir.clone()),
        namespace: None,
        pack_format: None,
        description: None,
        verbose: false,
        quiet: true,
        zip: false,
        experimental_resource_pack: false,
        validate: false,
        dry_run: false,
        commands_json: PathBuf::from("data/commands.json"),
    })
    .unwrap_err();

    assert!(error.contains("Refusing to copy resource-pack assets through symlink"));
    assert!(!output_dir
        .join("assets/resources/textures/item/leak.png")
        .exists());
}

#[test]
fn resource_pack_asset_passthrough_rejects_generated_asset_collision() {
    let temp_dir = tempfile::TempDir::new().unwrap();
    let project_dir = temp_dir.path().join("project");
    let source_dir = project_dir.join("src");
    let output_dir = project_dir.join("output");
    let asset_path = project_dir.join("assets/resources/models/item/test_item.json");
    fs::create_dir_all(&source_dir).unwrap();
    fs::create_dir_all(asset_path.parent().unwrap()).unwrap();
    fs::write(
        source_dir.join("main.cbl"),
        r#"
from stdlib import resource_pack
resource_pack.item_model("resources:test_item", {"parent": "minecraft:item/generated"})
"#,
    )
    .unwrap();
    fs::write(&asset_path, r#"{"parent":"minecraft:item/handheld"}"#).unwrap();
    fs::write(
        project_dir.join("cobble.toml"),
        r#"
[project]
name = "resources"
description = "Resources"
namespace = "resources"
version = "1.0.0"
pack_format = "101.1"

[build]
source = "src"
output = "output"

[experimental]
resource_pack = true
"#,
    )
    .unwrap();

    let error = cobble::commands::build::build(cobble::commands::build::BuildOptions {
        input: Some(source_dir),
        output: Some(output_dir.clone()),
        namespace: None,
        pack_format: None,
        description: None,
        verbose: false,
        quiet: true,
        zip: false,
        experimental_resource_pack: false,
        validate: false,
        dry_run: false,
        commands_json: PathBuf::from("data/commands.json"),
    })
    .unwrap_err();

    assert!(error.contains("conflicts with generated resource-pack output"));
    assert!(!output_dir.exists());
}

#[test]
fn resource_pack_asset_passthrough_rejects_generated_output_cleaning_overlap() {
    let temp_dir = tempfile::TempDir::new().unwrap();
    let project_dir = temp_dir.path().join("project");
    let source_dir = project_dir.join("src");
    let asset_path = project_dir.join("assets/resources/models/item/static_item.json");
    fs::create_dir_all(&source_dir).unwrap();
    fs::create_dir_all(asset_path.parent().unwrap()).unwrap();
    fs::write(
        source_dir.join("main.cbl"),
        r#"
from stdlib import resource_pack
resource_pack.item_model("resources:generated_item", {"parent": "minecraft:item/generated"})
"#,
    )
    .unwrap();
    fs::write(&asset_path, r#"{"parent":"minecraft:item/handheld"}"#).unwrap();
    fs::write(
        project_dir.join("cobble.toml"),
        r#"
[project]
name = "resources"
description = "Resources"
namespace = "resources"
version = "1.0.0"
pack_format = "101.1"

[build]
source = "src"
output = "."

[experimental]
resource_pack = true
"#,
    )
    .unwrap();

    let error = cobble::commands::build::build(cobble::commands::build::BuildOptions {
        input: Some(source_dir),
        output: Some(project_dir.clone()),
        namespace: None,
        pack_format: None,
        description: None,
        verbose: false,
        quiet: true,
        zip: false,
        experimental_resource_pack: false,
        validate: false,
        dry_run: false,
        commands_json: PathBuf::from("data/commands.json"),
    })
    .unwrap_err();

    assert!(error.contains("Refusing to copy resource-pack assets from output assets tree"));
    assert!(asset_path.exists());
    assert!(!project_dir.join("pack.mcmeta").exists());
}

#[test]
fn resource_pack_asset_passthrough_rejects_previous_generated_output_overlap() {
    let temp_dir = tempfile::TempDir::new().unwrap();
    let project_dir = temp_dir.path().join("project");
    let source_dir = project_dir.join("src");
    let asset_path = project_dir.join("assets/resources/textures/item/static_item.png");
    fs::create_dir_all(&source_dir).unwrap();
    fs::create_dir_all(asset_path.parent().unwrap()).unwrap();
    fs::create_dir_all(project_dir.join(".cobble")).unwrap();
    fs::write(
        source_dir.join("main.cbl"),
        "def setup():\n    /say assets\n",
    )
    .unwrap();
    fs::write(&asset_path, b"keep").unwrap();
    fs::write(
        project_dir.join(".cobble/generated_asset_namespaces.json"),
        r#"["resources"]"#,
    )
    .unwrap();
    fs::write(
        project_dir.join("cobble.toml"),
        r#"
[project]
name = "resources"
description = "Resources"
namespace = "resources"
version = "1.0.0"
pack_format = "101.1"

[build]
source = "src"
output = "."

[experimental]
resource_pack = true
"#,
    )
    .unwrap();

    let error = cobble::commands::build::build(cobble::commands::build::BuildOptions {
        input: Some(source_dir),
        output: Some(project_dir.clone()),
        namespace: None,
        pack_format: None,
        description: None,
        verbose: false,
        quiet: true,
        zip: false,
        experimental_resource_pack: false,
        validate: false,
        dry_run: false,
        commands_json: PathBuf::from("data/commands.json"),
    })
    .unwrap_err();

    assert!(error.contains("Previous generated resource-pack output may clean assets/resources"));
    assert_eq!(fs::read(&asset_path).unwrap(), b"keep");
    assert!(!project_dir.join("pack.mcmeta").exists());
}

#[test]
fn disabling_resource_pack_cleans_stale_static_assets_from_output_and_zip() {
    let temp_dir = tempfile::TempDir::new().unwrap();
    let project_dir = temp_dir.path().join("project");
    let source_dir = project_dir.join("src");
    let output_dir = project_dir.join("output");
    let asset_path = project_dir.join("assets/resources/textures/item/stale.png");
    fs::create_dir_all(&source_dir).unwrap();
    fs::create_dir_all(asset_path.parent().unwrap()).unwrap();
    fs::write(
        source_dir.join("main.cbl"),
        "def setup():\n    /say assets\n",
    )
    .unwrap();
    fs::write(&asset_path, b"stale").unwrap();
    fs::write(
        project_dir.join("cobble.toml"),
        r#"
[project]
name = "resources"
description = "Resources"
namespace = "resources"
version = "1.0.0"
pack_format = "101.1"

[build]
source = "src"
output = "output"

[experimental]
resource_pack = true
"#,
    )
    .unwrap();

    let build_options = |zip| cobble::commands::build::BuildOptions {
        input: Some(source_dir.clone()),
        output: Some(output_dir.clone()),
        namespace: None,
        pack_format: None,
        description: None,
        verbose: false,
        quiet: true,
        zip,
        experimental_resource_pack: false,
        validate: false,
        dry_run: false,
        commands_json: PathBuf::from("data/commands.json"),
    };

    cobble::commands::build::build(build_options(false)).unwrap();
    assert!(output_dir
        .join("assets/resources/textures/item/stale.png")
        .exists());

    fs::write(
        project_dir.join("cobble.toml"),
        r#"
[project]
name = "resources"
description = "Resources"
namespace = "resources"
version = "1.0.0"
pack_format = "101.1"

[build]
source = "src"
output = "output"

[experimental]
resource_pack = false
"#,
    )
    .unwrap();

    cobble::commands::build::build(build_options(true)).unwrap();

    assert_eq!(fs::read(&asset_path).unwrap(), b"stale");
    assert!(!output_dir
        .join("assets/resources/textures/item/stale.png")
        .exists());

    let zip_file = fs::File::open(project_dir.join("resources.zip")).unwrap();
    let mut archive = zip::ZipArchive::new(zip_file).unwrap();
    let names: Vec<String> = (0..archive.len())
        .map(|index| archive.by_index(index).unwrap().name().to_string())
        .collect();
    assert!(!names
        .iter()
        .any(|name| name == "assets/resources/textures/item/stale.png"));
}

#[test]
fn disabling_resource_pack_preserves_project_assets_when_output_is_project_root() {
    let temp_dir = tempfile::TempDir::new().unwrap();
    let project_dir = temp_dir.path().join("project");
    let source_dir = project_dir.join("src");
    let asset_path = project_dir.join("assets/resources/models/item/source_item.json");
    fs::create_dir_all(&source_dir).unwrap();
    fs::create_dir_all(asset_path.parent().unwrap()).unwrap();
    fs::create_dir_all(project_dir.join(".cobble")).unwrap();
    fs::write(
        source_dir.join("main.cbl"),
        "def setup():\n    /say assets\n",
    )
    .unwrap();
    fs::write(&asset_path, r#"{"parent":"minecraft:item/generated"}"#).unwrap();
    fs::write(
        project_dir.join(".cobble/generated_asset_namespaces.json"),
        r#"["resources"]"#,
    )
    .unwrap();
    fs::write(
        project_dir.join(".cobble/static_asset_passthrough.json"),
        r#"["resources/models/item/source_item.json"]"#,
    )
    .unwrap();
    fs::write(
        project_dir.join("cobble.toml"),
        r#"
[project]
name = "resources"
description = "Resources"
namespace = "resources"
version = "1.0.0"
pack_format = "101.1"

[build]
source = "src"
output = "."

[experimental]
resource_pack = false
"#,
    )
    .unwrap();

    cobble::commands::build::build(cobble::commands::build::BuildOptions {
        input: Some(source_dir),
        output: Some(project_dir.clone()),
        namespace: None,
        pack_format: None,
        description: None,
        verbose: false,
        quiet: true,
        zip: true,
        experimental_resource_pack: false,
        validate: false,
        dry_run: false,
        commands_json: PathBuf::from("data/commands.json"),
    })
    .unwrap();

    assert_eq!(
        fs::read_to_string(&asset_path).unwrap(),
        r#"{"parent":"minecraft:item/generated"}"#
    );
    assert!(!project_dir
        .join(".cobble/generated_asset_namespaces.json")
        .exists());

    let zip_file = fs::File::open(temp_dir.path().join("resources.zip")).unwrap();
    let mut archive = zip::ZipArchive::new(zip_file).unwrap();
    let names: Vec<String> = (0..archive.len())
        .map(|index| archive.by_index(index).unwrap().name().to_string())
        .collect();
    assert!(!names
        .iter()
        .any(|name| name == "assets/resources/models/item/source_item.json"));
}

#[test]
fn resource_pack_asset_passthrough_cleans_removed_static_assets_and_zip() {
    let temp_dir = tempfile::TempDir::new().unwrap();
    let project_dir = temp_dir.path().join("project");
    let source_dir = project_dir.join("src");
    let output_dir = project_dir.join("output");
    let asset_path = project_dir.join("assets/resources/textures/item/old.png");
    fs::create_dir_all(&source_dir).unwrap();
    fs::create_dir_all(asset_path.parent().unwrap()).unwrap();
    fs::write(
        source_dir.join("main.cbl"),
        "def setup():\n    /say assets\n",
    )
    .unwrap();
    fs::write(&asset_path, b"old").unwrap();
    fs::write(
        project_dir.join("cobble.toml"),
        r#"
[project]
name = "resources"
description = "Resources"
namespace = "resources"
version = "1.0.0"
pack_format = "101.1"

[build]
source = "src"
output = "output"

[experimental]
resource_pack = true
"#,
    )
    .unwrap();

    let build_options = |zip| cobble::commands::build::BuildOptions {
        input: Some(source_dir.clone()),
        output: Some(output_dir.clone()),
        namespace: None,
        pack_format: None,
        description: None,
        verbose: false,
        quiet: true,
        zip,
        experimental_resource_pack: false,
        validate: false,
        dry_run: false,
        commands_json: PathBuf::from("data/commands.json"),
    };

    cobble::commands::build::build(build_options(false)).unwrap();
    assert!(output_dir
        .join("assets/resources/textures/item/old.png")
        .exists());

    fs::remove_file(&asset_path).unwrap();
    cobble::commands::build::build(build_options(true)).unwrap();

    assert!(!output_dir
        .join("assets/resources/textures/item/old.png")
        .exists());

    let manifest: serde_json::Value = serde_json::from_str(
        &fs::read_to_string(output_dir.join(".cobble/build_manifest.json")).unwrap(),
    )
    .unwrap();
    assert_eq!(
        manifest["generated"]["resource_pack_static_assets"]
            .as_u64()
            .unwrap_or(0),
        0
    );

    let zip_file = fs::File::open(project_dir.join("resources.zip")).unwrap();
    let mut archive = zip::ZipArchive::new(zip_file).unwrap();
    let names: Vec<String> = (0..archive.len())
        .map(|index| archive.by_index(index).unwrap().name().to_string())
        .collect();
    assert!(!names
        .iter()
        .any(|name| name == "assets/resources/textures/item/old.png"));
}

#[test]
fn validated_resource_pack_rebuild_cleans_removed_static_assets_and_zip() {
    let temp_dir = tempfile::TempDir::new().unwrap();
    let project_dir = temp_dir.path().join("project");
    let source_dir = project_dir.join("src");
    let output_dir = project_dir.join("output");
    let asset_path = project_dir.join("assets/resources/textures/item/old.png");
    let valid_commands_json = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("data")
        .join("commands.json");
    if !valid_commands_json.exists() {
        return;
    }

    fs::create_dir_all(&source_dir).unwrap();
    fs::create_dir_all(asset_path.parent().unwrap()).unwrap();
    fs::write(
        source_dir.join("main.cbl"),
        "def setup():\n    /say assets\n",
    )
    .unwrap();
    fs::write(&asset_path, b"old").unwrap();
    fs::write(
        project_dir.join("cobble.toml"),
        r#"
[project]
name = "resources"
description = "Resources"
namespace = "resources"
version = "1.0.0"
pack_format = "101.1"

[build]
source = "src"
output = "output"

[experimental]
resource_pack = true
"#,
    )
    .unwrap();

    let build_options = |zip| cobble::commands::build::BuildOptions {
        input: Some(source_dir.clone()),
        output: Some(output_dir.clone()),
        namespace: None,
        pack_format: None,
        description: None,
        verbose: false,
        quiet: true,
        zip,
        experimental_resource_pack: false,
        validate: true,
        dry_run: false,
        commands_json: valid_commands_json.clone(),
    };

    cobble::commands::build::build(build_options(false)).unwrap();
    assert!(output_dir
        .join("assets/resources/textures/item/old.png")
        .exists());

    fs::remove_file(&asset_path).unwrap();
    cobble::commands::build::build(build_options(true)).unwrap();

    assert!(!output_dir
        .join("assets/resources/textures/item/old.png")
        .exists());

    let manifest: serde_json::Value = serde_json::from_str(
        &fs::read_to_string(output_dir.join(".cobble/build_manifest.json")).unwrap(),
    )
    .unwrap();
    assert_eq!(
        manifest["generated"]["resource_pack_static_assets"]
            .as_u64()
            .unwrap_or(0),
        0
    );

    let zip_file = fs::File::open(project_dir.join("resources.zip")).unwrap();
    let mut archive = zip::ZipArchive::new(zip_file).unwrap();
    let names: Vec<String> = (0..archive.len())
        .map(|index| archive.by_index(index).unwrap().name().to_string())
        .collect();
    assert!(!names
        .iter()
        .any(|name| name == "assets/resources/textures/item/old.png"));
}

#[test]
fn validated_resource_pack_opt_out_cleans_stale_static_assets_and_zip() {
    let temp_dir = tempfile::TempDir::new().unwrap();
    let project_dir = temp_dir.path().join("project");
    let source_dir = project_dir.join("src");
    let output_dir = project_dir.join("output");
    let asset_path = project_dir.join("assets/resources/textures/item/old.png");
    let valid_commands_json = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("data")
        .join("commands.json");
    if !valid_commands_json.exists() {
        return;
    }

    fs::create_dir_all(&source_dir).unwrap();
    fs::create_dir_all(asset_path.parent().unwrap()).unwrap();
    fs::write(
        source_dir.join("main.cbl"),
        "def setup():\n    /say assets\n",
    )
    .unwrap();
    fs::write(&asset_path, b"old").unwrap();
    fs::write(
        project_dir.join("cobble.toml"),
        r#"
[project]
name = "resources"
description = "Resources"
namespace = "resources"
version = "1.0.0"
pack_format = "101.1"

[build]
source = "src"
output = "output"

[experimental]
resource_pack = true
"#,
    )
    .unwrap();

    let build_options = |zip| cobble::commands::build::BuildOptions {
        input: Some(source_dir.clone()),
        output: Some(output_dir.clone()),
        namespace: None,
        pack_format: None,
        description: None,
        verbose: false,
        quiet: true,
        zip,
        experimental_resource_pack: false,
        validate: true,
        dry_run: false,
        commands_json: valid_commands_json.clone(),
    };

    cobble::commands::build::build(build_options(false)).unwrap();
    assert!(output_dir
        .join("assets/resources/textures/item/old.png")
        .exists());

    fs::write(
        project_dir.join("cobble.toml"),
        r#"
[project]
name = "resources"
description = "Resources"
namespace = "resources"
version = "1.0.0"
pack_format = "101.1"

[build]
source = "src"
output = "output"

[experimental]
resource_pack = false
"#,
    )
    .unwrap();
    cobble::commands::build::build(build_options(true)).unwrap();

    assert!(!output_dir
        .join("assets/resources/textures/item/old.png")
        .exists());
    assert!(!output_dir
        .join(".cobble/static_asset_passthrough.json")
        .exists());

    let zip_file = fs::File::open(project_dir.join("resources.zip")).unwrap();
    let mut archive = zip::ZipArchive::new(zip_file).unwrap();
    let names: Vec<String> = (0..archive.len())
        .map(|index| archive.by_index(index).unwrap().name().to_string())
        .collect();
    assert!(!names
        .iter()
        .any(|name| name == "assets/resources/textures/item/old.png"));
}

#[cfg(unix)]
#[test]
fn validated_resource_pack_asset_failure_preserves_previous_output() {
    let temp_dir = tempfile::TempDir::new().unwrap();
    let project_dir = temp_dir.path().join("project");
    let source_dir = project_dir.join("src");
    let output_dir = project_dir.join("output");
    let asset_path = project_dir.join("assets/resources/textures/item/icon.png");
    let valid_commands_json = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("data")
        .join("commands.json");
    if !valid_commands_json.exists() {
        return;
    }

    fs::create_dir_all(&source_dir).unwrap();
    fs::create_dir_all(asset_path.parent().unwrap()).unwrap();
    fs::write(
        source_dir.join("main.cbl"),
        "def setup():\n    /say first\n",
    )
    .unwrap();
    fs::write(&asset_path, b"old asset").unwrap();
    fs::write(
        project_dir.join("cobble.toml"),
        r#"
[project]
name = "resources"
description = "Resources"
namespace = "resources"
version = "1.0.0"
pack_format = "101.1"

[build]
source = "src"
output = "output"

[experimental]
resource_pack = true
"#,
    )
    .unwrap();

    let build_options = || cobble::commands::build::BuildOptions {
        input: Some(source_dir.clone()),
        output: Some(output_dir.clone()),
        namespace: None,
        pack_format: None,
        description: None,
        verbose: false,
        quiet: true,
        zip: false,
        experimental_resource_pack: false,
        validate: true,
        dry_run: false,
        commands_json: valid_commands_json.clone(),
    };

    cobble::commands::build::build(build_options()).unwrap();
    assert_eq!(
        fs::read_to_string(output_dir.join("data/resources/function/setup.mcfunction")).unwrap(),
        "say first\n"
    );
    assert_eq!(
        fs::read(output_dir.join("assets/resources/textures/item/icon.png")).unwrap(),
        b"old asset"
    );

    fs::write(
        source_dir.join("main.cbl"),
        "def setup():\n    /say second\n",
    )
    .unwrap();
    fs::write(&asset_path, b"new asset").unwrap();
    fs::set_permissions(&asset_path, fs::Permissions::from_mode(0o000)).unwrap();

    let error = cobble::commands::build::build(build_options()).unwrap_err();
    assert!(error.contains("Failed to copy resource-pack asset"));
    assert_eq!(
        fs::read_to_string(output_dir.join("data/resources/function/setup.mcfunction")).unwrap(),
        "say first\n"
    );
    assert_eq!(
        fs::read(output_dir.join("assets/resources/textures/item/icon.png")).unwrap(),
        b"old asset"
    );
}

#[test]
fn resource_pack_lang_declarations_merge_deterministically() {
    let (_temp, output_dir) = compile_resource_pack_source(
        r#"
from stdlib import resource_pack
resource_pack.lang("en_us", {
    "item.resources.zeta": "Zeta",
    "item.resources.same": "Same"
})
resource_pack.lang("en_us", {
    "block.resources.alpha": "Alpha",
    "item.resources.same": "Same"
})
"#,
    )
    .unwrap();

    let lang_path = output_dir.join("assets/resources/lang/en_us.json");
    let lang_contents = fs::read_to_string(&lang_path).unwrap();
    let lang: serde_json::Value = serde_json::from_str(&lang_contents).unwrap();
    assert_eq!(lang["block.resources.alpha"], "Alpha");
    assert_eq!(lang["item.resources.same"], "Same");
    assert_eq!(lang["item.resources.zeta"], "Zeta");

    let alpha_index = lang_contents.find("\"block.resources.alpha\"").unwrap();
    let same_index = lang_contents.find("\"item.resources.same\"").unwrap();
    let zeta_index = lang_contents.find("\"item.resources.zeta\"").unwrap();
    assert!(alpha_index < same_index);
    assert!(same_index < zeta_index);

    let manifest: serde_json::Value = serde_json::from_str(
        &fs::read_to_string(output_dir.join(".cobble/build_manifest.json")).unwrap(),
    )
    .unwrap();
    assert_eq!(manifest["generated"]["resource_pack_langs"], 1);
    let lang_resources: Vec<_> = manifest["resources"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|resource| resource["kind"] == "resource_pack_lang")
        .collect();
    assert_eq!(lang_resources.len(), 1);
}

#[test]
fn resource_pack_model_declarations_validate_known_field_types() {
    let parent_error = compile_resource_pack_source(
        r#"
from stdlib import resource_pack
resource_pack.item_model("bad_parent", {"parent": 3})
"#,
    )
    .unwrap_err();
    assert!(parent_error.contains("field 'parent' must be a string"));

    let textures_error = compile_resource_pack_source(
        r#"
from stdlib import resource_pack
resource_pack.block_model("bad_textures", {"textures": {"all": 7}})
"#,
    )
    .unwrap_err();
    assert!(textures_error.contains("texture 'all' must be a string"));

    let elements_error = compile_resource_pack_source(
        r#"
from stdlib import resource_pack
resource_pack.block_model("bad_elements", {"elements": {"from": [0, 0, 0]}})
"#,
    )
    .unwrap_err();
    assert!(elements_error.contains("field 'elements' must be an array"));
}

#[test]
fn conflicting_resource_pack_lang_entries_report_duplicate_diagnostic() {
    compile_resource_pack_source(
        r#"
from stdlib import resource_pack
resource_pack.lang("en_us", {"item.resources.test": "Test"})
resource_pack.lang("en_us", {"item.resources.test": "Test"})
"#,
    )
    .unwrap();

    let error = compile_resource_pack_source(
        r#"
from stdlib import resource_pack
resource_pack.lang("en_us", {"item.resources.test": "Test"})
resource_pack.lang("en_us", {"item.resources.test": "Changed"})
"#,
    )
    .unwrap_err();

    assert!(error.contains("Duplicate resource pack language file"));
    assert!(error.contains("invalid overwrite"));
    assert!(error.contains("translation key 'item.resources.test'"));
    assert!(error.contains("first declaration: main.cbl:3:1"));
    assert!(error.contains("second declaration: main.cbl:4:1"));
}

#[test]
fn item_component_helpers_generate_item_modifier_component_json() {
    let (_temp, output_dir) = compile_source(
        r#"
from stdlib import datapack, item_component, text

datapack.item_modifier("items/reward_components", {
    "function": "minecraft:set_components",
    "components": item_component.components(
        item_component.custom_name(text.colored("Cobble Reward", "gold")),
        item_component.lore([
            text.plain("Generated by Cobble"),
            text.colored("Keep this item", "gray")
        ]),
        item_component.unbreakable()
    )
})

def setup():
    /say setup
"#,
    )
    .unwrap();

    let item_modifier_path =
        output_dir.join("data/resources/item_modifier/items/reward_components.json");
    let item_modifier: serde_json::Value =
        serde_json::from_str(&fs::read_to_string(item_modifier_path).unwrap()).unwrap();

    assert_eq!(item_modifier["function"], "minecraft:set_components");
    assert_eq!(
        item_modifier["components"]["minecraft:custom_name"],
        serde_json::json!({"color": "gold", "text": "Cobble Reward"})
    );
    assert_eq!(
        item_modifier["components"]["minecraft:lore"],
        serde_json::json!([
            {"text": "Generated by Cobble"},
            {"color": "gray", "text": "Keep this item"}
        ])
    );
    assert_eq!(
        item_modifier["components"]["minecraft:unbreakable"],
        serde_json::json!({})
    );

    let manifest: serde_json::Value = serde_json::from_str(
        &fs::read_to_string(output_dir.join(".cobble/build_manifest.json")).unwrap(),
    )
    .unwrap();
    assert_eq!(manifest["generated"]["item_modifiers"], 1);
    assert_eq!(manifest["generated"]["total_json_resources"], 1);
    assert!(manifest["resources"]
        .as_array()
        .unwrap()
        .iter()
        .any(|resource| resource["kind"] == "item_modifier"
            && resource["namespace"] == "resources"
            && resource["path"] == "items/reward_components"));
}

#[test]
fn item_component_helpers_reject_duplicate_components_and_standalone_calls() {
    let duplicate_error = compile_source(
        r#"
from stdlib import datapack, item_component

datapack.item_modifier("duplicate", {
    "function": "minecraft:set_components",
    "components": item_component.components(
        item_component.unbreakable(),
        item_component.unbreakable()
    )
})
"#,
    )
    .unwrap_err();
    assert!(duplicate_error
        .contains("item_component.components() duplicate component 'minecraft:unbreakable'"));

    let standalone_error = compile_source(
        r#"
from stdlib import item_component

def standalone():
    item_component.unbreakable()
"#,
    )
    .unwrap_err();
    assert!(standalone_error
        .contains("item_component.unbreakable() returns an item component JSON object"));
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
            experimental_resource_pack: false,
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
        experimental_resource_pack: false,
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
import stdlib
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
        experimental_resource_pack: false,
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
