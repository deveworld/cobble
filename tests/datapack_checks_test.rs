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
            "written with `#`",
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
    // The server loads a match_block with an unknown block.
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
            "not an item a recipe can make",
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
