//! Resource checks that keep a pack from loading on a 26.3 server (bugs 68,
//! 69, 73-79 and the missing resource checks). Each rule was probed on the
//! server: the bad form stopped it from starting, dropped the resource, or
//! dropped a tag and the functions using it. The good forms here loaded.

mod common;

use common::mcsim::compile;

/// The error a pack made of `declarations` fails with.
fn error(declarations: &str) -> String {
    let source = format!("from stdlib import datapack\n\n{declarations}\n");
    match compile(&source) {
        Ok(_) => panic!("expected a compile error for:\n{declarations}"),
        Err(error) => error,
    }
}

fn builds(declarations: &str) {
    let source = format!("from stdlib import datapack\n\n{declarations}\n");
    if let Err(error) = compile(&source) {
        panic!("expected a build for:\n{declarations}\n{error}");
    }
}

fn assert_errors(cases: &[(&str, &str)]) {
    for (declarations, expected) in cases {
        let error = error(declarations);
        assert!(
            error.contains(expected),
            "{declarations}\nexpected `{expected}` in:\n{error}"
        );
        assert!(error.contains("main.cbl:"), "{declarations}: {error}");
    }
}

const STONE: &str = r###"{"type": "minecraft:item", "name": "minecraft:stone"}"###;

fn pool(entries: &str) -> String {
    format!(
        r###"datapack.loot_table("a", {{"pools": [{{"rolls": 1, "entries": [{entries}]}}]}})"###
    )
}

