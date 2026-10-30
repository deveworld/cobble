//! Checks on argument values whose shape is right but which Minecraft
//! rejects when it loads a function: mixed coordinates, reversed ranges,
//! broken NBT paths, and selector arguments that repeat or can't be negated.
//! Each rule was confirmed against a 26.3 server. Values with a `$(name)`
//! placeholder are known only when the function runs and are skipped.

use super::arg_parsers::split_selector_arguments;
use super::string_reader::StringReader;

/// Selector arguments that can appear once.
const SINGLE_ARGUMENTS: &[&str] = &[
    "x",
    "y",
    "z",
    "dx",
    "dy",
    "dz",
    "distance",
    "level",
    "x_rotation",
    "y_rotation",
    "limit",
    "sort",
    "scores",
    "advancements",
];
/// Selector arguments that can repeat only when every use is negated.
const NEGATABLE_ARGUMENTS: &[&str] = &["team", "name", "gamemode", "type"];
/// Selector arguments that can't be negated with `!`.
const PLAIN_ARGUMENTS: &[&str] = &[
    "x", "y", "z", "dx", "dy", "dz", "distance", "level", "limit", "sort",
];

/// What is wrong with the argument `token` of the parser `parser_type`.
pub fn problem(parser_type: &str, token: &str) -> Option<String> {
    if token.contains("$(") {
        return None;
    }
    match parser_type {
        "minecraft:block_pos" => coordinates_problem(token, true, true),
        "minecraft:vec3" => coordinates_problem(token, false, true),
        "minecraft:column_pos" => coordinates_problem(token, true, false),
        "minecraft:vec2" | "minecraft:rotation" => coordinates_problem(token, false, false),
        "minecraft:int_range" | "minecraft:float_range" => range_problem(token),
        "minecraft:nbt_path" => nbt_path_problem(token),
        "minecraft:entity" | "minecraft:score_holder" | "minecraft:game_profile" => {
            selector_problem(token)
        }
        _ => None,
    }
}

fn coordinates_problem(token: &str, integers: bool, local: bool) -> Option<String> {
    let parts: Vec<&str> = token.split_whitespace().collect();
    let locals = parts.iter().filter(|part| part.starts_with('^')).count();
    if locals > 0 && !local {
        return Some(format!("`{token}` can't use `^` coordinates"));
    }
    if locals > 0 && locals != parts.len() {
        return Some(format!("`{token}` mixes `^` with other coordinates"));
    }
    if integers {
        let fractional = parts
            .iter()
            .find(|part| !part.starts_with(['~', '^']) && part.contains('.'));
        if let Some(part) = fractional {
            return Some(format!("Block coordinate `{part}` must be a whole number"));
        }
    }
    None
}

fn range_problem(range: &str) -> Option<String> {
    let (low, high) = range.split_once("..")?;
    let (low, high) = (low.parse::<f64>().ok()?, high.parse::<f64>().ok()?);
    (low > high).then(|| format!("Range `{range}` has its low end above its high end"))
}

/// Follows Minecraft's `NbtPathArgument`.
fn nbt_path_problem(path: &str) -> Option<String> {
    let invalid = || Some(format!("`{path}` is not a valid NBT path"));
    let mut reader = StringReader::new(path);
    let mut first = true;
    while reader.can_read() {
        let valid = match reader.peek() {
            Some('{') => first && reader.read_nbt(),
            Some('[') => read_nbt_index(&mut reader),
            Some('"' | '\'') => reader.read_quoted_string() && read_nbt_filter(&mut reader),
            _ => read_nbt_name(&mut reader) && read_nbt_filter(&mut reader),
        };
        if !valid {
            return invalid();
        }
        first = false;
        match reader.peek() {
            None | Some('[' | '{') => {}
            Some('.') => {
                reader.read_char();
            }
            Some(_) => return invalid(),
        }
    }
    None
}

/// `[]`, `[3]`, `[-1]`, or `[{filter}]`.
fn read_nbt_index(reader: &mut StringReader) -> bool {
    reader.read_char();
    match reader.peek() {
        Some(']') => {}
        Some('{') => {
            if !reader.read_nbt() {
                return false;
            }
        }
        _ => {
            let start = reader.cursor();
            while reader.peek().is_some_and(|ch| ch != ']') {
                reader.read_char();
            }
            if reader.slice(start, reader.cursor()).parse::<i32>().is_err() {
                return false;
            }
        }
    }
    reader.peek() == Some(']') && reader.read_char().is_some()
}

fn read_nbt_name(reader: &mut StringReader) -> bool {
    let start = reader.cursor();
    while reader
        .peek()
        .is_some_and(|ch| !matches!(ch, ' ' | '"' | '\'' | '[' | ']' | '.' | '{' | '}'))
    {
        reader.read_char();
    }
    reader.cursor() > start
}

