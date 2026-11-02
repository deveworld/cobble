//! Text that has to pass through a macro line.
//!
//! A macro pastes a string argument as it is, and the SNBT string or path key
//! around the placeholder then reads its escapes: in `"$(left)$(right)"` the
//! text `back\slash` becomes `back lash`, and a `"` ends the string. Minecraft
//! has no command that joins two strings, and no way to read the escaped form
//! of a string back as text. So these helpers build text from pieces:
//!
//! 1. A text is split into pieces: runs without `\` or `"`, and single `\` or
//!    `"` characters. A run is found by pasting it into a macro line and
//!    comparing what comes back; a span that changes is cut in half.
//! 2. One pass of a macro line `"$(a0)...$(a7)"` joins eight pieces and reads
//!    one level of escapes. With `D` passes to join every piece, each piece
//!    starts escaped `D` times more than the result needs: a run stays as it
//!    is, and `\` and `"` are cut from a string of 127 backslashes and a `"`.
//!
//! Joining (`+`), repeating (`*`), `str()`, and runtime map keys use these
//! helpers. Text without `\` or `"` takes a short path that checks the text
//! once. The helpers keep their work in the storage `_cobble_esc` and never
//! change `args`.

use crate::transpiler::data_pack::DataPack;

/// The level a piece can be escaped to: a string of `2^LEVELS` characters
/// holds the escapes. Seven levels join `8^6` pieces for a map key, more
/// than a string Minecraft can save has characters.
const LEVELS: usize = 7;

/// The helper that escapes the key in `_cobble_esc.key` for a quoted path
/// key. It leaves the escaped key in `_cobble_esc.done` and returns 0 when
/// the key is empty or missing, which no path can name.
pub(in crate::transpiler) const ESCAPE_KEY: &str = "_cobble_esc_key";

/// Like a quoted key, but a container is shown as SNBT text as it is in an
/// NBT text component. Hover names and item components can't resolve one
/// themselves, so a temporary text display resolves it before the macro.
pub(in crate::transpiler) const ESCAPE_HOVER: &str = "_cobble_esc_hover";

pub(in crate::transpiler) fn add_hover_functions(data_pack: &mut DataPack) {
    add_functions(data_pack);
    if data_pack.functions.contains_key(ESCAPE_HOVER) {
        return;
    }
    let ns = &data_pack.namespace;
    let global = format!("storage {ns}:global");
    let esc = format!("{global} _cobble_esc");
    let parts = format!("{global} _cobble_text_parts");
    let part = format!("{global} _cobble_text_part");
    let functions = [
        (ESCAPE_HOVER, vec![
            format!("data remove {esc}.probe"),
            format!("execute store success score #esc_v temp run data modify {esc}.probe set string {esc}.key"),
            format!("execute if score #esc_v temp matches 1 run return run function {ns}:{ESCAPE_KEY}"),
            format!("data modify {parts} set value []"),
            // Resolve in a loaded chunk and remove the display before
            // Minecraft ticks or sends it to clients. A console caller's
            // position may be unloaded; any recipient supplies a fallback.
            format!("execute if loaded ~ ~ ~ summon minecraft:text_display run function {ns}:_cobble_text_resolve"),
            format!("execute unless loaded ~ ~ ~ at @a[limit=1] summon minecraft:text_display run function {ns}:_cobble_text_resolve"),
            format!("function {ns}:_cobble_esc_side_from"),
            format!("execute if data {parts}[0] run function {ns}:_cobble_text_parts"),
            "scoreboard players set #esc_k temp 1".to_string(),
            format!("function {ns}:_cobble_esc_build"),
            format!("data modify {esc}.done set from {esc}.out"),
        ]),
        ("_cobble_text_resolve", vec![
            format!("data modify entity @s text set value {{nbt:\"_cobble_esc.key\",storage:\"{ns}:global\"}}"),
            format!("data modify {parts} set from entity @s text.extra"),
            "kill @s".to_string(),
        ]),
        ("_cobble_text_parts", vec![
            format!("data modify {part} set from {parts}[0]"),
            format!("data remove {parts}[0]"),
            format!("data modify {esc}.text set value \"\""),
            format!("execute if data {part}.text run data modify {esc}.text set string {part}.text"),
            format!("execute unless data {part}.text run data modify {esc}.text set string {part}"),
            format!("function {ns}:_cobble_esc_split"),
            format!("data modify {parts} prepend from {part}.extra[]"),
            format!("execute if data {parts}[0] run function {ns}:_cobble_text_parts"),
        ]),
    ];
    for (name, commands) in functions {
        data_pack.add_function(name.to_string(), commands);
    }
}