#[test]
fn loot_tables_need_their_fields() {
    assert_errors(&[
        (
            &format!(r###"datapack.loot_table("a", {{"pools": [{{"entries": [{STONE}]}}]}})"###),
            "`pools[0].rolls` is missing",
        ),
        (
            r###"datapack.loot_table("a", {"pools": [{"rolls": 1}]})"###,
            "`pools[0].entries` is missing",
        ),
        (
            &pool(r###"{"name": "minecraft:stone"}"###),
            "`pools[0].entries[0].type` is missing",
        ),
        (
            &pool(r###"{"type": "minecraft:item"}"###),
            "`pools[0].entries[0].name` is missing",
        ),
        (
            &pool(r###"{"type": "minecraft:tag", "expand": True}"###),
            "`pools[0].entries[0].items` is missing",
        ),
        (
            &pool(
                r###"{"type": "minecraft:loot_table", "name": "minecraft:chests/simple_dungeon"}"###,
            ),
            "is now `value`",
        ),
        (
            &pool(r###"{"type": "minecraft:item", "name": "minecraft:air"}"###),
            "minecraft:empty",
        ),
        (
            &pool(r###"{"type": "minecraft:item", "name": "minecraft:diamon"}"###),
            "`minecraft:diamon` is not an item",
        ),
        (
            &pool(r###"{"type": "minecraft:tag", "items": "minecraft:logs", "expand": True}"###),
            "`minecraft:logs` is not an item",
        ),
    ]);
    builds(&pool(
        r###"{"type": "minecraft:tag", "items": "#minecraft:logs", "expand": True}"###,
    ));
}

#[test]
fn a_condition_is_one_predicate_not_a_list() {
    assert_errors(&[
        (
            &format!(
                r###"datapack.loot_table("a", {{"pools": [{{"rolls": 1, "condition": [{{"type": "minecraft:random_chance", "chance": 0.5}}], "entries": [{STONE}]}}]}})"###
            ),
            "`pools[0].condition` is one predicate, not a list",
        ),
        (
            r###"datapack.item_modifier("a", {"type": "minecraft:set_count", "count": 1, "condition": [{"type": "minecraft:random_chance", "chance": 0.5}]})"###,
            "`condition` is one predicate, not a list",
        ),
    ]);
}

#[test]
fn item_modifiers_need_their_fields() {
    assert_errors(&[
        (
            r###"datapack.item_modifier("a", {"type": "minecraft:set_count"})"###,
            "`count` is missing",
        ),
        (
            r###"datapack.item_modifier("a", {"type": "minecraft:set_lore", "lore": ["x"]})"###,
            "`mode` is missing",
        ),
        (
            r###"datapack.item_modifier("a", {"type": "minecraft:set_components"})"###,
            "`components` is missing",
        ),
        (
            r###"datapack.item_modifier("a", {"type": "minecraft:set_custom_data"})"###,
            "`tag` is missing",
        ),
        (
            r###"datapack.item_modifier("a", {"type": "minecraft:apply_bonus"})"###,
            "`formula` is missing",
        ),
        (
            r###"datapack.item_modifier("a", [{"type": "minecraft:limit_count"}])"###,
            "`[0].limit` is missing",
        ),
        (
            r###"datapack.item_modifier("a", {"type": "minecraft:sequence", "functions": [{"function": "minecraft:set_count", "count": 1}]})"###,
            "`functions[0].function` is now `type`",
        ),
        (
            r###"datapack.item_modifier("a", {"type": "minecraft:enchant_randomly", "options": "minecraft:sharpnes"})"###,
            "`minecraft:sharpnes` is not an enchantment",
        ),
    ]);
    builds(r###"datapack.item_modifier("a", {"type": "minecraft:set_name"})"###);
}

#[test]
fn predicates_need_their_fields_and_known_ids() {
    assert_errors(&[
        (
            r###"datapack.predicate("a", {"type": "minecraft:random_chance"})"###,
            "`chance` is missing",
        ),
        (
            r###"datapack.predicate("a", {"type": "minecraft:entity_properties", "predicate": {}})"###,
            "`entity` is missing",
        ),
        (
            r###"datapack.predicate("a", {"type": "minecraft:all_of"})"###,
            "`terms` is missing",
        ),
        (
            r###"datapack.predicate("a", {"type": "minecraft:inverted"})"###,
            "`term` is missing",
        ),
        (
            r###"datapack.predicate("a", {"type": "minecraft:enchantment_active_check"})"###,
            "`active` is missing",
        ),
        (
            r###"datapack.predicate("a", {"type": "minecraft:all_of", "terms": [{"type": "minecraft:random_chance"}]})"###,
            "`terms[0].chance` is missing",
        ),
        (
            r###"datapack.predicate("a", {"type": "minecraft:time_check", "value": {"min": 0, "max": 6000}})"###,
            "`clock` is missing",
        ),
        (
            r###"datapack.predicate("a", {"type": "minecraft:entity_properties", "entity": "this", "predicate": {"minecraft:entity_type": "minecraft:zombi"}})"###,
            "`minecraft:zombi` is not an entity type",
        ),
        (
            r###"datapack.predicate("a", {"type": "minecraft:location_check", "predicate": {"block": {"blocks": "minecraft:ston"}}})"###,
            "`minecraft:ston` is not a block",
        ),
        (
            r###"datapack.predicate("a", {"type": "minecraft:entity_properties", "entity": "this", "predicate": {"effects": {"minecraft:sped": {}}}})"###,
            "`minecraft:sped` is not an effect",
        ),
        (
            r###"datapack.predicate("a", {"type": "minecraft:match_tool", "predicate": {"items": ["#minecraft:logs"]}})"###,
            "a list holds ids only",
        ),
        (
            r###"datapack.predicate("a", {"type": "minecraft:match_tool", "predicate": {"items": "#minecraft:nope"}})"###,
            "the item tag `#minecraft:nope`",
        ),
    ]);
    // The server ignores `block`, which match_block doesn't read.
    builds(
        r###"datapack.predicate("a", {"type": "minecraft:match_block", "block": "minecraft:ston"})"###,
    );
    builds(
        r###"datapack.predicate("a", {"type": "minecraft:time_check", "clock": "minecraft:overworld", "value": {"min": 0, "max": 6000}})"###,
    );
}

fn stonecutting(ingredient: &str, result: &str) -> String {
    format!(
        r###"datapack.recipe("a", {{"type": "minecraft:stonecutting", "ingredient": {ingredient}, "result": {result}}})"###
    )
}

const SLAB: &str = r###"{"id": "minecraft:stone_slab"}"###;

#[test]
fn recipes_need_their_fields_and_known_items() {
    assert_errors(&[
        (
            &stonecutting(r###"["#minecraft:logs", "minecraft:stone"]"###, SLAB),
            "a list holds ids only",
        ),
        (
            r###"datapack.recipe("a", {"type": "minecraft:crafting_dye", "target": {"item": "minecraft:stone"}, "dye": "minecraft:red_dye", "result": {"id": "minecraft:stone"}})"###,
            "`target` is now the item id itself",
        ),
        (
            &stonecutting(r###""minecraft:ston""###, SLAB),
            "`minecraft:ston` is not an item",
        ),
        (
            &stonecutting(r###""#minecraft:log""###, SLAB),
            "the item tag `#minecraft:log`",
        ),
        (
            &stonecutting(r###""#sim:nope""###, SLAB),
            "the item tag `#sim:nope`",
        ),
        (
            r###"datapack.recipe("a", {"type": "minecraft:stonecutting", "ingredient": "minecraft:stone"})"###,
            "`result` is missing",
        ),
        (
            r###"datapack.recipe("a", {"ingredient": "minecraft:stone", "result": {"id": "minecraft:stone_slab"}})"###,
            "`type` is missing",
        ),
        (
            &stonecutting(r###""minecraft:stone""###, r###"{"count": 2}"###),
            "`result.id` is missing",
        ),
        (
            &stonecutting(
                r###""minecraft:stone""###,
                r###"{"id": "minecraft:stone", "count": 100}"###,
            ),
            "1 to 99",
        ),
        (
            &stonecutting(r###""minecraft:stone""###, r###"{"id": "minecraft:air"}"###),
            "can't be air",
        ),
        (
            r###"datapack.recipe("a", {"type": "minecraft:crafting_shapeless", "category": "weapons", "ingredients": ["minecraft:stone"], "result": {"id": "minecraft:stone_slab"}})"###,
            "`category` `weapons`",
        ),
        (
            r###"datapack.recipe("a", {"type": "minecraft:smelting", "ingredient": "minecraft:stone", "cookingtime": "200", "result": {"id": "minecraft:stone_slab"}})"###,
            "`cookingtime` is a number",
        ),
        (
            &format!(
                r###"datapack.recipe("a", {{"type": "minecraft:crafting_shapeless", "ingredients": [{}], "result": {SLAB}}})"###,
                [r#""minecraft:stone""#; 10].join(", ")
            ),
            "1 to 9",
        ),
    ]);
    // A transmute result may leave out its id.
    builds(
        r###"datapack.recipe("a", {"type": "minecraft:crafting_transmute", "input": "#minecraft:bundles", "material": "minecraft:black_dye", "result": {"components": {}}})"###,
    );
}

fn shaped(pattern: &str, key: &str) -> String {
    format!(
        r###"datapack.recipe("a", {{"type": "minecraft:crafting_shaped", "pattern": {pattern}, "key": {key}, "result": {SLAB}}})"###
    )
}

#[test]
fn shaped_patterns_match_their_key() {
    assert_errors(&[
        (
            &shaped(r###"["#X"]"###, r###"{"#": "minecraft:stone"}"###),
            "`pattern` uses `X`, which `key` doesn't define",
        ),
        (
            &shaped(
                r###"["#"]"###,
                r###"{"#": "minecraft:stone", "X": "minecraft:dirt"}"###,
            ),
            "`key.X` is not used in `pattern`",
        ),
        (
            &shaped(r###"["##", "#"]"###, r###"{"#": "minecraft:stone"}"###),
            "rows must all be as wide",
        ),
        (
            &shaped(r#####"["####"]"#####, r###"{"#": "minecraft:stone"}"###),
            "1 to 3 wide",
        ),
        (
            &shaped(
                r###"["##"]"###,
                r###"{"#": "minecraft:stone", " ": "minecraft:dirt"}"###,
            ),
            "can't define \" \"",
        ),
    ]);
    builds(&shaped(
        r####"["# #", "###"]"####,
        r###"{"#": "minecraft:stone"}"###,
    ));
}

const IMPOSSIBLE: &str = r###"{"a": {"trigger": "minecraft:impossible"}}"###;

#[test]
fn advancements_need_their_fields_and_requirements() {
    let icon = r###"{"id": "minecraft:stone"}"###;
    assert_errors(&[
        (
            &format!(
                r###"datapack.advancement("a", {{"criteria": {IMPOSSIBLE}, "requirements": [["b"]]}})"###
            ),
            "names the criterion `b`",
        ),
        (
            r###"datapack.advancement("a", {"parent": "minecraft:story/root"})"###,
            "`criteria` is missing",
        ),
        (
            r###"datapack.advancement("a", {"criteria": {}})"###,
            "`criteria` is empty",
        ),
        (
            r###"datapack.advancement("a", {"criteria": {"a": {}}})"###,
            "`criteria.a.trigger` is missing",
        ),
        (
            &format!(
                r###"datapack.advancement("a", {{"parent": "minecraft:story/root", "display": {{"icon": {icon}, "description": "d"}}, "criteria": {IMPOSSIBLE}}})"###
            ),
            "`display.title` is missing",
        ),
        (
            &format!(
                r###"datapack.advancement("a", {{"parent": "minecraft:story/root", "display": {{"icon": {icon}, "title": "t"}}, "criteria": {IMPOSSIBLE}}})"###
            ),
            "`display.description` is missing",
        ),
        (
            &format!(
                r###"datapack.advancement("a", {{"parent": "minecraft:story/root", "display": {{"title": "t", "description": "d"}}, "criteria": {IMPOSSIBLE}}})"###
            ),
            "`display.icon` is missing",
        ),
        (
            &format!(
                r###"datapack.advancement("a", {{"parent": "minecraft:story/root", "display": {{"icon": {icon}, "title": "t", "description": "d", "frame": "quest"}}, "criteria": {IMPOSSIBLE}}})"###
            ),
            "`display.frame` `quest`",
        ),
        (
            &format!(
                r###"datapack.advancement("a", {{"criteria": {IMPOSSIBLE}, "requirements": ["a"]}})"###
            ),
            "`requirements[0]` must be a list",
        ),
        (
            r###"datapack.advancement("a", {"criteria": {"a": {"trigger": "minecraft:impossible"}, "b": {"trigger": "minecraft:impossible"}}, "requirements": [["a"]]})"###,
            "the criterion `b` is in no list",
        ),
        (
            &format!(
                r###"datapack.advancement("a", {{"parent": "minecraft:story/root", "display": {{"icon": {{"id": "minecraft:diamon"}}, "title": "t", "description": "d"}}, "criteria": {IMPOSSIBLE}}})"###
            ),
            "`minecraft:diamon` is not an item",
        ),
        (
            &format!(
                r###"datapack.advancement("a", {{"parent": "sim:nope", "criteria": {IMPOSSIBLE}}})"###
            ),
            "the advancement `sim:nope` at `parent`",
        ),
        (
            r###"datapack.advancement("a", {"criteria": {"a": {"trigger": "minecraft:player_killed_entity", "conditions": {"entity": {"flags": {"is_sneaking": True}}}}}})"###,
            "is now a predicate with a `type`",
        ),
    ]);
}

#[test]
fn ignored_entity_conditions_and_a_null_display_build() {
    // Bug 78: the server ignores `entity` on a trigger without one.
    builds(
        r###"datapack.advancement("a", {"criteria": {"a": {"trigger": "minecraft:tick", "conditions": {"entity": {"flags": {"is_sneaking": True}}}}}})"###,
    );
    // Bug 79: null is no display.
    builds(&format!(
        r###"datapack.advancement("a", {{"display": None, "criteria": {IMPOSSIBLE}}})"###
    ));
    // References to the pack's own resources, declared later, are found.
    builds(&format!(
        r###"datapack.advancement("a", {{"parent": "sim:b", "criteria": {IMPOSSIBLE}, "rewards": {{"loot": ["sim:t"]}}}})
datapack.advancement("b", {{"criteria": {IMPOSSIBLE}}})
datapack.loot_table("t", {{"pools": []}})"###
    ));
}

#[test]
fn references_to_undeclared_resources_are_errors() {
    assert_errors(&[
        (
            r###"datapack.dialog_tag("minecraft:pause_screen_additions", ["sim:notice"])"###,
            "names the dialog `sim:notice`",
        ),
        (
            &format!(
                r###"datapack.loot_table("a", {{"pools": [{{"rolls": 1, "condition": "sim:nope", "entries": [{STONE}]}}]}})"###
            ),
            "the predicate `sim:nope` at `pools[0].condition`",
        ),
        (
            &pool(
                r###"{"type": "minecraft:item", "name": "minecraft:stone", "modifier": "sim:nope"}"###,
            ),
            "the item modifier `sim:nope`",
        ),
        (
            &pool(r###"{"type": "minecraft:loot_table", "value": "sim:nope"}"###),
            "the loot table `sim:nope`",
        ),
        (
            &format!(
                r###"datapack.advancement("a", {{"criteria": {IMPOSSIBLE}, "rewards": {{"loot": ["sim:nope"]}}}})"###
            ),
            "the loot table `sim:nope` at `rewards.loot[0]`",
        ),
        (
            r###"datapack.advancement("a", {"criteria": {"a": {"trigger": "minecraft:recipe_unlocked", "conditions": {"recipes": ["sim:nope"]}}}})"###,
            "the recipe `sim:nope`",
        ),
        (
            r###"datapack.dialog("a", {"type": "minecraft:dialog_list", "title": "t", "dialogs": ["sim:nope"]})"###,
            "the dialog `sim:nope` at `dialogs[0]`",
        ),
        (
            r###"datapack.dialog("a", {"type": "minecraft:notice", "title": "t", "action": {"label": "l", "action": {"type": "minecraft:show_dialog", "dialog": "sim:nope"}}})"###,
            "the dialog `sim:nope` at `action.action.dialog`",
        ),
    ]);
    // An optional entry may be missing, and other namespaces aren't checked.
    builds(
        r###"datapack.dialog_tag("minecraft:pause_screen_additions", [{"id": "sim:notice", "required": False}, "othermod:notice"])"###,
    );
    builds(&pool(
        r###"{"type": "minecraft:loot_table", "value": "othermod:chests/a"}"###,
    ));
}

#[test]
fn tags_must_exist_and_not_include_themselves() {
    assert_errors(&[
        (
            r###"datapack.block_tag("a", ["#sim:a", "minecraft:stone"])"###,
            "includes itself (#sim:a -> #sim:a)",
        ),
        (
            r###"datapack.block_tag("a", ["#sim:b"])
datapack.block_tag("b", ["#sim:a"])"###,
            "includes itself (#sim:a -> #sim:b -> #sim:a)",
        ),
        (
            r###"datapack.block_tag("a", ["#minecraft:log"])"###,
            "names the block tag `#minecraft:log`, which neither Minecraft nor the pack defines",
        ),
        (
            r###"datapack.item_tag("only_item", ["minecraft:stone"])
datapack.block_tag("b", ["#sim:only_item"])"###,
            "names the block tag `#sim:only_item`, which the pack doesn't define",
        ),
    ]);
    builds(
        r###"datapack.block_tag("a", ["#minecraft:logs", {"id": "#sim:nope", "required": False}, "#othermod:ores"])
datapack.block_tag("b", ["#sim:a"])"###,
    );
}

#[test]
fn function_tag_entries_must_be_functions_of_the_pack() {
    for (declarations, expected) in [
        (
            r###"datapack.function_tag("minecraft:load", ["sim:setpu"])"###,
            "names the function `sim:setpu`, which the pack doesn't define",
        ),
        (
            r###"datapack.function_tag("minecraft:load", ["setup"])"###,
            "write `sim:setup` for this pack's function",
        ),
        (
            r###"datapack.function_tag("t", ["#sim:nope"])"###,
            "names the function tag `#sim:nope`",
        ),
    ] {
        let error = error(&format!(
            "{declarations}\n\ndef setup():\n    /say setup-ran\n"
        ));
        assert!(error.contains(expected), "{declarations}: {error}");
    }
    builds(
        r###"datapack.function_tag("minecraft:load", ["sim:setup", "#sim:more", "othermod:init"])
datapack.function_tag("more", ["sim:setup"])

def setup():
    /say setup-ran
"###,
    );
}

/// `cobble check --json --namespace sim` of a file holding `declarations`:
/// the diagnostics, and whether the check passed.
fn check_json(declarations: &str) -> (Vec<serde_json::Value>, bool) {
    let temp_dir = tempfile::TempDir::new().unwrap();
    let file = temp_dir.path().join("main.cbl");
    std::fs::write(
        &file,
        format!("from stdlib import datapack\n\n{declarations}\n"),
    )
    .unwrap();
    let output = std::process::Command::new(env!("CARGO_BIN_EXE_cobble"))
        .args(["check", "--json", "--namespace", "sim"])
        .arg(&file)
        .output()
        .unwrap();
    let report: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    let diagnostics = report["diagnostics"]
        .as_array()
        .cloned()
        .unwrap_or_default();
    (diagnostics, output.status.success())
}

#[test]
fn references_into_another_packs_namespace_stay_unchecked() {
    // Regression: the ICY105_Mechanization import names tags of its
    // dependency packs, in namespaces it also declares something in.
    builds(
        r###"datapack.function_tag("xlib:v1/hook", ["sim:a"])
datapack.function_tag("minecraft:load", ["#xlib:v1/internal/wipe"])
datapack.predicate("xlib:here", {"type": "minecraft:random_chance", "chance": 0.5})
datapack.loot_table("a", {"pools": [{"rolls": 1, "condition": "xlib:elsewhere", "entries": [{"type": "minecraft:tag", "items": "#xlib:fuels", "expand": True}]}]})
datapack.item_tag("xlib:fuels", ["minecraft:coal"])
datapack.block_tag("b", ["#xlib:ores", "xlib:ore"])

def a():
    /say hooked
"###,
    );
    // The pack's own namespace is still checked.
    assert_errors(&[(
        r###"datapack.function_tag("minecraft:load", ["#sim:v1/wipe"])"###,
        "names the function tag `#sim:v1/wipe`, which the pack doesn't define",
    )]);
    // `cobble check` takes the namespace `cobble build` does.
    let (diagnostics, ok) =
        check_json(r###"datapack.function_tag("minecraft:load", ["#sim:nope"])"###);
    assert!(!ok, "{diagnostics:?}");
}

#[test]
fn optional_tag_entries_may_name_the_tag_itself() {
    builds(
        r###"datapack.block_tag("a", [{"id": "#sim:a", "required": False}, "minecraft:stone"])
datapack.item_tag("a", [{"id": "#sim:a", "required": False}, "minecraft:stone"])
datapack.function_tag("a", [{"id": "#sim:a", "required": False}, "sim:go"])
datapack.block_tag("c", [{"id": "#sim:d", "required": False}])
datapack.block_tag("d", ["#sim:c"])

def go():
    /say ran
"###,
    );
    // A tag entry in the pack's own namespace names a block or an item
    // that can't exist.
    assert_errors(&[
        (
            r###"datapack.block_tag("a", ["sim:nope"])"###,
            "The block tag `sim:a` names `sim:nope`, which doesn't exist",
        ),
        (
            r###"datapack.item_tag("a", ["sim:nope"])"###,
            "a data pack can't add items",
        ),
    ]);
    builds(r###"datapack.item_tag("a", [{"id": "sim:nope", "required": False}])"###);
}

#[test]
fn a_tag_entry_names_its_items_like_any_item_list() {
    builds(&pool(
        r###"{"type": "minecraft:tag", "items": "minecraft:stone", "expand": True}"###,
    ));
    builds(&pool(
        r###"{"type": "minecraft:tag", "items": ["minecraft:stone", "minecraft:dirt"], "expand": True}"###,
    ));
    builds(&pool(
        r###"{"type": "minecraft:tag", "items": "#minecraft:logs"}"###,
    ));
    assert_errors(&[
        (
            &pool(r###"{"type": "minecraft:tag", "items": ["#minecraft:logs"], "expand": True}"###),
            "a list holds ids only",
        ),
        (
            &pool(r###"{"type": "minecraft:tag", "items": "#minecraft:nope", "expand": True}"###),
            "the item tag `#minecraft:nope`",
        ),
    ]);
}

const FISHING: &str = r###"datapack.advancement("a", {"criteria": {"a": {"trigger": "minecraft:fishing_rod_hooked", "conditions": {"entity": ENTITY}}}})"###;

#[test]
fn every_trigger_reads_its_own_entity_conditions() {
    // Regression: fishing_rod_hooked has an `entity`.
    assert_errors(&[
        (
            &FISHING.replace("ENTITY", "{}"),
            "`criteria.a.conditions.entity` is now a predicate with a `type`",
        ),
        (
            &FISHING.replace("ENTITY", r###"{"flags": {"is_baby": True}}"###),
            "is now a predicate with a `type`",
        ),
        (
            &FISHING.replace("ENTITY", "[]"),
            "`criteria.a.conditions.entity` is now one predicate, not a list",
        ),
        (
            r###"datapack.advancement("a", {"criteria": {"a": {"trigger": "minecraft:bred_animals", "conditions": {"child": {"type": "minecraft:entity_properties"}}}}})"###,
            "`criteria.a.conditions.child.entity` is missing",
        ),
        (
            r###"datapack.advancement("a", {"criteria": {"a": {"trigger": "minecraft:tick", "conditions": {"player": {"flags": {"is_sneaking": True}}}}}})"###,
            "`criteria.a.conditions.player` is now a predicate with a `type`",
        ),
    ]);
    // A wrapped predicate there builds without a warning, an `entity` on a
    // trigger without one is ignored with a warning, and `impossible` reads
    // no conditions at all.
    let wrapped = FISHING.replace(
        "ENTITY",
        r###"{"type": "minecraft:entity_properties", "entity": "this", "predicate": {"flags": {"is_baby": True}}}"###,
    );
    let (diagnostics, ok) = check_json(&wrapped);
    assert!(ok && diagnostics.is_empty(), "{diagnostics:?}");
    let (diagnostics, ok) = check_json(
        r###"datapack.advancement("a", {"criteria": {"a": {"trigger": "minecraft:impossible", "conditions": {"player": {}}}, "b": {"trigger": "minecraft:tick", "conditions": {"entity": {}}}}})"###,
    );
    assert!(ok, "{diagnostics:?}");
    let messages: Vec<&str> = diagnostics
        .iter()
        .map(|diagnostic| diagnostic["message"].as_str().unwrap())
        .collect();
    assert_eq!(messages.len(), 2, "{messages:?}");
    assert!(
        messages[0].contains(
            "`criteria.a.conditions.player` is ignored: `minecraft:impossible` has no `player` condition"
        ),
        "{messages:?}"
    );
    assert!(
        messages[1].contains("`criteria.b.conditions.entity` is ignored"),
        "{messages:?}"
    );
}

#[test]
fn none_in_a_required_field_is_missing() {
    let display = r###"{"icon": {"id": "minecraft:stone"}, "title": "t", "description": "d"}"###;
    assert_errors(&[
        (
            &format!(
                r###"datapack.loot_table("a", {{"pools": [{{"rolls": None, "entries": [{STONE}]}}]}})"###
            ),
            "`pools[0].rolls` is missing",
        ),
        (
            r###"datapack.loot_table("a", {"pools": [{"rolls": 1, "entries": None}]})"###,
            "`pools[0].entries` is missing",
        ),
        (
            &pool(r###"{"type": "minecraft:item", "name": None}"###),
            "`pools[0].entries[0].name` is missing",
        ),
        (
            &pool(r###"{"type": "minecraft:tag", "items": None}"###),
            "`pools[0].entries[0].items` is missing",
        ),
        (
            &pool(r###"{"type": "minecraft:loot_table", "value": None}"###),
            "`pools[0].entries[0].value` is missing",
        ),
        (
            r###"datapack.predicate("a", {"type": "minecraft:random_chance", "chance": None})"###,
            "`chance` is missing",
        ),
        (
            r###"datapack.predicate("a", {"type": "minecraft:all_of", "terms": None})"###,
            "`terms` is missing",
        ),
        (
            r###"datapack.predicate("a", {"type": "minecraft:time_check", "clock": None, "value": 0})"###,
            "`clock` is missing",
        ),
        (
            r###"datapack.item_modifier("a", {"type": "minecraft:set_count", "count": None})"###,
            "`count` is missing",
        ),
        (
            r###"datapack.item_modifier("a", {"type": "minecraft:set_lore", "lore": [], "mode": None})"###,
            "`mode` is missing",
        ),
        (
            &stonecutting(r###""minecraft:stone""###, "None"),
            "`result` is missing",
        ),
        (
            r###"datapack.recipe("a", {"type": "minecraft:crafting_shaped", "pattern": None, "key": {"#": "minecraft:stone"}, "result": {"id": "minecraft:stone"}})"###,
            "`pattern` is missing",
        ),
        (
            r###"datapack.advancement("a", {"criteria": None})"###,
            "`criteria` is missing",
        ),
        (
            r###"datapack.advancement("a", {"criteria": {"a": {"trigger": None}}})"###,
            "`criteria.a.trigger` is missing",
        ),
        (
            &format!(
                r###"datapack.advancement("a", {{"parent": "minecraft:story/root", "criteria": {IMPOSSIBLE}, "display": {{"icon": {{"id": "minecraft:stone"}}, "title": None, "description": "d"}}}})"###
            ),
            "`display.title` is missing",
        ),
        (
            &format!(
                r###"datapack.advancement("a", {{"parent": "minecraft:story/root", "criteria": {IMPOSSIBLE}, "display": {{"icon": None, "title": "t", "description": "d"}}}})"###
            ),
            "`display.icon` is missing",
        ),
        // A root with a display needs a background, and `None` is none.
        (
            &format!(
                r###"datapack.advancement("a", {{"criteria": {IMPOSSIBLE}, "display": {{"icon": {{"id": "minecraft:stone"}}, "title": "t", "description": "d", "background": None}}}})"###
            ),
            "must set `display.background`",
        ),
        (
            &format!(
                r###"datapack.advancement("a", {{"parent": None, "criteria": {IMPOSSIBLE}, "display": {display}}})"###
            ),
            "must set `display.background`",
        ),
    ]);
    // `None` in a field that may be left out is the field left out.
    builds(&format!(
        r###"datapack.advancement("a", {{"parent": "minecraft:story/root", "criteria": {IMPOSSIBLE}, "display": {display}, "rewards": None}})"###
    ));
}

#[test]
fn fields_hold_the_kind_of_value_minecraft_reads() {
    assert_errors(&[
        (
            &format!(
                r###"datapack.loot_table("a", {{"pools": [{{"rolls": "x", "entries": [{STONE}]}}]}})"###
            ),
            "`pools[0].rolls` `x` is not a number provider",
        ),
        (
            &format!(
                r###"datapack.loot_table("a", {{"pools": [{{"rolls": [1], "entries": [{STONE}]}}]}})"###
            ),
            "`pools[0].rolls` is a list, but it must be a number",
        ),
        (
            &format!(
                r###"datapack.loot_table("a", {{"pools": [{{"rolls": {{"min": 1, "max": 2}}, "entries": [{STONE}]}}]}})"###
            ),
            "`pools[0].rolls` is a number provider without a `type`",
        ),
        (
            r###"datapack.predicate("a", {"type": "minecraft:random_chance", "chance": {"min": 0, "max": 1}})"###,
            "`chance` is a number provider without a `type`",
        ),
        (
            &format!(
                r###"datapack.loot_table("a", {{"pools": [{{"rolls": 1, "condition": 5, "entries": [{STONE}]}}]}})"###
            ),
            "`pools[0].condition` is the number 5, but it must be one predicate",
        ),
        (
            r###"datapack.loot_table("a", {"pools": [{"rolls": 1, "entries": {}}]})"###,
            "`pools[0].entries` is an object, but it must be a list",
        ),
        (
            r###"datapack.predicate("a", {"type": "minecraft:all_of", "terms": [[{"type": "minecraft:random_chance", "chance": 0.5}]]})"###,
            "`terms[0]` is one predicate, not a list",
        ),
        (
            r###"datapack.predicate("a", {"type": "minecraft:any_of", "terms": [[]]})"###,
            "`terms[0]` is one predicate, not a list",
        ),
        (
            r###"datapack.advancement("a", {"criteria": {"a": {"trigger": "minecraft:placed_block", "conditions": {"location": [{"type": "minecraft:random_chance", "chance": 0.5}]}}}})"###,
            "`criteria.a.conditions.location` is one predicate, not a list",
        ),
        (
            r###"datapack.advancement("a", {"criteria": {"a": {"trigger": "minecraft:placed_block", "conditions": {"location": []}}}})"###,
            "`criteria.a.conditions.location` is one predicate, not a list",
        ),
        (
            &pool(r###"{"type": "minecraft:item", "name": "minecraft:stone", "weight": "2"}"###),
            "`pools[0].entries[0].weight` is the text \"2\", but it must be a number",
        ),
        (
            r###"datapack.predicate("a", {"type": "minecraft:weather_check", "raining": 1})"###,
            "`raining` is the number 1, but it must be True or False",
        ),
        (
            r###"datapack.predicate("a", {"type": "minecraft:entity_scores", "entity": "this", "scores": []})"###,
            "`scores` is a list, but it must be an object",
        ),
        (
            r###"datapack.predicate("a", {"type": "minecraft:table_bonus", "enchantment": "minecraft:fortune", "chances": []})"###,
            "`chances` is an empty list",
        ),
        (
            r###"datapack.item_modifier("a", {"type": "minecraft:set_name", "name": 5})"###,
            "`name` is the number 5, but it must be a text component",
        ),
        (
            r###"datapack.recipe("a", {"type": "minecraft:crafting_shaped", "pattern": "#", "key": {"#": "minecraft:stone"}, "result": {"id": "minecraft:stone"}})"###,
            "`pattern` is the text \"#\", but it must be a list",
        ),
        (
            r###"datapack.recipe("a", {"type": "minecraft:crafting_shapeless", "category": 1, "ingredients": ["minecraft:stone"], "result": {"id": "minecraft:stone"}})"###,
            "`category` is the number 1, but it must be one of",
        ),
        (
            &stonecutting(
                r###""minecraft:stone""###,
                r###"{"id": "minecraft:stone_slab", "count": "2"}"###,
            ),
            "`result.count` is the text \"2\", but it must be a number",
        ),
        (
            r###"datapack.advancement("a", {"criteria": {"a": {"trigger": 5}}})"###,
            "`criteria.a.trigger` is the number 5, but it must be the id of an advancement trigger",
        ),
        (
            r###"datapack.advancement("a", {"criteria": []})"###,
            "`criteria` is a list, but it must be an object of criteria",
        ),
        (
            r###"datapack.advancement("a", {"criteria": {"a": []}})"###,
            "`criteria.a` is a list, but it must be a criterion object",
        ),
        (
            &format!(
                r###"datapack.advancement("a", {{"criteria": {IMPOSSIBLE}, "requirements": "a"}})"###
            ),
            "`requirements` is the text \"a\", but it must be a list of lists",
        ),
        (
            &format!(
                r###"datapack.advancement("a", {{"parent": "minecraft:story/root", "criteria": {IMPOSSIBLE}, "display": {{"icon": {{"id": "minecraft:stone"}}, "title": "t", "description": "d", "frame": 1}}}})"###
            ),
            "`display.frame` is the number 1, but it must be text",
        ),
        (
            &format!(
                r###"datapack.advancement("a", {{"parent": "minecraft:story/root", "criteria": {IMPOSSIBLE}, "display": {{"icon": {{"count": 1}}, "title": "t", "description": "d"}}}})"###
            ),
            "`display.icon.id` is missing",
        ),
        (
            &format!(
                r###"datapack.advancement("a", {{"parent": "minecraft:story/root", "criteria": {IMPOSSIBLE}, "display": {{"icon": {{"id": "minecraft:stone"}}, "title": 5, "description": "d"}}}})"###
            ),
            "`display.title` is the number 5, but it must be a text component",
        ),
        (
            r###"datapack.recipe("a", {"type": "minecraft:brewing", "input": {"item": "minecraft:potion"}, "reagent": "minecraft:gunpowder", "output": {"id": "minecraft:splash_potion"}})"###,
            "`reagent` is the text \"minecraft:gunpowder\", but it must be an object",
        ),
        (
            r###"datapack.recipe("a", {"type": "minecraft:brewing", "input": {"tag": "minecraft:logs"}, "reagent": {"item": "minecraft:gunpowder"}, "output": {"id": "minecraft:splash_potion"}})"###,
            "`input.item` is missing",
        ),
    ]);
    // The forms Minecraft takes.
    builds(&format!(
        r###"datapack.loot_table("a", {{"pools": [{{"rolls": {{"type": "minecraft:uniform", "min": 1, "max": 2}}, "bonus_rolls": 0.5, "entries": [{STONE}]}}, {{"rolls": "minecraft:compostable/low", "entries": []}}]}})
datapack.predicate("b", {{"type": "minecraft:random_chance", "chance": {{"type": "minecraft:uniform", "min": 0, "max": 1}}}})
datapack.recipe("c", {{"type": "minecraft:stonecutting", "ingredient": "minecraft:stone", "result": "minecraft:stone_slab"}})
datapack.recipe("d", {{"type": "minecraft:brewing", "input": {{"item": "#minecraft:logs"}}, "reagent": {{"item": "minecraft:gunpowder"}}, "output": "minecraft:splash_potion"}})
datapack.recipe("e", {{"type": "minecraft:smithing_transform", "base": "minecraft:diamond_sword", "result": {{"id": "minecraft:netherite_sword"}}}})
datapack.advancement("f", {{"parent": "minecraft:story/root", "criteria": {IMPOSSIBLE}, "display": {{"icon": "minecraft:stone", "title": ["t"], "description": {{"text": "d"}}}}}})"###
    ));
}

#[test]
fn types_need_the_fields_minecraft_reads() {
    for (kind, missing) in [
        ("copy_components", "source"),
        ("copy_custom_data", "source"),
        ("copy_state", "block"),
        ("enchanted_count_increase", "enchantment"),
        ("exploration_map", "destination"),
        ("fill_player_head", "entity"),
        ("filtered", "item_filter"),
        ("modify_contents", "component"),
        ("set_attributes", "modifiers"),
        ("set_banner_pattern", "patterns"),
        ("set_contents", "component"),
        ("set_instrument", "options"),
        ("set_loot_table", "loot_table_id"),
        ("set_ominous_bottle_amplifier", "amplifier"),
        ("set_random_dyes", "number_of_dyes"),
        ("set_writable_book_pages", "pages"),
        ("set_written_book_pages", "pages"),
    ] {
        let error = error(&format!(
            r###"datapack.item_modifier("a", {{"type": "minecraft:{kind}"}})"###
        ));
        assert!(
            error.contains(&format!("`{missing}` is missing")),
            "{kind}: {error}"
        );
    }
    for (kind, missing) in [
        ("float_value_check", "value"),
        ("environment_attribute_check", "attribute"),
        ("random_chance_with_enchanted_bonus", "enchantment"),
        ("entity_scores", "scores"),
    ] {
        let error = error(&format!(
            r###"datapack.predicate("a", {{"type": "minecraft:{kind}"}})"###
        ));
        assert!(
            error.contains(&format!("`{missing}` is missing")),
            "{kind}: {error}"
        );
    }
    for (kind, missing) in [
        ("stonecutting", "ingredient"),
        ("smelting", "ingredient"),
        ("crafting_shapeless", "ingredients"),
        ("smithing_transform", "base"),
        ("crafting_dye", "target"),
        ("crafting_shaped", "pattern"),
        ("crafting_shaped", "key"),
        ("crafting_special_bannerduplicate", "result"),
        ("smithing_trim", "pattern"),
    ] {
        let error = error(&format!(
            r###"datapack.recipe("a", {{"type": "minecraft:{kind}"}})"###
        ));
        assert!(
            error.contains(&format!("`{missing}` is missing")),
            "{kind}: {error}"
        );
    }
    assert_errors(&[
        (
            &pool(r###"{"type": "minecraft:dynamic"}"###),
            "`pools[0].entries[0].name` is missing",
        ),
        (
            &pool(r###"{"type": "minecraft:slots"}"###),
            "`pools[0].entries[0].slot_source` is missing",
        ),
        (
            r###"datapack.item_modifier("a", {"type": "minecraft:modify_contents", "component": "minecraft:container", "modifier": {"function": "minecraft:set_count", "count": 1}})"###,
            "`modifier.function` is now `type`",
        ),
        (
            r###"datapack.advancement("a", {"criteria": {"a": {"trigger": "minecraft:placed_block", "conditions": {"location": {"type": "minecraft:random_chance"}}}}})"###,
            "`criteria.a.conditions.location.chance` is missing",
        ),
        (
            r###"datapack.advancement("a", {"criteria": {"a": {"trigger": "minecraft:placed_block", "conditions": {"location": {"type": "minecraft:all_of"}}}}})"###,
            "`criteria.a.conditions.location.terms` is missing",
        ),
        (
            r###"datapack.advancement("a", {"criteria": {"a": {"trigger": "minecraft:item_used_on_block", "conditions": {"location": {"type": "minecraft:time_check", "value": 0}}}}})"###,
            "`criteria.a.conditions.location.clock` is missing",
        ),
        (
            r###"datapack.advancement("a", {"criteria": {"a": {"trigger": "minecraft:recipe_unlocked", "conditions": {"recipe": "minecraft:stone"}}}})"###,
            "`criteria.a.conditions.recipes` is missing",
        ),
        (
            r###"datapack.predicate("a", {"type": "minecraft:entity_properties", "entity": "this", "predicate": {"type": "minecraft:zombie"}})"###,
            "`predicate.type` is not a part of an entity predicate",
        ),
        (
            r###"datapack.recipe("a", {"type": "minecraft:crafting_shaped", "pattern": ["   "], "key": {}, "result": {"id": "minecraft:stone"}})"###,
            "`pattern` is all spaces",
        ),
        (
            r###"datapack.recipe("a", {"type": "minecraft:crafting_shaped", "pattern": [" "], "key": {}, "result": {"id": "minecraft:stone"}})"###,
            "`pattern` is all spaces",
        ),
        (
            r###"datapack.recipe("a", {"type": "minecraft:crafting_dye", "category": "weapons", "target": "minecraft:white_wool", "dye": "minecraft:red_dye", "result": {"id": "minecraft:red_wool"}})"###,
            "`category` `weapons` is not a category of this recipe",
        ),
        (
            r###"datapack.recipe("a", {"type": "minecraft:crafting_imbue", "category": "weapons", "source": "minecraft:arrow", "material": "minecraft:lingering_potion", "result": {"id": "minecraft:tipped_arrow"}})"###,
            "`category` `weapons` is not a category of this recipe",
        ),
        (
            r###"datapack.recipe("a", {"type": "minecraft:crafting_special_repairitem"})"###,
            "Minecraft holds once",
        ),
    ]);
    // Minecraft's own repair recipe can be replaced, a category another
    // type doesn't read is ignored, and a blank row beside a real one is
    // fine.
    builds(
        r###"datapack.recipe("minecraft:repair_item", {"type": "minecraft:crafting_special_repairitem"})
datapack.recipe("a", {"type": "minecraft:stonecutting", "category": "weapons", "ingredient": "minecraft:stone", "result": {"id": "minecraft:stone_slab"}})
datapack.recipe("b", {"type": "minecraft:crafting_dye", "category": "equipment", "target": "minecraft:white_wool", "dye": "minecraft:red_dye", "result": {"id": "minecraft:red_wool"}})
datapack.recipe("c", {"type": "minecraft:crafting_shaped", "pattern": ["#", " "], "key": {"#": "minecraft:stone"}, "result": {"id": "minecraft:stone"}})
datapack.predicate("d", {"type": "minecraft:entity_properties", "entity": "this", "predicate": {"flags": {"is_sneaking": True}}})
datapack.item_modifier("e", {"type": "minecraft:set_enchantments"})"###,
    );
}

#[test]
fn ids_are_looked_up_in_every_nested_field() {
    assert_errors(&[
        (
            r###"datapack.item_modifier("a", {"type": "minecraft:set_item", "item": "minecraft:diamon"})"###,
            "`item` `minecraft:diamon` is not an item",
        ),
        (
            r###"datapack.predicate("a", {"type": "minecraft:match_block", "blocks": "minecraft:ston"})"###,
            "`blocks` `minecraft:ston` is not a block",
        ),
        (
            r###"datapack.predicate("a", {"type": "minecraft:match_block", "blocks": "#minecraft:nope"})"###,
            "the block tag `#minecraft:nope`",
        ),
        (
            r###"datapack.predicate("a", {"type": "minecraft:entity_properties", "entity": "this", "predicate": {"location": {"block": {"blocks": "minecraft:ston"}}}})"###,
            "`predicate.location.block.blocks` `minecraft:ston` is not a block",
        ),
        (
            r###"datapack.predicate("a", {"type": "minecraft:entity_properties", "entity": "this", "predicate": {"equipment": {"mainhand": {"items": "minecraft:diamon"}}}})"###,
            "`predicate.equipment.mainhand.items` `minecraft:diamon` is not an item",
        ),
        (
            r###"datapack.predicate("a", {"type": "minecraft:entity_properties", "entity": "this", "predicate": {"vehicle": {"minecraft:entity_type": "minecraft:zombi"}}})"###,
            "`minecraft:zombi` is not an entity type",
        ),
        (
            r###"datapack.predicate("a", {"type": "minecraft:location_check", "predicate": {"fluid": {"fluids": "minecraft:wate"}}})"###,
            "`predicate.fluid.fluids` `minecraft:wate` is not a fluid",
        ),
        (
            r###"datapack.predicate("a", {"type": "minecraft:location_check", "predicate": {"biomes": "minecraft:nope"}})"###,
            "`minecraft:nope` is not a biome",
        ),
        (
            r###"datapack.predicate("a", {"type": "minecraft:location_check", "predicate": {"biomes": "#minecraft:nope"}})"###,
            "names the tag `#minecraft:nope`, which Minecraft doesn't define",
        ),
        (
            r###"datapack.predicate("a", {"type": "minecraft:time_check", "clock": "minecraft:nope", "value": 0})"###,
            "`clock` `minecraft:nope` is not a world clock",
        ),
        (
            r###"datapack.predicate("a", {"type": "minecraft:damage_source_properties", "predicate": {"tags": [{"id": "minecraft:nope", "expected": True}]}})"###,
            "`minecraft:nope` is not a damage type",
        ),
        (
            r###"datapack.recipe("a", {"type": "minecraft:brewing", "input": {"item": "minecraft:potio"}, "reagent": {"item": "minecraft:gunpowder"}, "output": {"id": "minecraft:splash_potion"}})"###,
            "`input.item` `minecraft:potio` is not an item",
        ),
        (
            r###"datapack.recipe("a", {"type": "minecraft:brewing", "input": {"item": "minecraft:potion"}, "reagent": {"item": "minecraft:gunpowder"}, "output": {"id": "minecraft:splash_potio"}})"###,
            "`output.id` `minecraft:splash_potio` is not an item",
        ),
        (
            r###"datapack.recipe("a", {"type": "minecraft:smithing_trim", "template": "minecraft:stone", "base": "minecraft:stone", "addition": "minecraft:stone", "pattern": "minecraft:nope"})"###,
            "`pattern` `minecraft:nope` is not a trim pattern",
        ),
        (
            r###"datapack.item_modifier("a", {"type": "minecraft:enchant_with_levels", "levels": 1, "options": "minecraft:sharpnes"})"###,
            "`options` `minecraft:sharpnes` is not an enchantment",
        ),
        (
            r###"datapack.item_modifier("a", {"type": "minecraft:set_enchantments", "enchantments": {"minecraft:sharpnes": 1}})"###,
            "`minecraft:sharpnes` is not an enchantment",
        ),
        (
            r###"datapack.item_modifier("a", {"type": "minecraft:set_potion", "id": "minecraft:nope"})"###,
            "`id` `minecraft:nope` is not a potion",
        ),
        (
            r###"datapack.item_modifier("a", {"type": "minecraft:set_instrument", "options": "minecraft:nope"})"###,
            "`options` `minecraft:nope` is not an instrument",
        ),
        (
            r###"datapack.item_modifier("a", {"type": "minecraft:set_stew_effect", "effects": [{"type": "minecraft:sped", "duration": 1}]})"###,
            "`effects[0].type` `minecraft:sped` is not an effect",
        ),
        (
            r###"datapack.item_modifier("a", {"type": "minecraft:set_attributes", "modifiers": [{"id": "a:b", "attribute": "minecraft:nope", "operation": "add_value", "amount": 1, "slot": "any"}]})"###,
            "`modifiers[0].attribute` `minecraft:nope` is not an attribute",
        ),
        (
            r###"datapack.item_modifier("a", {"type": "minecraft:copy_state", "block": "minecraft:ston", "properties": []})"###,
            "`block` `minecraft:ston` is not a block",
        ),
        (
            r###"datapack.item_modifier("a", {"type": "minecraft:set_count", "count": "nope"})"###,
            "`count` `nope` is not a number provider",
        ),
        (
            r###"datapack.advancement("a", {"criteria": {"a": {"trigger": "minecraft:inventory_changed", "conditions": {"items": [{"items": "minecraft:diamon"}]}}}})"###,
            "`criteria.a.conditions.items[0].items` `minecraft:diamon` is not an item",
        ),
        (
            r###"datapack.advancement("a", {"criteria": {"a": {"trigger": "minecraft:inventory_changed", "conditions": {"items": [{"items": ["#minecraft:logs"]}]}}}})"###,
            "a list holds ids only",
        ),
        (
            r###"datapack.advancement("a", {"criteria": {"a": {"trigger": "minecraft:inventory_changed", "conditions": {"items": [{"items": "#minecraft:nope"}]}}}})"###,
            "the item tag `#minecraft:nope`",
        ),
        (
            r###"datapack.advancement("a", {"criteria": {"a": {"trigger": "minecraft:enter_block", "conditions": {"blocks": "minecraft:ston"}}}})"###,
            "`minecraft:ston` is not a block",
        ),
        (
            r###"datapack.advancement("a", {"criteria": {"a": {"trigger": "minecraft:effects_changed", "conditions": {"effects": {"minecraft:sped": {}}}}}})"###,
            "`minecraft:sped` is not an effect",
        ),
    ]);
    // Instrument tags and a match_block's block state pass.
    builds(
        r###"datapack.item_modifier("a", {"type": "minecraft:set_instrument", "options": "#minecraft:goat_horns"})
datapack.predicate("b", {"type": "minecraft:match_block", "blocks": "minecraft:stone", "state": {"nope": "x"}})"###,
    );
}

#[test]
fn air_is_no_item_stack_or_ingredient() {
    for declaration in [
        stonecutting(r###""minecraft:stone""###, r###""air""###),
        stonecutting(r###""minecraft:stone""###, r###""minecraft:air""###),
        stonecutting(r###""minecraft:air""###, SLAB),
        stonecutting(r###"["minecraft:stone", "minecraft:air"]"###, SLAB),
        r###"datapack.recipe("a", {"type": "minecraft:crafting_shaped", "pattern": ["#"], "key": {"#": "minecraft:air"}, "result": {"id": "minecraft:stone"}})"###.to_string(),
        r###"datapack.recipe("a", {"type": "minecraft:crafting_shapeless", "ingredients": ["minecraft:air"], "result": {"id": "minecraft:stone"}})"###.to_string(),
        format!(
            r###"datapack.advancement("a", {{"parent": "minecraft:story/root", "criteria": {IMPOSSIBLE}, "display": {{"icon": {{"id": "minecraft:air"}}, "title": "t", "description": "d"}}}})"###
        ),
        r###"datapack.recipe("a", {"type": "minecraft:brewing", "input": {"item": "minecraft:potion"}, "reagent": {"item": "minecraft:gunpowder"}, "output": {"id": "minecraft:air"}})"###.to_string(),
        r###"datapack.item_modifier("a", {"type": "minecraft:set_item", "item": "minecraft:air"})"###.to_string(),
    ] {
        let error = error(&declaration);
        assert!(error.contains("can't be air"), "{declaration}: {error}");
    }
}

#[test]
fn references_are_found_in_every_form_minecraft_reads() {
    assert_errors(&[
        (
            r###"datapack.advancement("a", {"criteria": {"a": {"trigger": "minecraft:recipe_unlocked", "conditions": {"recipes": "sim:nope"}}}})"###,
            "the recipe `sim:nope` at `criteria.a.conditions.recipes`",
        ),
        (
            r###"datapack.advancement("a", {"criteria": {"a": {"trigger": "minecraft:recipe_crafted", "conditions": {"recipes": ["sim:nope"]}}}})"###,
            "the recipe `sim:nope` at `criteria.a.conditions.recipes[0]`",
        ),
        (
            r###"datapack.advancement("a", {"criteria": {"a": {"trigger": "minecraft:crafter_recipe_crafted", "conditions": {"recipes": "sim:nope"}}}})"###,
            "the recipe `sim:nope`",
        ),
        (
            r###"datapack.advancement("a", {"criteria": {"a": {"trigger": "minecraft:player_generates_container_loot", "conditions": {"loot_tables": "sim:nope"}}}})"###,
            "the loot table `sim:nope` at `criteria.a.conditions.loot_tables`",
        ),
        (
            r###"datapack.advancement("a", {"criteria": {"a": {"trigger": "minecraft:player_generates_container_loot", "conditions": {"loot_tables": ["sim:nope"]}}}})"###,
            "the loot table `sim:nope`",
        ),
        (
            &format!(
                r###"datapack.advancement("a", {{"criteria": {IMPOSSIBLE}, "rewards": {{"loot": "sim:nope"}}}})"###
            ),
            "the loot table `sim:nope` at `rewards.loot`",
        ),
        (
            &pool(
                r###"{"type": "minecraft:item", "name": "minecraft:stone", "modifier": ["sim:nope"]}"###,
            ),
            "the item modifier `sim:nope` at `pools[0].entries[0].modifier[0]`",
        ),
        (
            r###"datapack.loot_table("a", {"pools": [], "modifier": "sim:nope"})"###,
            "the item modifier `sim:nope` at `modifier`",
        ),
        (
            r###"datapack.predicate("a", {"type": "minecraft:all_of", "terms": ["sim:nope"]})"###,
            "the predicate `sim:nope` at `terms[0]`",
        ),
        (
            r###"datapack.predicate("a", {"type": "minecraft:inverted", "term": "sim:nope"})"###,
            "the predicate `sim:nope` at `term`",
        ),
        (
            r###"datapack.item_modifier("a", {"type": "minecraft:sequence", "functions": ["sim:nope"]})"###,
            "the item modifier `sim:nope` at `functions[0]`",
        ),
        (
            r###"datapack.item_modifier("a", {"type": "minecraft:filtered", "item_filter": {}, "on_pass": "sim:nope"})"###,
            "the item modifier `sim:nope` at `on_pass`",
        ),
        (
            r###"datapack.item_modifier("a", {"type": "minecraft:set_loot_table", "loot_table_id": "sim:nope"})"###,
            "the loot table `sim:nope` at `loot_table_id`",
        ),
        (
            r###"datapack.advancement("a", {"criteria": {"a": {"trigger": "minecraft:tick", "conditions": {"player": "sim:nope"}}}})"###,
            "the predicate `sim:nope` at `criteria.a.conditions.player`",
        ),
        (
            &format!(
                r###"datapack.advancement("a", {{"parent": "sim:a", "criteria": {IMPOSSIBLE}}})"###
            ),
            "The advancement `sim:a` is its own parent (sim:a -> sim:a)",
        ),
        (
            &format!(
                r###"datapack.advancement("a", {{"parent": "sim:b", "criteria": {IMPOSSIBLE}}})
datapack.advancement("b", {{"parent": "sim:c", "criteria": {IMPOSSIBLE}}})
datapack.advancement("c", {{"parent": "sim:a", "criteria": {IMPOSSIBLE}}})"###
            ),
            "(sim:a -> sim:b -> sim:c -> sim:a)",
        ),
    ]);
    builds(
        r###"datapack.predicate("p", {"type": "minecraft:random_chance", "chance": 0.5})
datapack.item_modifier("m", {"type": "minecraft:set_count", "count": 1})
datapack.loot_table("t", {"pools": [], "modifier": ["sim:m"]})
datapack.recipe("r", {"type": "minecraft:stonecutting", "ingredient": "minecraft:stone", "result": {"id": "minecraft:stone_slab"}})
datapack.predicate("q", {"type": "minecraft:all_of", "terms": "sim:p"})
datapack.item_modifier("n", {"type": "minecraft:sequence", "functions": {"type": "minecraft:set_count", "count": 1}})
datapack.advancement("a", {"criteria": {"a": {"trigger": "minecraft:recipe_unlocked", "conditions": {"recipes": "sim:r", "player": "sim:p"}}}, "rewards": {"loot": "sim:t", "recipes": ["sim:not_checked"]}})"###,
    );
}

#[test]
fn many_tag_declarations_build_in_linear_time() {
    // Each merge used to parse and write the whole tag again, and each
    // reference check listed every resource: 20,000 declarations took
    // minutes in a release build.
    let count = 10_000;
    let mut merged = String::new();
    let mut distinct = String::new();
    let mut items = String::new();
    for i in 0..count {
        merged.push_str(&format!(
            "datapack.function_tag(\"t\", [\"sim:f\", \"sim:f{i}\"])\n"
        ));
        distinct.push_str(&format!("datapack.function_tag(\"t{i}\", [\"sim:f\"])\n"));
        items.push_str(&format!(
            "datapack.item_tag(\"t\", [{{\"id\": \"sim:i{i}\", \"required\": False}}])\n"
        ));
    }
    let defs: String = (0..count)
        .map(|i| format!("def f{i}():\n    /say {i}\n"))
        .collect();
    for declarations in [
        format!("{merged}{defs}def f():\n    /say f\n"),
        format!("{distinct}def f():\n    /say f\n"),
        items,
    ] {
        let start = std::time::Instant::now();
        builds(&declarations);
        let seconds = start.elapsed().as_secs_f64();
        assert!(seconds < 40.0, "took {seconds:.1} s");
    }
    // Merged function tags keep their order and drop repeats.
    let (_temp, output) = compile(
        "from stdlib import datapack\ndatapack.function_tag(\"t\", [\"sim:g\", \"sim:f\"])\ndatapack.function_tag(\"t\", [\"sim:f\", \"sim:h\"])\ndef f():\n    /say f\ndef g():\n    /say g\ndef h():\n    /say h\n",
    )
    .unwrap();
    let tag: serde_json::Value = serde_json::from_str(
        &std::fs::read_to_string(output.join("data/sim/tags/function/t.json")).unwrap(),
    )
    .unwrap();
    assert_eq!(
        tag["values"],
        serde_json::json!(["sim:g", "sim:f", "sim:h"])
    );
}