fn read_nbt_filter(reader: &mut StringReader) -> bool {
    reader.peek() != Some('{') || reader.read_nbt()
}

fn selector_problem(selector: &str) -> Option<String> {
    let rest = selector.strip_prefix('@')?;
    let variable = rest.chars().next()?;
    let arguments = rest[variable.len_utf8()..]
        .strip_prefix('[')?
        .strip_suffix(']')?;
    let arguments = split_selector_arguments(arguments);
    let mut seen: Vec<(&str, bool)> = Vec::new();
    for argument in &arguments {
        let Some((key, value)) = argument.split_once('=') else {
            continue;
        };
        let (key, value) = (key.trim(), value.trim());
        let negated = value.starts_with('!');
        let plain_value = value.trim_start_matches('!');
        if variable == 's' && matches!(key, "limit" | "sort") {
            return Some(format!("`@s` can't take `{key}`"));
        }
        if negated && PLAIN_ARGUMENTS.contains(&key) {
            return Some(format!("Selector argument `{key}` can't be negated"));
        }
        if matches!(key, "tag" | "team") && plain_value.starts_with(['"', '\'']) {
            return Some(format!("The value of `{key}` can't be quoted"));
        }
        let unquoted = !plain_value.starts_with(['"', '\'', '{']);
        if unquoted && plain_value.contains(char::is_whitespace) {
            return Some(format!("The value of `{key}` can't hold a space"));
        }
        if SINGLE_ARGUMENTS.contains(&key) && seen.iter().any(|(name, _)| *name == key) {
            return Some(format!("Selector argument `{key}` can appear once"));
        }
        if NEGATABLE_ARGUMENTS.contains(&key)
            && seen
                .iter()
                .any(|(name, other)| *name == key && !(negated && *other))
        {
            return Some(format!(
                "Selector argument `{key}` can repeat only when every use is negated"
            ));
        }
        seen.push((key, negated));
        if let Some(problem) = selector_value_problem(key, plain_value) {
            return Some(problem);
        }
    }
    None
}