/// Adds the functions behind runtime text to `data_pack`, once.
pub(in crate::transpiler) fn add_functions(data_pack: &mut DataPack) {
    if data_pack.functions.contains_key(ESCAPE_KEY) {
        return;
    }
    data_pack.track_objective("temp");
    data_pack.track_runtime_loop();
    let namespace = data_pack.namespace.clone();
    for (name, commands) in functions(&namespace) {
        data_pack.add_function(name, commands);
    }
}

/// Where [`key_commands`] reads the key, in `<namespace>:global`.
pub(in crate::transpiler) const KEY_INPUT: &str = "_cobble_esc.key";

/// The commands that copy the runtime map key at [`KEY_INPUT`] to `target`,
/// escaped for a quoted path key. `refusal` is the message shown when the
/// key is empty, which no path can name.
pub(in crate::transpiler) fn key_commands(
    data_pack: &mut DataPack,
    target: &str,
    refusal: &str,
) -> Vec<String> {
    add_functions(data_pack);
    let ns = data_pack.namespace.clone();
    vec![
        format!("execute store result score #esc_ok temp run function {ns}:{ESCAPE_KEY}"),
        format!(
            "execute if score #esc_ok temp matches 0 run tellraw @a {{\"text\":\"{}\",\"color\":\"red\"}}",
            crate::transpiler::escape_quoted(refusal)
        ),
        format!("data modify storage {ns}:global {target} set from storage {ns}:global _cobble_esc.done"),
    ]
}

