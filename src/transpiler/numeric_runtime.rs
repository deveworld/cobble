//! Runtime dispatch for storage-backed numbers. Native providers implement
//! binary32; software helpers implement binary64; scores are integer registers.

use super::DataPack;

const OPS: &[(&str, &str)] = &[
    ("add", "add"),
    ("sub", "sub"),
    ("mul", "mul"),
    ("div", "div"),
    ("mod", "mod"),
    ("pow", "pow"),
    ("floor_div", "div"),
    ("compare", "sub"),
];

pub(super) fn add_functions(pack: &mut DataPack) {
    let mask: u32 = OPS
        .iter()
        .map(|(name, _)| *name)
        .chain(["cast_int", "cast_float", "cast_double", "sqrt"])
        .enumerate()
        .filter(|(_, op)| {
            pack.functions
                .contains_key(&format!("_cobble_numeric_double_{op}"))
        })
        .map(|(index, _)| 1 << index)
        .sum();
    let marker = format!("# numeric double mask {mask}");
    if pack
        .functions
        .get("_cobble_numeric_types")
        .and_then(|commands| commands.first())
        == Some(&marker)
    {
        return;
    }
    let ns = pack.namespace.clone();
    pack.track_objective("temp");
    let mut types = vec![marker];
    for side in ["left", "right"] {
        types.extend([
            format!("execute store success score #numeric_{side}_valid temp run data get storage {ns}:global _cobble_numeric.{side} 1"),
            format!("data modify storage {ns}:global _cobble_numeric.{side}_suffix set string storage {ns}:global _cobble_numeric.{side} -1"),
            format!("scoreboard players set #numeric_{side}_kind temp 0"),
            format!("execute if data storage {ns}:global _cobble_numeric{{{side}_suffix:\"f\"}} run scoreboard players set #numeric_{side}_kind temp 1"),
            format!("execute if data storage {ns}:global _cobble_numeric{{{side}_suffix:\"d\"}} run scoreboard players set #numeric_{side}_kind temp 2"),
        ]);
    }
    for side in ["left", "right"] {
        types.push(format!("data modify storage {ns}:global _cobble_numeric.number_text set string storage {ns}:global _cobble_numeric.{side}"));
        for value in [
            "NaNf",
            "NaNd",
            "Infinityf",
            "Infinityd",
            "-Infinityf",
            "-Infinityd",
        ] {
            types.push(format!("execute if data storage {ns}:global _cobble_numeric{{number_text:\"{value}\"}} run scoreboard players set #numeric_{side}_valid temp 0"));
        }
    }
    pack.add_function("_cobble_numeric_types".into(), types);
    let mut normalize = Vec::new();
    for side in ["left", "right"] {
        normalize.extend([
            format!("execute if score #numeric_{side}_kind temp matches 1 run data modify storage {ns}:global _cobble_numeric.{side}_float set from storage {ns}:global _cobble_numeric.{side}"),
            format!("execute if score #numeric_{side}_kind temp matches 0 store result storage {ns}:global _cobble_numeric.{side}_int int 1 run data get storage {ns}:global _cobble_numeric.{side} 1"),
            format!("execute if score #numeric_{side}_kind temp matches 0 run data modify storage {ns}:global _cobble_numeric.{side}_float set compute default float {{type:\"minecraft:from_int\",input:{{type:\"minecraft:storage\",storage:\"{ns}:global\",path:\"_cobble_numeric.{side}_int\"}}}}"),
        ]);
    }
    pack.add_function("_cobble_numeric_float_inputs".into(), normalize);
    for &(op, provider) in OPS {
        let left = format!("{{type:\"minecraft:storage\",storage:\"{ns}:global\",path:\"_cobble_numeric.left_float\"}}");
        let right = format!("{{type:\"minecraft:storage\",storage:\"{ns}:global\",path:\"_cobble_numeric.right_float\"}}");
        let provider = match op {
            "add" | "mul" => format!("{{type:\"minecraft:{provider}\",inputs:[{left},{right}]}}"),
            "pow" => format!("{{type:\"minecraft:{provider}\",base:{left},exponent:{right}}}"),
            _ => format!("{{type:\"minecraft:{provider}\",left:{left},right:{right}}}"),
        };
        let mut float_commands = vec![
            format!("function {ns}:_cobble_numeric_float_inputs"),
            format!("data remove storage {ns}:global _cobble_numeric.result"),
            format!("execute store success score #numeric_ok temp run data modify storage {ns}:global _cobble_numeric.result set compute default float {provider}"),
        ];
        if op == "floor_div" {
            let result = format!("{{type:\"minecraft:storage\",storage:\"{ns}:global\",path:\"_cobble_numeric.result\"}}");
            float_commands.push(format!("data modify storage {ns}:global _cobble_numeric.result set compute default float {{type:\"minecraft:floor\",input:{result}}}"));
        }
        if op == "mod" {
            float_commands.extend([
                format!("function {ns}:_cobble_numeric_float_signs"),
                format!("execute if score #numeric_sign_result temp matches 0 if score #numeric_sign_right temp matches -1 run data modify storage {ns}:global _cobble_numeric.result set value -0.0f"),
                format!("execute if score #numeric_sign_result temp matches 0 if score #numeric_sign_right temp matches 1 run data modify storage {ns}:global _cobble_numeric.result set value 0.0f"),
                format!("execute unless score #numeric_sign_result temp matches 0 unless score #numeric_sign_result temp = #numeric_sign_right temp run data modify storage {ns}:global _cobble_numeric.result set compute default float {{type:\"minecraft:add\",inputs:[{{type:\"minecraft:storage\",storage:\"{ns}:global\",path:\"_cobble_numeric.result\"}},{right}]}}"),
            ]);
        }
        if op == "compare" {
            // Opposite signs can overflow subtraction. Resolve those first.
            float_commands = vec![
                format!("function {ns}:_cobble_numeric_float_inputs"),
                format!("data modify storage {ns}:global _cobble_numeric.result set from storage {ns}:global _cobble_numeric.left_float"),
                format!("function {ns}:_cobble_numeric_float_signs"),
                "scoreboard players operation #numeric_order temp = #numeric_sign_result temp".into(),
                "scoreboard players operation #numeric_order temp -= #numeric_sign_right temp".into(),
                format!("execute if score #numeric_order temp matches 0 run data modify storage {ns}:global _cobble_numeric.result set compute default float {provider}"),
                format!("execute if score #numeric_order temp matches 0 run function {ns}:_cobble_numeric_float_signs"),
                "execute if score #numeric_order temp matches 0 run scoreboard players operation #numeric_order temp = #numeric_sign_result temp".into(),
                "execute if score #numeric_order temp matches ..-1 run scoreboard players set #numeric_order temp -1".into(),
                "execute if score #numeric_order temp matches 1.. run scoreboard players set #numeric_order temp 1".into(),
                format!("execute store result storage {ns}:global _cobble_numeric.result int 1 run scoreboard players get #numeric_order temp"),
            ];
        }
        if op != "compare" {
            float_commands.push(format!("function {ns}:_cobble_numeric_float_validate"));
        }
        pack.add_function(format!("_cobble_numeric_float_{op}"), float_commands);
        let mut commands = vec![
            format!("data modify storage {ns}:global _cobble_numeric.result set value 0"),
            format!("function {ns}:_cobble_numeric_coerce"),
            format!("function {ns}:_cobble_numeric_types"),
            "execute if score #numeric_left_valid temp matches 0 run tellraw @a {\"text\":\"Expected numeric operands.\",\"color\":\"red\"}".into(),
            "execute if score #numeric_left_valid temp matches 1 if score #numeric_right_valid temp matches 0 run tellraw @a {\"text\":\"Expected numeric operands.\",\"color\":\"red\"}".into(),
            "execute if score #numeric_left_valid temp matches 0 run return fail".into(),
            "execute if score #numeric_right_valid temp matches 0 run return fail".into(),
            format!("execute if score #numeric_left_kind temp matches 2 run return run function {ns}:_cobble_numeric_dispatch_double_{op}"),
            format!("execute if score #numeric_right_kind temp matches 2 run return run function {ns}:_cobble_numeric_dispatch_double_{op}"),
            format!("execute if score #numeric_left_kind temp matches 1 run return run function {ns}:_cobble_numeric_float_{op}"),
            format!("execute if score #numeric_right_kind temp matches 1 run return run function {ns}:_cobble_numeric_float_{op}"),
            format!("execute store result score #numeric_left temp run data get storage {ns}:global _cobble_numeric.left 1"),
            format!("execute store result score #numeric_right temp run data get storage {ns}:global _cobble_numeric.right 1"),
        ];
        if !pack
            .functions
            .contains_key(&format!("_cobble_numeric_double_{op}"))
        {
            commands.retain(|command| !command.contains("_cobble_numeric_dispatch_double_"));
            commands.insert(
                4,
                "execute if score #numeric_left_kind temp matches 2 run return fail".into(),
            );
            commands.insert(
                5,
                "execute if score #numeric_right_kind temp matches 2 run return fail".into(),
            );
        }
        if op == "div" {
            commands.push(format!(
                "return run function {ns}:_cobble_numeric_float_div"
            ));
        }
        if op == "compare" {
            commands.extend([
                "scoreboard players set #numeric_left_order temp 0".into(),
                "execute if score #numeric_left temp < #numeric_right temp run scoreboard players set #numeric_left_order temp -1".into(),
                "execute if score #numeric_left temp > #numeric_right temp run scoreboard players set #numeric_left_order temp 1".into(),
                "scoreboard players operation #numeric_left temp = #numeric_left_order temp".into(),
            ]);
        } else if op == "pow" {
            commands.extend([
                "scoreboard players set #numeric_power temp 1".into(),
                "execute if score #numeric_right temp matches ..-1 run scoreboard players set #numeric_power temp 0".into(),
                format!("execute if score #numeric_right temp matches 1.. run function {ns}:_cobble_numeric_integer_power"),
                "scoreboard players operation #numeric_left temp = #numeric_power temp".into(),
            ]);
        } else {
            let operation = match op {
                "add" => "+=",
                "sub" => "-=",
                "mul" => "*=",
                "div" | "floor_div" => "/=",
                _ => "%=",
            };
            commands.push(format!(
                "scoreboard players operation #numeric_left temp {operation} #numeric_right temp"
            ));
        }
        commands.push(format!("execute store result storage {ns}:global _cobble_numeric.result int 1 run scoreboard players get #numeric_left temp"));
        pack.add_function(format!("_cobble_numeric_{op}"), commands);
    }
    for op in
        OPS.iter()
            .map(|(name, _)| *name)
            .chain(["cast_int", "cast_float", "cast_double", "sqrt"])
    {
        if pack
            .functions
            .contains_key(&format!("_cobble_numeric_double_{op}"))
        {
            pack.add_function(format!("_cobble_numeric_dispatch_double_{op}"), vec![
                format!("data remove storage {ns}:global _cobble_numeric.saved_args"),
                format!("data modify storage {ns}:global _cobble_numeric.saved_args set from storage {ns}:global args"),
                format!("data modify storage {ns}:global args set from storage {ns}:global _cobble_numeric"),
                format!("function {ns}:_cobble_numeric_double_{op} with storage {ns}:global args"),
                format!("data remove storage {ns}:global args"),
                format!("data modify storage {ns}:global args set from storage {ns}:global _cobble_numeric.saved_args"),
            ]);
        }
    }
    let mut coerce = Vec::new();
    for side in ["left", "right"] {
        coerce.extend([
            format!("execute store success score #numeric_coerce temp run data get storage {ns}:global _cobble_numeric.{side} 1"),
            format!("execute if score #numeric_coerce temp matches 0 store result storage {ns}:global _cobble_numeric.{side} int 1 run data get storage {ns}:global _cobble_numeric.{side}"),
        ]);
    }
    pack.add_function("_cobble_numeric_coerce".into(), coerce);
    let mut signs = Vec::new();
    for side in ["result", "right_float"] {
        let name = if side == "result" { "result" } else { "right" };
        signs.extend([
            format!("scoreboard players set #numeric_sign_{name} temp 1"),
            format!("data modify storage {ns}:global _cobble_numeric.sign_text set string storage {ns}:global _cobble_numeric.{side} 0 1"),
            format!("execute if data storage {ns}:global _cobble_numeric{{sign_text:\"-\"}} run scoreboard players set #numeric_sign_{name} temp -1"),
            format!("execute if data storage {ns}:global _cobble_numeric{{{side}:0.0f}} run scoreboard players set #numeric_sign_{name} temp 0"),
            format!("execute if data storage {ns}:global _cobble_numeric{{{side}:-0.0f}} run scoreboard players set #numeric_sign_{name} temp 0"),
        ]);
    }
    pack.add_function("_cobble_numeric_float_signs".into(), signs);
    let mut validate = vec![format!("data modify storage {ns}:global _cobble_numeric.number_text set string storage {ns}:global _cobble_numeric.result")];
    for value in ["NaNf", "Infinityf", "-Infinityf"] {
        validate.push(format!("execute if data storage {ns}:global _cobble_numeric{{number_text:\"{value}\"}} run scoreboard players set #numeric_ok temp 0"));
    }
    validate.extend([
        format!("execute if score #numeric_ok temp matches 0 run data modify storage {ns}:global _cobble_numeric.result set value 0.0f"),
        "execute if score #numeric_ok temp matches 0 run tellraw @a {\"text\":\"Invalid or overflowing float calculation.\",\"color\":\"red\"}".into(),
    ]);
    pack.add_function("_cobble_numeric_float_validate".into(), validate);

    pack.add_function("_cobble_numeric_equal".into(), vec![
        format!("function {ns}:_cobble_numeric_types"),
        format!("execute if score #numeric_left_valid temp matches 1 if score #numeric_right_valid temp matches 1 run return run function {ns}:_cobble_numeric_compare"),
        format!("data modify storage {ns}:global _cobble_numeric.copy set from storage {ns}:global _cobble_numeric.left"),
        format!("execute store success storage {ns}:global _cobble_numeric.result int 1 run data modify storage {ns}:global _cobble_numeric.copy set from storage {ns}:global _cobble_numeric.right"),
    ]);
    pack.add_function("_cobble_numeric_truth".into(), vec![
        format!("data modify storage {ns}:global _cobble_numeric.right set value 0"),
        format!("function {ns}:_cobble_numeric_types"),
        format!("execute if score #numeric_left_valid temp matches 1 run return run function {ns}:_cobble_numeric_compare"),
        format!("execute store result storage {ns}:global _cobble_numeric.result int 1 run data get storage {ns}:global _cobble_numeric.left"),
    ]);

    let mut sqrt = vec![
        format!("data modify storage {ns}:global _cobble_numeric.result set value 0"),
        format!("function {ns}:_cobble_numeric_types"),
    ];
    if pack.functions.contains_key("_cobble_numeric_double_sqrt") {
        sqrt.push(format!("execute if score #numeric_left_kind temp matches 2 run return run function {ns}:_cobble_numeric_dispatch_double_sqrt"));
    }
    sqrt.push(format!(
        "return run function {ns}:_cobble_numeric_float_sqrt"
    ));
    pack.add_function("_cobble_numeric_sqrt".into(), sqrt);
    pack.add_function("_cobble_numeric_float_sqrt".into(), vec![
        format!("data remove storage {ns}:global _cobble_numeric.result"),
        format!("execute store success score #numeric_ok temp run data modify storage {ns}:global _cobble_numeric.result set compute default float {{type:\"minecraft:sqrt\",input:{{type:\"minecraft:storage\",storage:\"{ns}:global\",path:\"_cobble_numeric.left\"}}}}"),
        format!("function {ns}:_cobble_numeric_float_validate"),
    ]);
    pack.add_function("_cobble_numeric_integer_power".into(), vec![
        "scoreboard players operation #numeric_odd temp = #numeric_right temp".into(),
        "scoreboard players set #numeric_two temp 2".into(),
        "scoreboard players operation #numeric_odd temp %= #numeric_two temp".into(),
        "execute if score #numeric_odd temp matches 1 run scoreboard players operation #numeric_power temp *= #numeric_left temp".into(),
        "scoreboard players operation #numeric_right temp /= #numeric_two temp".into(),
        "scoreboard players operation #numeric_left temp *= #numeric_left temp".into(),
        format!("execute if score #numeric_right temp matches 1.. run return run function {ns}:_cobble_numeric_integer_power"),
    ]);
}