fn selector_value_problem(key: &str, value: &str) -> Option<String> {
    match key {
        "distance" | "level" => {
            if value.trim_start_matches('.').starts_with('-') || value.starts_with('-') {
                return Some(format!("`{key}` can't be negative"));
            }
            range_problem(value)
        }
        "scores" => value
            .trim_start_matches('{')
            .trim_end_matches('}')
            .split(',')
            .filter_map(|score| score.split_once('='))
            .find_map(|(_, range)| range_problem(range.trim())),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::problem;

    #[test]
    fn values_minecraft_accepts_pass() {
        for (parser, token) in [
            ("minecraft:block_pos", "~1.5 ~ ~"),
            ("minecraft:block_pos", "^1.5 ^ ^"),
            ("minecraft:block_pos", "1 2 3"),
            ("minecraft:block_pos", "$(x) 2 ^"),
            ("minecraft:vec3", "1.5 2 3"),
            ("minecraft:vec3", "^1 ^ ^"),
            ("minecraft:vec3", "~ ~1.5 ~"),
            ("minecraft:column_pos", "~1.5 0"),
            ("minecraft:rotation", "~ 1.5"),
            ("minecraft:int_range", "1..1"),
            ("minecraft:int_range", "..5"),
            ("minecraft:int_range", "5.."),
            ("minecraft:int_range", "-5..-1"),
            ("minecraft:nbt_path", "Pos[0]"),
            ("minecraft:nbt_path", "Pos[-1]"),
            ("minecraft:nbt_path", "Pos[]"),
            ("minecraft:nbt_path", "Inventory[{Slot:0b}].id"),
            ("minecraft:nbt_path", "{a:1}"),
            ("minecraft:nbt_path", "{a:1}.b"),
            ("minecraft:nbt_path", "a.b[0].c"),
            ("minecraft:nbt_path", "\"a b\".c"),
            ("minecraft:nbt_path", "'a'"),
            ("minecraft:nbt_path", "a{b:1}.c"),
            ("minecraft:nbt_path", "a[0][1]"),
            ("minecraft:nbt_path", "a."),
            ("minecraft:nbt_path", "a[$(i)].b"),
            ("minecraft:entity", "@e[distance=..5,tag=a,tag=b]"),
            ("minecraft:entity", "@e[distance=1.5..2.5]"),
            ("minecraft:entity", "@e[x_rotation=5..1]"),
            ("minecraft:entity", "@e[dx=-1]"),
            ("minecraft:entity", "@e[name=\"a b\"]"),
            ("minecraft:entity", "@e[team=!a,team=!b]"),
            (
                "minecraft:entity",
                "@e[type=!minecraft:cow,type=!minecraft:pig]",
            ),
            (
                "minecraft:entity",
                "@e[gamemode=!survival,gamemode=!creative]",
            ),
            ("minecraft:entity", "@e[nbt={a:1},nbt={b:2}]"),
            ("minecraft:entity", "@e[nbt={a:\"b c\"}]"),
            ("minecraft:entity", "@e[predicate=a:b,predicate=a:c]"),
            ("minecraft:entity", "@e[scores={a=1..,b=..2}]"),
            ("minecraft:entity", "@p[limit=1,sort=nearest]"),
            ("minecraft:entity", "@a[team=]"),
            ("minecraft:entity", "@s"),
            ("minecraft:entity", "Steve"),
        ] {
            assert_eq!(problem(parser, token), None, "{token}");
        }
    }

    #[test]
    fn values_minecraft_rejects_are_reported() {
        for (parser, token, expected) in [
            ("minecraft:block_pos", "~ ^ ~", "mixes"),
            ("minecraft:vec3", "^ ~ ^", "mixes"),
            ("minecraft:vec3", "1 ~ ^", "mixes"),
            (
                "minecraft:block_pos",
                "~ ~ 1.5",
                "`1.5` must be a whole number",
            ),
            (
                "minecraft:block_pos",
                "1 2 3.0",
                "`3.0` must be a whole number",
            ),
            ("minecraft:column_pos", "1.5 0", "must be a whole number"),
            ("minecraft:column_pos", "^ ^", "can't use `^`"),
            ("minecraft:vec2", "^ ^", "can't use `^`"),
            ("minecraft:rotation", "^ 0", "can't use `^`"),
            ("minecraft:int_range", "5..1", "low end above its high end"),
            ("minecraft:nbt_path", "a[", "not a valid NBT path"),
            ("minecraft:nbt_path", "a]", "not a valid NBT path"),
            ("minecraft:nbt_path", ".a", "not a valid NBT path"),
            ("minecraft:nbt_path", "a..b", "not a valid NBT path"),
            ("minecraft:nbt_path", "a[0]b", "not a valid NBT path"),
            ("minecraft:nbt_path", "a[x]", "not a valid NBT path"),
            ("minecraft:nbt_path", "a[1.5]", "not a valid NBT path"),
            (
                "minecraft:entity",
                "@e[distance=-1]",
                "`distance` can't be negative",
            ),
            (
                "minecraft:entity",
                "@e[distance=-1..5]",
                "can't be negative",
            ),
            ("minecraft:entity", "@e[distance=5..1]", "low end above"),
            (
                "minecraft:entity",
                "@e[level=-1]",
                "`level` can't be negative",
            ),
            ("minecraft:entity", "@e[level=5..1]", "low end above"),
            ("minecraft:entity", "@e[scores={a=5..1}]", "low end above"),
            ("minecraft:entity", "@e[tag=a b]", "can't hold a space"),
            ("minecraft:entity", "@e[tag=\"a\"]", "can't be quoted"),
            ("minecraft:entity", "@e[team=\"a b\"]", "can't be quoted"),
            (
                "minecraft:entity",
                "@e[limit=1,limit=2]",
                "`limit` can appear once",
            ),
            ("minecraft:entity", "@e[x=1,x=2]", "`x` can appear once"),
            (
                "minecraft:entity",
                "@e[scores={a=1},scores={b=2}]",
                "can appear once",
            ),
            (
                "minecraft:entity",
                "@e[team=a,team=b]",
                "every use is negated",
            ),
            (
                "minecraft:entity",
                "@e[team=a,team=!b]",
                "every use is negated",
            ),
            (
                "minecraft:entity",
                "@e[team=!a,team=b]",
                "every use is negated",
            ),
            (
                "minecraft:entity",
                "@e[gamemode=survival,gamemode=!creative]",
                "every use",
            ),
            (
                "minecraft:entity",
                "@a[type=minecraft:pig,type=minecraft:pig]",
                "every use",
            ),
            ("minecraft:entity", "@e[limit=!1]", "can't be negated"),
            ("minecraft:entity", "@e[distance=!1]", "can't be negated"),
            ("minecraft:entity", "@s[limit=1]", "`@s` can't take `limit`"),
            (
                "minecraft:entity",
                "@s[sort=nearest]",
                "`@s` can't take `sort`",
            ),
        ] {
            let found = problem(parser, token).unwrap_or_default();
            assert!(found.contains(expected), "{token}: {found}");
        }
    }
}