/// The functions behind runtime text, for the pack namespace `ns`.
fn functions(ns: &str) -> Vec<(String, Vec<String>)> {
    let esc = format!("storage {ns}:global _cobble_esc");
    let global = format!("storage {ns}:global");
    let call = |name: &str| format!("function {ns}:{name}");
    let escapes = format!("\"{}\\\"\"", "\\\\".repeat((1 << LEVELS) - 1));
    let group: String = (0..8).map(|i| format!("$(a{i})")).collect();
    let empty_group: Vec<String> = (0..8).map(|i| format!("a{i}:\"\"")).collect();

    let mut functions: Vec<(&str, Vec<String>)> = vec![
        // Macro lines. Each pastes one value into an SNBT string.
        (
            "_cobble_esc_cut",
            vec![format!(
                "$data modify {esc}.seg set string {esc}.text $(a) $(b)"
            )],
        ),
        (
            "_cobble_esc_paste_seg",
            vec![format!("$data modify {esc}.rt set value \"$(seg)\"")],
        ),
        (
            "_cobble_esc_paste_side",
            vec![format!("$data modify {esc}.rt set value \"$(side)\"")],
        ),
        (
            "_cobble_esc_paste_done",
            vec![format!("$data modify {esc}.rt set value \"$(done)\"")],
        ),
        (
            "_cobble_esc_paste_value",
            vec![format!(
                "$data modify {global} _cobble_result set value \"$(value)\""
            )],
        ),
        (
            "_cobble_esc_join_try",
            vec![format!(
                "$data modify {esc}.p set value {{l:\"$(left)\",r:\"$(right)\",j:\"$(left)$(right)\"}}"
            )],
        ),
        (
            "_cobble_esc_consts",
            vec![
                format!("$data modify {esc}.bs set string {esc}.q 0 $(n)"),
                format!("$data modify {esc}.qs set string {esc}.q -$(n)"),
            ],
        ),
        (
            "_cobble_esc_group",
            vec![format!("$data modify {esc}.next append value \"{group}\"")],
        ),
        (
            "_cobble_esc_has_at",
            vec![
                format!("$execute if data {global} args.box.\"$(done)\" run return 1"),
                "return 0".to_string(),
            ],
        ),
        // Splitting `text` into `pieces`: {t:"run"}, {b:1b} for `\`, {q:1b}
        // for `"`. `stack` holds the spans still to look at, last first.
        (
            "_cobble_esc_split",
            vec![
                format!("execute store result score #esc_n temp run data get {esc}.text"),
                "execute if score #esc_n temp matches ..0 run return 0".to_string(),
                format!("data modify {esc}.stack set value [{{a:0,b:0}}]"),
                format!(
                    "execute store result {esc}.stack[0].b int 1 run scoreboard players get #esc_n temp"
                ),
                call("_cobble_esc_split_step"),
            ],
        ),
        (
            "_cobble_esc_split_step",
            vec![
                format!("data modify {esc}.span set from {esc}.stack[-1]"),
                format!("data remove {esc}.stack[-1]"),
                format!("{} with {esc}.span", call("_cobble_esc_cut")),
                format!("data remove {esc}.rt"),
                format!("{} with {esc}", call("_cobble_esc_paste_seg")),
                format!(
                    "execute store success score #esc_d temp run data modify {esc}.rt set from {esc}.seg"
                ),
                format!(
                    "execute if score #esc_d temp matches 0 run data modify {esc}.pieces append value {{}}"
                ),
                format!(
                    "execute if score #esc_d temp matches 0 run data modify {esc}.pieces[-1].t set from {esc}.seg"
                ),
                format!(
                    "execute if score #esc_d temp matches 1 run {}",
                    call("_cobble_esc_split_dirty")
                ),
                format!(
                    "execute if data {esc}.stack[0] run {}",
                    call("_cobble_esc_split_step")
                ),
            ],
        ),
        (
            "_cobble_esc_split_dirty",
            vec![
                format!("execute store result score #esc_a temp run data get {esc}.span.a"),
                format!("execute store result score #esc_b temp run data get {esc}.span.b"),
                "scoreboard players operation #esc_m temp = #esc_b temp".to_string(),
                "scoreboard players operation #esc_m temp -= #esc_a temp".to_string(),
                format!(
                    "execute if score #esc_m temp matches ..1 run return run {}",
                    call("_cobble_esc_special")
                ),
                "scoreboard players set #esc_2 temp 2".to_string(),
                "scoreboard players operation #esc_m temp /= #esc_2 temp".to_string(),
                "scoreboard players operation #esc_m temp += #esc_a temp".to_string(),
                // The right half goes on the stack first, so the left half
                // is looked at first.
                format!("data modify {esc}.stack append from {esc}.span"),
                format!(
                    "execute store result {esc}.stack[-1].a int 1 run scoreboard players get #esc_m temp"
                ),
                format!("data modify {esc}.stack append from {esc}.span"),
                format!(
                    "execute store result {esc}.stack[-1].b int 1 run scoreboard players get #esc_m temp"
                ),
            ],
        ),
        (
            "_cobble_esc_special",
            vec![
                format!("data modify {esc}.c set from {esc}.seg"),
                format!(
                    "execute store success score #esc_d temp run data modify {esc}.c set value \"\\\\\""
                ),
                format!(
                    "execute if score #esc_d temp matches 0 run return run data modify {esc}.pieces append value {{b:1b}}"
                ),
                format!("data modify {esc}.c set from {esc}.seg"),
                format!(
                    "execute store success score #esc_d temp run data modify {esc}.c set value \"\\\"\""
                ),
                format!(
                    "execute if score #esc_d temp matches 0 run return run data modify {esc}.pieces append value {{q:1b}}"
                ),
                "scoreboard players set #esc_bad temp 1".to_string(),
            ],
        ),
        // Joining `pieces` into `out`, escaped #esc_k times.
        (
            "_cobble_esc_build",
            vec![
                format!("data modify {esc}.out set value \"\""),
                format!("execute store result score #esc_m temp run data get {esc}.pieces"),
                "scoreboard players set #esc_8 temp 8".to_string(),
                "scoreboard players set #esc_D temp 0".to_string(),
                "scoreboard players set #esc_w temp 1".to_string(),
                format!(
                    "execute if score #esc_w temp < #esc_m temp run {}",
                    call("_cobble_esc_passes")
                ),
                "scoreboard players operation #esc_i temp = #esc_D temp".to_string(),
                "scoreboard players operation #esc_i temp += #esc_k temp".to_string(),
                format!(
                    "execute if score #esc_i temp matches {}.. run return run scoreboard players set #esc_bad temp 1",
                    LEVELS + 1
                ),
                "scoreboard players set #esc_w temp 1".to_string(),
                format!(
                    "execute if score #esc_i temp matches 1.. run {}",
                    call("_cobble_esc_double")
                ),
                format!(
                    "execute store result {esc}.n int 1 run scoreboard players get #esc_w temp"
                ),
                format!("data modify {esc}.q set value {escapes}"),
                format!("{} with {esc}", call("_cobble_esc_consts")),
                format!("data modify {esc}.cur set value []"),
                format!(
                    "execute if data {esc}.pieces[0] run {}",
                    call("_cobble_esc_leaves")
                ),
                format!(
                    "execute if score #esc_D temp matches 1.. run {}",
                    call("_cobble_esc_merge")
                ),
                format!("data modify {esc}.out set from {esc}.cur[0]"),
            ],
        ),
        (
            "_cobble_esc_passes",
            vec![
                "scoreboard players operation #esc_w temp *= #esc_8 temp".to_string(),
                "scoreboard players add #esc_D temp 1".to_string(),
                format!(
                    "execute if score #esc_w temp < #esc_m temp run {}",
                    call("_cobble_esc_passes")
                ),
            ],
        ),
        (
            "_cobble_esc_double",
            vec![
                "scoreboard players operation #esc_w temp += #esc_w temp".to_string(),
                "scoreboard players remove #esc_i temp 1".to_string(),
                format!(
                    "execute if score #esc_i temp matches 1.. run {}",
                    call("_cobble_esc_double")
                ),
            ],
        ),
        (
            "_cobble_esc_leaves",
            vec![
                format!(
                    "execute if data {esc}.pieces[0].t run data modify {esc}.cur append from {esc}.pieces[0].t"
                ),
                format!(
                    "execute if data {esc}.pieces[0].b run data modify {esc}.cur append from {esc}.bs"
                ),
                format!(
                    "execute if data {esc}.pieces[0].q run data modify {esc}.cur append from {esc}.qs"
                ),
                format!("data remove {esc}.pieces[0]"),
                format!(
                    "execute if data {esc}.pieces[0] run {}",
                    call("_cobble_esc_leaves")
                ),
            ],
        ),
        (
            "_cobble_esc_merge",
            vec![
                format!("data modify {esc}.next set value []"),
                call("_cobble_esc_pass"),
                format!("data modify {esc}.cur set from {esc}.next"),
                "scoreboard players remove #esc_D temp 1".to_string(),
                format!(
                    "execute if score #esc_D temp matches 1.. run {}",
                    call("_cobble_esc_merge")
                ),
            ],
        ),
        ("_cobble_esc_pass", {
            let mut commands = vec![format!(
                "data modify {esc}.g set value {{{}}}",
                empty_group.join(",")
            )];
            for i in 0..8 {
                commands.push(format!("data modify {esc}.g.a{i} set from {esc}.cur[0]"));
                commands.push(format!("data remove {esc}.cur[0]"));
            }
            commands.push(format!("{} with {esc}.g", call("_cobble_esc_group")));
            commands.push(format!(
                "execute if data {esc}.cur[0] run {}",
                call("_cobble_esc_pass")
            ));
            commands
        }),
        // The pieces of the value in `side`: a number or a list shows as a
        // macro line shows it, and text is split.
        (
            "_cobble_esc_side",
            vec![
                format!("execute unless data {esc}.side run return 0"),
                format!("execute store success score #esc_d temp run data get {esc}.side 1"),
                format!(
                    "execute if score #esc_d temp matches 1 run return run {}",
                    call("_cobble_esc_side_shown")
                ),
                format!("data remove {esc}.text"),
                format!(
                    "execute store success score #esc_d temp run data modify {esc}.text set string {esc}.side"
                ),
                format!(
                    "execute if score #esc_d temp matches 0 run return run {}",
                    call("_cobble_esc_side_shown")
                ),
                call("_cobble_esc_split"),
            ],
        ),
        (
            "_cobble_esc_side_shown",
            vec![
                format!("data remove {esc}.rt"),
                format!("{} with {esc}", call("_cobble_esc_paste_side")),
                format!(
                    "execute unless data {esc}.rt run return run scoreboard players set #esc_bad temp 1"
                ),
                format!("data modify {esc}.text set from {esc}.rt"),
                call("_cobble_esc_split"),
            ],
        ),
        (
            "_cobble_esc_side_from",
            vec![
                format!("data modify {esc}.pieces set value []"),
                "scoreboard players set #esc_bad temp 0".to_string(),
            ],
        ),
        // `left + right` into `_cobble_result`. A missing side counts as
        // empty text, and a value that can't be shown gives empty text.
        (
            "_cobble_esc_join",
            vec![
                format!("data modify {global} _cobble_result set value \"\""),
                format!("data remove {esc}.p"),
                format!(
                    "execute if data {global} args.left if data {global} args.right run {} with {global} args",
                    call("_cobble_esc_join_try")
                ),
                format!(
                    "execute store success score #esc_l temp run data modify {esc}.p.l set from {global} args.left"
                ),
                format!(
                    "execute store success score #esc_r temp run data modify {esc}.p.r set from {global} args.right"
                ),
                format!(
                    "execute if score #esc_l temp matches 0 if score #esc_r temp matches 0 if data {esc}.p.j run return run data modify {global} _cobble_result set from {esc}.p.j"
                ),
                call("_cobble_esc_side_from"),
                format!("data remove {esc}.side"),
                format!("data modify {esc}.side set from {global} args.left"),
                call("_cobble_esc_side"),
                format!("data remove {esc}.side"),
                format!("data modify {esc}.side set from {global} args.right"),
                call("_cobble_esc_side"),
                "scoreboard players set #esc_k temp 0".to_string(),
                call("_cobble_esc_build"),
                "execute if score #esc_bad temp matches 1 run return fail".to_string(),
                format!("data modify {global} _cobble_result set from {esc}.out"),
            ],
        ),
        // `text * count` into `_cobble_result`.
        (
            "_cobble_esc_repeat",
            vec![
                format!("data modify {global} _cobble_result set value \"\""),
                format!("execute store result score #esc_c temp run data get {global} args.count"),
                "execute if score #esc_c temp matches ..0 run return 0".to_string(),
                call("_cobble_esc_side_from"),
                format!("data remove {esc}.side"),
                format!("data modify {esc}.side set from {global} args.text"),
                call("_cobble_esc_side"),
                "execute if score #esc_bad temp matches 1 run return fail".to_string(),
                format!("data modify {esc}.unit set from {esc}.pieces"),
                "scoreboard players remove #esc_c temp 1".to_string(),
                format!(
                    "execute if score #esc_c temp matches 1.. run {}",
                    call("_cobble_esc_repeat_more")
                ),
                "scoreboard players set #esc_k temp 0".to_string(),
                call("_cobble_esc_build"),
                "execute if score #esc_bad temp matches 1 run return fail".to_string(),
                format!("data modify {global} _cobble_result set from {esc}.out"),
            ],
        ),
        (
            "_cobble_esc_repeat_more",
            vec![
                format!("data modify {esc}.pieces append from {esc}.unit[]"),
                "scoreboard players remove #esc_c temp 1".to_string(),
                format!(
                    "execute if score #esc_c temp matches 1.. run {}",
                    call("_cobble_esc_repeat_more")
                ),
            ],
        ),
        // `str(value)` into `_cobble_result`: text stays as it is, and a
        // number or a list shows as a macro line shows it.
        (
            "_cobble_esc_str",
            vec![
                format!("data remove {global} _cobble_result"),
                format!(
                    "execute store success score #esc_d temp run data modify {global} _cobble_result set string {global} args.value"
                ),
                format!("execute store success score #esc_n temp run data get {global} args.value 1"),
                format!(
                    "execute if score #esc_n temp matches 1 run {} with {global} args",
                    call("_cobble_esc_number_text")
                ),
                format!(
                    "execute if score #esc_d temp matches 0 run {} with {global} args",
                    call("_cobble_esc_paste_value")
                ),
                format!(
                    "execute unless data {global} _cobble_result run data modify {global} _cobble_result set value \"\""
                ),
            ],
        ),
        (
            "_cobble_esc_number_text",
            {
                let mut commands = vec![format!("data modify {esc}.suffix set string {global} args.value -1")];
                for suffix in ["f", "d", "b", "s", "L"] {
                    commands.push(format!("execute if data {esc}{{suffix:\"{suffix}\"}} run data modify {global} _cobble_result set string {global} args.value 0 -1"));
                }
                commands
            },
        ),
        (
            ESCAPE_KEY,
            vec![
                format!("data remove {esc}.done"),
                format!("execute unless data {esc}.key run return 0"),
                format!("data modify {esc}.done set from {esc}.key"),
                format!("data remove {esc}.key"),
                format!("data remove {esc}.rt"),
                format!("{} with {esc}", call("_cobble_esc_paste_done")),
                format!(
                    "execute store success score #esc_d temp run data modify {esc}.rt set from {esc}.done"
                ),
                format!(
                    "execute if score #esc_d temp matches 0 run return run {}",
                    call("_cobble_esc_key_kept")
                ),
                // A number is a key as a macro line shows it, and a list or
                // a map can't be one; both stay as they are.
                format!("execute store success score #esc_d temp run data get {esc}.done 1"),
                "execute if score #esc_d temp matches 1 run return 1".to_string(),
                format!("data remove {esc}.text"),
                format!(
                    "execute store success score #esc_d temp run data modify {esc}.text set string {esc}.done"
                ),
                "execute if score #esc_d temp matches 0 run return 1".to_string(),
                call("_cobble_esc_side_from"),
                call("_cobble_esc_split"),
                "scoreboard players set #esc_k temp 1".to_string(),
                call("_cobble_esc_build"),
                format!("data modify {esc}.done set from {esc}.out"),
                "return 1".to_string(),
            ],
        ),
        (
            "_cobble_esc_key_kept",
            vec![
                format!("execute store result score #esc_n temp run data get {esc}.done"),
                "execute if score #esc_n temp matches 0 run return 0".to_string(),
                "return 1".to_string(),
            ],
        ),
        // `key in box` for a map.
        (
            "_cobble_esc_has",
            vec![
                format!("data remove {esc}.key"),
                format!("data modify {esc}.key set from {global} args.key"),
                format!(
                    "execute store result score #esc_ok temp run {}",
                    call(ESCAPE_KEY)
                ),
                "execute if score #esc_ok temp matches 0 run return 0".to_string(),
                format!("return run {} with {esc}", call("_cobble_esc_has_at")),
            ],
        ),
    ];
    functions
        .drain(..)
        .map(|(name, commands)| (name.to_string(), commands))
        .collect()
}

/// `text` escaped for a double-quoted path key, which reads only `\\` and
/// `\"`.
pub(in crate::transpiler) fn escape_path_key(text: &str) -> String {
    text.replace('\\', "\\\\").replace('"', "\\\"")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_escape_string_holds_every_level() {
        let functions = functions("ns");
        let build = &functions
            .iter()
            .find(|(name, _)| name == "_cobble_esc_build")
            .unwrap()
            .1;
        let set = build
            .iter()
            .find(|command| command.contains(".q set value"))
            .unwrap();
        let literal = set.split_once("set value ").unwrap().1;
        // 127 escaped backslashes and an escaped quote, in quotes.
        assert_eq!(literal.len(), 2 + 127 * 2 + 2);
        assert!(literal.ends_with("\\\"\""));
    }

    #[test]
    fn path_keys_escape_backslashes_and_quotes() {
        assert_eq!(escape_path_key(r#"a\b"c"#), r#"a\\b\"c"#);
    }
}
