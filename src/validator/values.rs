//! Checks on argument values whose shape is right but which Minecraft
//! rejects when it loads a function: mixed coordinates, reversed ranges,
//! broken NBT paths, and selector arguments that repeat or can't be negated.
//! Each rule was confirmed against a 26.3 server. Values with a `$(name)`
//! placeholder are known only when the function runs and are skipped.

use super::arg_parsers::is_id;
use super::registries::unquoted;
use super::snbt;
use super::string_reader::{is_java_whitespace, StringReader};
use super::text_component;

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
const NEGATABLE_ARGUMENTS: &[&str] = &["team", "name", "gamemode"];
/// Selector arguments that can't be negated with `!`.
const PLAIN_ARGUMENTS: &[&str] = &[
    "x",
    "y",
    "z",
    "dx",
    "dy",
    "dz",
    "distance",
    "level",
    "limit",
    "sort",
    "x_rotation",
    "y_rotation",
    "scores",
    "advancements",
];

/// What stops the target `token` from fitting its argument: the one target or
/// the players that `properties` ask for, which a score holder or a game
/// profile can ask for too.
pub fn target_problem(
    parser_type: &str,
    properties: Option<&serde_json::Value>,
    token: &str,
) -> Option<String> {
    if token.contains("$(") {
        return None;
    }
    let property = |name: &str| properties?.get(name)?.as_str();
    let game_profile = parser_type == "minecraft:game_profile";
    let only_players =
        game_profile || (parser_type == "minecraft:entity" && property("type") == Some("players"));
    let only_one = matches!(parser_type, "minecraft:entity" | "minecraft:score_holder")
        && property("amount") == Some("single");
    if !only_players && !only_one {
        return None;
    }

    if !token.starts_with('@') {
        let is_uuid = snbt::is_uuid(unquoted(token));
        return (only_players && !game_profile && is_uuid).then(|| {
            format!("`{token}` is a UUID, which only names an entity: this takes players")
        });
    }
    let variable = token.chars().nth(1)?;
    let arguments = selector_arguments(token).ok()?;
    let value_of = |key: &str| {
        arguments
            .iter()
            .find(|(name, _)| name == key)
            .map(|(_, value)| value.as_str())
    };
    let more_than_one = match value_of("limit") {
        Some(limit) => limit.parse::<i32>().is_ok_and(|limit| limit > 1),
        None => matches!(variable, 'a' | 'e'),
    };
    if only_one && more_than_one {
        return Some(format!(
            "`{token}` can select more than one target, and this takes one: use `limit=1`"
        ));
    }
    // `@s` passes as the one running the command, except for a game profile.
    // A `type` of player or an option that only players have keeps others out.
    let names_players = matches!(
        value_of("type"),
        Some("player" | ":player" | "minecraft:player")
    ) || value_of("level").is_some()
        || value_of("gamemode").is_some()
        || value_of("advancements").is_some_and(|value| value.contains('='));
    let any_entity = matches!(variable, 'e' | 'n') || (game_profile && variable == 's');
    (only_players && any_entity && !names_players).then(|| {
        format!("`{token}` can select other entities, and this takes players: use `type=player`")
    })
}

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
        "minecraft:entity" => entity_name_problem(token).or_else(|| selector_problem(token)),
        "minecraft:score_holder" | "minecraft:game_profile" => selector_problem(token),
        "minecraft:message" => message_selectors(token)
            .iter()
            .find_map(|selector| selector_problem(selector)),
        "minecraft:nbt_compound_tag" => compound_problem(token),
        "minecraft:style" => {
            compound_problem(token).or_else(|| text_component::style_problem(token))
        }
        "minecraft:nbt_tag" => snbt::parse(token).err(),
        _ => None,
    }
}

/// What stops a word from naming an entity: a player's name has 1 to 16
/// characters of Java's, and a UUID goes by other rules.
fn entity_name_problem(token: &str) -> Option<String> {
    if token.starts_with('@') {
        return None;
    }
    let name = StringReader::new(token).read_string()?;
    let fits = !name.is_empty() && name.encode_utf16().count() <= 16;
    (!fits && !snbt::is_uuid(&name))
        .then(|| format!("`{token}` can't name a target: a player name has 1 to 16 characters"))
}

/// What stops `token` from being read as an SNBT compound.
pub(super) fn compound_problem(token: &str) -> Option<String> {
    match snbt::parse(token) {
        Ok(value) if value.is_object() => None,
        Ok(_) => Some(format!("`{token}` must be a compound (`{{...}}`)")),
        Err(problem) => Some(problem),
    }
}

fn coordinates_problem(token: &str, integers: bool, local: bool) -> Option<String> {
    // Nothing between two spaces is a coordinate too.
    let parts: Vec<&str> = token.split(' ').collect();
    let locals = parts.iter().filter(|part| part.starts_with('^')).count();
    if locals > 0 && !local {
        return Some(format!("`{token}` can't use `^` coordinates"));
    }
    if locals > 0 && locals != parts.len() {
        return Some(format!("`{token}` mixes `^` with other coordinates"));
    }
    if integers {
        let absolute = || {
            parts
                .iter()
                .filter(|part| !part.is_empty() && !part.starts_with(['~', '^']))
        };
        if let Some(part) = absolute().find(|part| part.contains('.')) {
            return Some(format!("Block coordinate `{part}` must be a whole number"));
        }
        if let Some(part) = absolute().find(|part| part.parse::<i32>().is_err()) {
            return Some(format!("Block coordinate `{part}` is past 32 bits"));
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
            Some('{') => first && read_nbt_compound(&mut reader),
            Some('[') => read_nbt_index(&mut reader),
            Some('"' | '\'') => {
                reader.read_string().is_some_and(|key| !key.is_empty())
                    && read_nbt_filter(&mut reader)
            }
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
            if !read_nbt_compound(reader) {
                return false;
            }
        }
        _ => {
            let start = reader.cursor();
            while reader.peek().is_some_and(|ch| ch != ']') {
                reader.read_char();
            }
            // Java's parser takes a `+` too, but it never gets to see one.
            let index = reader.slice(start, reader.cursor());
            if index.starts_with('+') || index.parse::<i32>().is_err() {
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
    reader.peek() != Some('{') || read_nbt_compound(reader)
}

fn read_nbt_compound(reader: &mut StringReader) -> bool {
    let start = reader.cursor();
    reader.read_nbt() && compound_problem(&reader.slice(start, reader.cursor())).is_none()
}

/// The arguments of a selector with their values, or what stops Minecraft from
/// reading them. Follows its `EntitySelectorParser`.
pub(super) fn selector_arguments(selector: &str) -> Result<Vec<(String, String)>, String> {
    let options = selector.get(2..).unwrap_or_default();
    if options.is_empty() {
        return Ok(Vec::new());
    }
    read_selector_arguments(&mut StringReader::new(options))
}

/// The selectors with arguments in a message. Minecraft reads `@` and a
/// selector's letter as one wherever they stand, and a selector with an error
/// stops the command. One that doesn't read takes up the rest of the text.
pub(super) fn message_selectors(message: &str) -> Vec<String> {
    let mut selectors = Vec::new();
    let mut reader = StringReader::new(message);
    while let Some(ch) = reader.read_char() {
        let start = reader.cursor() - 1;
        if ch != '@' || !reader.peek().is_some_and(is_selector_variable) {
            continue;
        }
        reader.read_char();
        if reader.peek() == Some('[') {
            if read_selector_arguments(&mut reader).is_err() {
                reader.read_greedy();
            }
            selectors.push(reader.slice(start, reader.cursor()));
        }
    }
    selectors
}

fn is_selector_variable(ch: char) -> bool {
    matches!(ch, 'a' | 'e' | 'n' | 'p' | 'r' | 's')
}

/// The `[...]` at the cursor of `reader`.
fn read_selector_arguments(reader: &mut StringReader) -> Result<Vec<(String, String)>, String> {
    let mut arguments = Vec::new();
    reader.read_char();
    reader.skip_whitespace();
    while reader.can_read() && reader.peek() != Some(']') {
        reader.skip_whitespace();
        let Some(key) = reader.read_string() else {
            return Err("A selector argument has a broken name".to_string());
        };
        if key.is_empty() {
            return Err("A selector argument has no name".to_string());
        }
        if !is_selector_argument(&key) {
            return Err(format!("`{key}` is not a selector argument"));
        }
        reader.skip_whitespace();
        if reader.peek() != Some('=') {
            return Err(format!("Selector argument `{key}` needs a value"));
        }
        reader.read_char();
        reader.skip_whitespace();
        let start = reader.cursor();
        read_selector_value(reader, &key)?;
        arguments.push((key.clone(), reader.slice(start, reader.cursor())));
        reader.skip_whitespace();
        match reader.peek() {
            Some(',') => {
                reader.read_char();
            }
            Some(']') | None => break,
            Some(found) => return Err(format!("The value of `{key}` can't hold `{found}`")),
        }
    }
    if reader.read_char() != Some(']') {
        return Err("A selector's arguments aren't closed".to_string());
    }
    Ok(arguments)
}

fn is_selector_argument(key: &str) -> bool {
    matches!(
        key,
        "x" | "y"
            | "z"
            | "dx"
            | "dy"
            | "dz"
            | "distance"
            | "scores"
            | "tag"
            | "team"
            | "limit"
            | "sort"
            | "level"
            | "gamemode"
            | "name"
            | "x_rotation"
            | "y_rotation"
            | "type"
            | "nbt"
            | "predicate"
            | "advancements"
    )
}

/// One `!`, which spaces can follow.
fn read_negation(reader: &mut StringReader) {
    if reader.peek() == Some('!') {
        reader.read_char();
        reader.skip_whitespace();
    }
}

/// An id, which can be empty. Returns whether what it read is one.
pub(super) fn read_id(reader: &mut StringReader) -> bool {
    let start = reader.cursor();
    while reader.peek().is_some_and(|ch| {
        ch.is_ascii_lowercase() || ch.is_ascii_digit() || matches!(ch, '_' | '.' | '-' | '/' | ':')
    }) {
        reader.read_char();
    }
    is_id(&reader.slice(start, reader.cursor()))
}

fn read_selector_value(reader: &mut StringReader, key: &str) -> Result<(), String> {
    if reader.peek() == Some('!') && PLAIN_ARGUMENTS.contains(&key) {
        return Err(format!("Selector argument `{key}` can't be negated"));
    }
    read_negation(reader);
    let takes = |what: &str, valid: bool| {
        valid
            .then_some(())
            .ok_or_else(|| format!("Selector argument `{key}` takes {what}"))
    };
    match key {
        "name" => takes("a name", reader.read_string().is_some()),
        "tag" | "team" => {
            reader.read_unquoted_string();
            Ok(())
        }
        "gamemode" => takes(
            "survival, creative, adventure, or spectator",
            matches!(
                reader.read_unquoted_string().as_str(),
                "survival" | "creative" | "adventure" | "spectator"
            ),
        ),
        "sort" => takes(
            "nearest, furthest, random, or arbitrary",
            matches!(
                reader.read_unquoted_string().as_str(),
                "nearest" | "furthest" | "random" | "arbitrary"
            ),
        ),
        "limit" => takes(
            "a whole number from 1",
            reader
                .read_integer()
                .is_some_and(|limit| (1..=i64::from(i32::MAX)).contains(&limit)),
        ),
        "distance" | "x_rotation" | "y_rotation" => {
            takes("a number or a range", reader.read_float_range())
        }
        "level" => takes("a whole number or a range", reader.read_int_range()),
        "type" => {
            if reader.peek() == Some('#') {
                reader.read_char();
            }
            takes("an entity type or a `#` tag", read_id(reader))
        }
        "predicate" if reader.peek() == Some('{') => {
            Err("`predicate` takes the id of a predicate, not an inline one".to_string())
        }
        "predicate" => takes("the id of a predicate", read_id(reader)),
        "nbt" => takes(
            "a compound (`{...}`)",
            reader.peek() == Some('{') && reader.read_nbt(),
        ),
        "scores" => read_entries(reader, key, read_unquoted_key, read_score_range),
        "advancements" => read_entries(reader, key, read_id, read_advancement_done),
        _ => takes("a number", reader.read_float().is_some()),
    }
}

/// `{key=value,...}`, where a comma between two entries is optional.
fn read_entries(
    reader: &mut StringReader,
    argument: &str,
    read_key: fn(&mut StringReader) -> bool,
    read_value: fn(&mut StringReader) -> Result<(), String>,
) -> Result<(), String> {
    let invalid = || format!("`{argument}` takes `{{key=value,...}}`");
    if reader.peek() != Some('{') {
        return Err(invalid());
    }
    reader.read_char();
    reader.skip_whitespace();
    while reader.can_read() && reader.peek() != Some('}') {
        reader.skip_whitespace();
        if !read_key(reader) {
            return Err(invalid());
        }
        reader.skip_whitespace();
        if reader.peek() != Some('=') {
            return Err(invalid());
        }
        reader.read_char();
        reader.skip_whitespace();
        read_value(reader)?;
        reader.skip_whitespace();
        if reader.peek() == Some(',') {
            reader.read_char();
        }
    }
    if reader.peek() != Some('}') {
        return Err(invalid());
    }
    reader.read_char();
    Ok(())
}

fn read_unquoted_key(reader: &mut StringReader) -> bool {
    reader.read_unquoted_string();
    true
}

fn read_score_range(reader: &mut StringReader) -> Result<(), String> {
    let start = reader.cursor();
    if !reader.read_int_range() {
        return Err("A score in `scores` needs a value or a range".to_string());
    }
    range_problem(&reader.slice(start, reader.cursor())).map_or(Ok(()), Err)
}

/// `true`, `false`, or `{criterion=true,...}`.
fn read_advancement_done(reader: &mut StringReader) -> Result<(), String> {
    if reader.peek() == Some('{') {
        return read_entries(reader, "advancements", read_unquoted_key, read_done_flag);
    }
    read_done_flag(reader)
}

fn read_done_flag(reader: &mut StringReader) -> Result<(), String> {
    matches!(reader.read_string().as_deref(), Some("true" | "false"))
        .then_some(())
        .ok_or_else(|| "`advancements` needs true or false".to_string())
}

fn selector_problem(selector: &str) -> Option<String> {
    let rest = selector.strip_prefix('@')?;
    let variable = rest.chars().next()?;
    if !is_selector_variable(variable) {
        return Some(format!("`@{variable}` is not a selector"));
    }
    let arguments = match selector_arguments(selector) {
        Ok(arguments) => arguments,
        Err(problem) => return Some(problem),
    };
    let mut seen: Vec<(&str, bool)> = Vec::new();
    let mut one_type = false;
    for (key, value) in &arguments {
        let key = key.as_str();
        let negated = value.starts_with('!');
        let plain_value = value
            .strip_prefix('!')
            .unwrap_or(value)
            .trim_start_matches(is_java_whitespace);
        if variable == 's' && matches!(key, "limit" | "sort") {
            return Some(format!("`@s` can't take `{key}`"));
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
        if key == "type" {
            // Tags and negated types combine freely, but one named type
            // can't share a selector with any other `type`.
            let names_one = !negated && !plain_value.starts_with('#');
            if (names_one || one_type) && seen.iter().any(|(name, _)| *name == "type") {
                return Some(
                    "A selector that names one `type` can't have another `type`".to_string(),
                );
            }
            one_type |= names_one;
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
            // `-0` counts as zero.
            let negative = |end: &str| end.parse::<f64>().is_ok_and(|end| end < 0.0);
            if value.split("..").any(negative) {
                return Some(format!("`{key}` can't be negative"));
            }
            range_problem(value)
        }
        "nbt" => compound_problem(value),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::{problem, target_problem};

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
            ("minecraft:vec3", " 1 2"),
            ("minecraft:vec3", "~  ~"),
            ("minecraft:block_pos", "1 2 "),
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
            ("minecraft:entity", "@e[type=#a:b,type=!zombie]"),
            ("minecraft:entity", "@e[type=#a:b,type=#c:d]"),
            ("minecraft:entity", "@e[type=!zombie,type=#a:b]"),
            ("minecraft:entity", "@e[type=!#a:b,type=#c:d]"),
            ("minecraft:entity", "@e[type=#a:b,type=!#c:d]"),
            ("minecraft:entity", "@e[nbt={a:1},nbt={b:2}]"),
            ("minecraft:entity", "@e[nbt=!{a:1}]"),
            ("minecraft:nbt_tag", "0x10"),
            ("minecraft:nbt_tag", "1_000"),
            ("minecraft:nbt_tag", "1ub"),
            ("minecraft:nbt_tag", "bool( 1 )"),
            ("minecraft:nbt_tag", "$(value)"),
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
            ("minecraft:vec3", " ^ ^", "mixes"),
            ("minecraft:vec3", "^  ^", "mixes"),
            ("minecraft:block_pos", "^ ^ ", "mixes"),
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
            ("minecraft:entity", "@e[tag=a b]", "can't hold `b`"),
            ("minecraft:entity", "@e[tag=\"a\"]", "can't hold `\"`"),
            ("minecraft:entity", "@e[team=\"a b\"]", "can't hold `\"`"),
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
                "names one `type`",
            ),
            (
                "minecraft:entity",
                "@e[type=zombie,type=!skeleton]",
                "names one `type`",
            ),
            (
                "minecraft:entity",
                "@e[type=!zombie,type=skeleton]",
                "names one `type`",
            ),
            (
                "minecraft:entity",
                "@e[type=#minecraft:skeletons,type=zombie]",
                "names one `type`",
            ),
            (
                "minecraft:entity",
                "@e[type=zombie,type=#minecraft:skeletons]",
                "names one `type`",
            ),
            ("minecraft:entity", "@e[nbt=1]", "`nbt` takes a compound"),
            ("minecraft:entity", "@e[nbt={a:1c:2}]", "Invalid SNBT"),
            (
                "minecraft:entity",
                "@e[predicate={type:\"minecraft:random_chance\",chance:0.5}]",
                "not an inline one",
            ),
            ("minecraft:nbt_compound_tag", "{a:1c:2}", "Invalid SNBT"),
            ("minecraft:nbt_compound_tag", "[1]", "must be a compound"),
            ("minecraft:nbt_tag", "1abc", "Invalid SNBT"),
            ("minecraft:nbt_tag", "256b", "out of range"),
            ("minecraft:nbt_path", "a{b:1c:2}.c", "not a valid NBT path"),
            ("minecraft:nbt_path", "a[{b:1c:2}]", "not a valid NBT path"),
            ("minecraft:nbt_path", "{b:1c:2}", "not a valid NBT path"),
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

    #[test]
    fn selector_values_minecraft_accepts_pass() {
        for selector in [
            "@e[]",
            "@e[ ]",
            "@e[tag=a,]",
            "@e[ tag = a , limit = 1 ]",
            "@e[tag=a ,limit=1]",
            "@e[tag=]",
            "@e[tag=!]",
            "@e[tag=!a]",
            "@e[tag=! a]",
            "@e[tag= !a]",
            "@e[tag=a.b+c-d_e]",
            "@e[team=!]",
            "@e[name=]",
            "@e[name=!]",
            "@e[name=\"\"]",
            "@e[name=\"a:b\"]",
            "@e[name=!\"a b\"]",
            "@e[name='a b']",
            "@e[name='a]b']",
            // A quoted name reads like a plain one.
            "@e[\"tag\"=a]",
            "@e['limit'=1]",
            "@e[limit=01]",
            "@e[limit=2147483647]",
            "@e[x=.5]",
            "@e[x=5.]",
            "@e[dy=-1.5]",
            "@e[distance=.5]",
            "@e[distance=.5...7]",
            "@e[y_rotation=1...5]",
            "@e[x_rotation=-90]",
            "@e[type=#]",
            "@e[type=!#a:b]",
            "@e[nbt= {a:1}]",
            "@e[nbt={a:\"]\"}]",
            "@e[predicate=]",
            "@e[predicate=!]",
            "@e[predicate=!a]",
            "@e[predicate=a:b/c]",
            "@e[predicate=a:b,predicate=c:d]",
            // Commas between entries are optional, and a score needs no name.
            "@e[scores={}]",
            "@e[scores={ }]",
            "@e[scores={=1}]",
            "@e[scores={a=1,}]",
            "@e[scores={a=1 b=2}]",
            "@e[scores={ a = 1 }]",
            "@e[scores={a.b=1..2}]",
            "@e[advancements={}]",
            "@e[advancements={a=true}]",
            "@e[advancements={a:b/c=false}]",
            "@e[advancements={a:b=\"true\"}]",
            "@e[advancements={ a:b = true }]",
            "@e[advancements={a:b=true c:d=false}]",
            "@e[advancements={a:b={}}]",
            "@e[advancements={a:b={c=true,d=false,}}]",
            "@e[advancements={a:b={c=true d=false}}]",
            "@n[limit=1]",
            "@a[sort=random]",
        ] {
            assert_eq!(problem("minecraft:entity", selector), None, "{selector}");
        }
    }

    #[test]
    fn selector_values_minecraft_rejects_are_reported() {
        for (selector, expected) in [
            ("@x", "`@x` is not a selector"),
            ("@e[,]", "no name"),
            ("@e[,tag=a]", "no name"),
            ("@e[tag=a,,limit=1]", "no name"),
            ("@e[tag=a, ]", "no name"),
            ("@e[\"\"=a]", "no name"),
            ("@e[foo=bar]", "`foo` is not a selector argument"),
            ("@e[Tag=a]", "`Tag` is not a selector argument"),
            ("@e[\"nope\"=a]", "`nope` is not a selector argument"),
            ("@e[tag]", "needs a value"),
            ("@e[tag,limit=1]", "needs a value"),
            ("@e[!tag=a]", "no name"),
            ("@e[\"limit\"=1,limit=2]", "can appear once"),
            ("@e[tag=!!a]", "can't hold `!`"),
            ("@e[tag=a:b]", "can't hold `:`"),
            ("@e[tag=a/b]", "can't hold `/`"),
            ("@e[tag=\u{e9}]", "can't hold `\u{e9}`"),
            ("@e[team=!!a]", "can't hold `!`"),
            ("@e[team=a:b]", "can't hold `:`"),
            ("@e[name=!!a]", "can't hold `!`"),
            ("@e[name=a:b]", "can't hold `:`"),
            ("@e[name=a b]", "can't hold `b`"),
            ("@e[name=\"a\"b]", "can't hold `b`"),
            ("@e[gamemode=!!survival]", "`gamemode` takes"),
            ("@e[gamemode=]", "`gamemode` takes"),
            ("@e[gamemode=\"survival\"]", "`gamemode` takes"),
            ("@e[sort=]", "`sort` takes"),
            ("@e[sort=\"nearest\"]", "`sort` takes"),
            ("@e[sort=!nearest]", "can't be negated"),
            ("@e[limit=+1]", "`limit` takes"),
            ("@e[limit=0]", "`limit` takes"),
            ("@e[limit=1.0]", "`limit` takes"),
            ("@e[limit=]", "`limit` takes"),
            ("@e[limit=2147483648]", "`limit` takes"),
            ("@e[limit=1e1]", "can't hold `e`"),
            ("@e[x=]", "`x` takes a number"),
            ("@e[x=+1]", "`x` takes a number"),
            ("@e[x=1e3]", "can't hold `e`"),
            ("@e[x=1E3]", "can't hold `E`"),
            ("@e[x=NaN]", "`x` takes a number"),
            ("@e[x=inf]", "`x` takes a number"),
            ("@e[x=1.2.3]", "`x` takes a number"),
            ("@e[x=!1]", "can't be negated"),
            ("@e[dz=1e1]", "can't hold `e`"),
            ("@e[distance=]", "`distance` takes"),
            ("@e[distance=..]", "`distance` takes"),
            ("@e[distance=.]", "`distance` takes"),
            ("@e[distance=+1]", "`distance` takes"),
            ("@e[distance=1..+2]", "can't hold `+`"),
            ("@e[distance=1e1]", "can't hold `e`"),
            ("@e[distance=1..2..3]", "can't hold `.`"),
            ("@e[distance=1...5]", "low end above"),
            ("@e[level=]", "`level` takes"),
            ("@e[level=1.5]", "`level` takes"),
            ("@e[level=+1]", "`level` takes"),
            ("@e[x_rotation=!1]", "can't be negated"),
            ("@e[y_rotation=!1]", "can't be negated"),
            ("@e[x_rotation=+1]", "`x_rotation` takes"),
            ("@e[x_rotation=1e1]", "can't hold `e`"),
            ("@e[type=#!a:b]", "can't hold `!`"),
            ("@e[type=#A]", "can't hold `A`"),
            ("@e[type=a:b:c]", "`type` takes"),
            ("@e[type=a/b:c]", "`type` takes"),
            ("@e[type=#a/b:c]", "`type` takes"),
            ("@e[nbt=]", "`nbt` takes"),
            ("@e[nbt=[]]", "`nbt` takes"),
            ("@e[nbt=\"a\"]", "`nbt` takes"),
            ("@e[nbt=!!{}]", "`nbt` takes"),
            ("@e[predicate=A]", "can't hold `A`"),
            ("@e[predicate=a:B]", "can't hold `B`"),
            ("@e[predicate=#a:b]", "can't hold `#`"),
            ("@e[predicate=\"a:b\"]", "can't hold `\"`"),
            ("@e[predicate=!!a]", "can't hold `!`"),
            ("@e[predicate=a:b:c]", "`predicate` takes"),
            ("@e[predicate=a/b:c]", "`predicate` takes"),
            ("@e[scores=]", "`scores` takes"),
            ("@e[scores=a]", "`scores` takes"),
            ("@e[scores=!{}]", "can't be negated"),
            ("@e[scores={,a=1}]", "`scores` takes"),
            ("@e[scores={\"a\"=1}]", "`scores` takes"),
            ("@e[scores={a:b=1}]", "`scores` takes"),
            ("@e[scores={a/b=1}]", "`scores` takes"),
            ("@e[scores={a}]", "`scores` takes"),
            ("@e[scores={a=}]", "needs a value or a range"),
            ("@e[scores={a=..}]", "needs a value or a range"),
            ("@e[scores={a=1.5}]", "needs a value or a range"),
            ("@e[scores={a=+1}]", "needs a value or a range"),
            ("@e[advancements=]", "`advancements` takes"),
            ("@e[advancements=a]", "`advancements` takes"),
            ("@e[advancements=!{}]", "can't be negated"),
            ("@e[advancements={\"a:b\"=true}]", "`advancements` takes"),
            ("@e[advancements={A:b=true}]", "`advancements` takes"),
            ("@e[advancements={a:B=true}]", "`advancements` takes"),
            ("@e[advancements={a:b:c=true}]", "`advancements` takes"),
            ("@e[advancements={,a:b=true}]", "`advancements` takes"),
            ("@e[advancements={a:b=True}]", "needs true or false"),
            ("@e[advancements={a:b=1}]", "needs true or false"),
            ("@e[advancements={a:b={,c=true}}]", "`advancements` takes"),
            ("@e[advancements={a:b={c:d=true}}]", "`advancements` takes"),
            (
                "@e[advancements={a:b={\"c\"=true}}]",
                "`advancements` takes",
            ),
            ("@e[advancements={a:b={c=x}}]", "needs true or false"),
            ("@e[advancements={a:b={c=1}}]", "needs true or false"),
            ("@e[advancements={a:b={c={}}}]", "needs true or false"),
        ] {
            let found = problem("minecraft:entity", selector).unwrap_or_default();
            assert!(found.contains(expected), "{selector}: {found}");
        }
    }

    #[test]
    fn selectors_with_placeholders_are_known_when_the_function_runs() {
        for selector in [
            "@e[tag=a:$(x)]",
            "@e[limit=$(n)]",
            "@e[$(args)]",
            "@e[scores={$(a)=1}]",
            "@$(kind)",
        ] {
            assert_eq!(problem("minecraft:entity", selector), None, "{selector}");
            let single = serde_json::json!({"type": "players", "amount": "single"});
            assert_eq!(
                target_problem("minecraft:entity", Some(&single), selector),
                None,
                "{selector}"
            );
        }
    }

    #[test]
    fn player_arguments_take_players() {
        let players = serde_json::json!({"type": "players", "amount": "multiple"});
        let entities = serde_json::json!({"type": "entities", "amount": "multiple"});
        let player_slot = |token: &str| target_problem("minecraft:entity", Some(&players), token);

        for token in [
            "@e[type=player]",
            "@e[type=minecraft:player]",
            "@e[type=:player]",
            "@e[ type = player ]",
            "@e[\"type\"=player]",
            "@e[limit=1,type=player]",
            "@n[type=player]",
            // Only players have these options.
            "@e[level=1]",
            "@e[level=1..]",
            "@e[gamemode=survival]",
            "@e[gamemode=!survival]",
            "@e[advancements={a=true}]",
            "@n[level=1]",
            "@n[gamemode=survival]",
            "@e[type=!player,level=1]",
            "@e[type=pig,level=1]",
            // These four only ever select players, whatever `type` says.
            "@a",
            "@a[type=pig]",
            "@p[type=pig]",
            "@r[type=!player]",
            "@s",
            "@s[type=pig]",
            "Steve",
            "\"Steve\"",
            "a-b",
            // Not a UUID: Java's reader wants five groups that aren't empty.
            "g-0-0-0-0",
            "0-0-0-0",
            "0-0-0-0-0-0",
            "0--0-0-0",
            "-0-0-0-0",
            "0-+-0-0-0",
        ] {
            assert_eq!(player_slot(token), None, "{token}");
        }
        for token in [
            "@e",
            "@n",
            "@e[limit=1]",
            "@e[type=!player]",
            "@e[type=pig]",
            "@n[type=#minecraft:skeletons]",
            "@e[tag=player]",
            "@e[name=player]",
            "@e[nbt={type:player}]",
            "@e[advancements={}]",
            "@n[advancements={ }]",
            "@e[scores={a=1}]",
            "@e[team=a]",
            "@e[predicate=a:b]",
        ] {
            let found = player_slot(token).unwrap_or_default();
            assert!(found.contains("`type=player`"), "{token}: {found}");
        }
        for token in [
            "0-0-0-0-0",
            "\"0-0-0-0-0\"",
            "'0-0-0-0-0'",
            "a-b-c-d-e",
            "A-B-C-D-E",
            "+0-0-0-0-0",
            "0-0-0-0-+0",
            "7fffffffffffffff-0-0-0-0",
            "00000000-0000-0000-0000-000000000000",
        ] {
            let found = player_slot(token).unwrap_or_default();
            assert!(found.contains("is a UUID"), "{token}: {found}");
        }
        // Any entity goes in an argument that doesn't ask for players.
        for token in ["@e", "@n", "0-0-0-0-0"] {
            assert_eq!(
                target_problem("minecraft:entity", Some(&entities), token),
                None,
                "{token}"
            );
        }
    }

    #[test]
    fn game_profiles_take_players() {
        let parser = "minecraft:game_profile";
        for token in [
            "@a",
            "@p",
            "@r",
            "@a[limit=2]",
            "@e[type=player]",
            "@n[type=player]",
            "Steve",
            "a:b",
            "0-0-0-0-0",
        ] {
            assert_eq!(target_problem(parser, None, token), None, "{token}");
        }
        // Unlike an entity argument, this one takes no `@s`.
        for token in ["@e", "@n", "@s", "@e[limit=1]"] {
            let found = target_problem(parser, None, token).unwrap_or_default();
            assert!(found.contains("`type=player`"), "{token}: {found}");
        }
    }

    #[test]
    fn single_arguments_take_one_target() {
        let entity = serde_json::json!({"type": "entities", "amount": "single"});
        let player = serde_json::json!({"type": "players", "amount": "single"});
        let many = serde_json::json!({"type": "entities", "amount": "multiple"});
        let holder = serde_json::json!({"amount": "single"});
        let holders = serde_json::json!({"amount": "multiple"});
        let slots = [
            ("minecraft:entity", &entity),
            ("minecraft:entity", &player),
            ("minecraft:score_holder", &holder),
        ];
        for (parser, properties) in slots {
            for token in [
                "@s",
                "@p",
                "@r",
                "@a[limit=1]",
                "@a[limit=01]",
                "@p[limit=1]",
                "@a[limit=1,tag=a]",
                "@a[tag=a,limit=1]",
            ] {
                assert_eq!(
                    target_problem(parser, Some(properties), token),
                    None,
                    "{parser} {token}"
                );
            }
            for token in [
                "@a",
                "@a[limit=2]",
                "@p[limit=2]",
                "@r[limit=2]",
                "@a[tag=a]",
                "@a[limit=2,tag=a]",
                "@a[nbt={limit:1}]",
                "@a[name=\"limit=1\"]",
            ] {
                let found = target_problem(parser, Some(properties), token).unwrap_or_default();
                assert!(found.contains("`limit=1`"), "{parser} {token}: {found}");
            }
        }
        for properties in [&entity, &holder] {
            let parser = if properties == &holder {
                "minecraft:score_holder"
            } else {
                "minecraft:entity"
            };
            for token in ["@n", "@e[limit=1]"] {
                assert_eq!(target_problem(parser, Some(properties), token), None);
            }
            for token in ["@e", "@e[limit=2]", "@n[limit=2]"] {
                let found = target_problem(parser, Some(properties), token).unwrap_or_default();
                assert!(found.contains("`limit=1`"), "{parser} {token}: {found}");
            }
        }
        // `*` and plain names are one holder each, whatever they turn out to be.
        for token in ["*", "**", "a", "#a"] {
            assert_eq!(
                target_problem("minecraft:score_holder", Some(&holder), token),
                None,
                "{token}"
            );
        }
        assert_eq!(
            target_problem("minecraft:score_holder", Some(&holders), "@a"),
            None
        );
        assert_eq!(target_problem("minecraft:entity", Some(&many), "@a"), None);
    }

    #[test]
    fn entity_names_have_16_characters_or_read_as_uuids() {
        let parser = "minecraft:entity";
        for token in [
            "abcdefghijklmnop",
            "\"abcdefghijklmnop\"",
            "'a b'",
            "\"\u{d55c}\u{ae00}\"",
            // 16 of Java's characters: each emoji counts as two.
            "\"\u{1f600}\u{1f600}\u{1f600}\u{1f600}\u{1f600}\u{1f600}\u{1f600}\u{1f600}\"",
            "\"abcdefghijklmno\\\\\"",
            "0-0-0-0-0",
            "\"0-0-0-0-0\"",
            "0-0-0-0",
            "0-0-0-0-0-0",
            "7fffffffffffffff-0-0-0-0",
            "00000000-0000-0000-0000-000000000000",
        ] {
            assert_eq!(problem(parser, token), None, "{token}");
        }
        for token in [
            "abcdefghijklmnopq",
            "\"abcdefghijklmnopq\"",
            "'abcdefghijklmnopq'",
            "\"\"",
            "''",
            "\"\u{1f600}\u{1f600}\u{1f600}\u{1f600}\u{1f600}\u{1f600}\u{1f600}\u{1f600}\u{1f600}\"",
            "\"abcdefghijklmnop\\\\\"",
            // Not Java's form of a UUID: no groups, too long, or past a long.
            "00000000000000000000000000000000",
            "0-0-0-0-000000000000000000000000000000",
            "8000000000000000-0-0-0-0",
            "ffffffffffffffff-0-0-0-0",
        ] {
            assert_eq!(
                problem(parser, token),
                Some(format!(
                    "`{token}` can't name a target: a player name has 1 to 16 characters"
                )),
                "{token}"
            );
        }
        // Score holders and placeholders go by other rules.
        let long_name = "abcdefghijklmnopqrstuvwxyz";
        assert_eq!(problem("minecraft:score_holder", long_name), None);
        assert_eq!(problem(parser, "$(name)abcdefghijklmnopqrstuvwxyz"), None);
        assert_eq!(problem(parser, "\"$(name)\""), None);
    }

    #[test]
    fn selectors_in_messages_are_read() {
        let parser = "minecraft:message";
        for token in [
            "hi",
            "@a[tag=a]",
            "@a",
            "@e and more",
            "@e [limit=0]",
            "x @E[limit=0]",
            "@x[",
            "@[",
            "@",
            "a@x[limit=0]",
            "@e[name=\"a]\"] b",
            // Known only when the function runs.
            "@e[limit=$(n)]",
            "hi @a[ $(rest)",
            "$(a) @e[limit=0]",
            "@e[limit=0] $(rest)",
        ] {
            assert_eq!(problem(parser, token), None, "{token}");
        }
        for (token, expected) in [
            ("hi @a[", "A selector's arguments aren't closed"),
            ("@e[limit=1", "A selector's arguments aren't closed"),
            (
                "hi @e[limit=0] there",
                "Selector argument `limit` takes a whole number from 1",
            ),
            ("mail@e[tag=a:b]", "The value of `tag` can't hold `:`"),
            (
                "@a[tag=a]@e[limit=0]",
                "Selector argument `limit` takes a whole number from 1",
            ),
            (
                "@@e[limit=0]",
                "Selector argument `limit` takes a whole number from 1",
            ),
            (
                "\"@e[limit=0]\"",
                "Selector argument `limit` takes a whole number from 1",
            ),
            (
                "@e[name=\"a]\"] @e[limit=0]",
                "Selector argument `limit` takes a whole number from 1",
            ),
            ("@e[limit=1 ok]", "The value of `limit` can't hold `o`"),
            ("@e[foo=bar]", "`foo` is not a selector argument"),
            ("@s[limit=1]", "`@s` can't take `limit`"),
            ("@n[distance=-1]", "`distance` can't be negative"),
            (
                "@r[sort=x]",
                "Selector argument `sort` takes nearest, furthest, random, or arbitrary",
            ),
            (
                "@a[type=player,type=pig]",
                "A selector that names one `type` can't have another `type`",
            ),
            (
                "@a[nbt={a:1c:2}]",
                "Invalid SNBT: `1c` is not a number; quote it if it is text",
            ),
        ] {
            assert_eq!(problem(parser, token).as_deref(), Some(expected), "{token}");
        }
    }

    #[test]
    fn block_coordinates_are_32_bits() {
        for (parser, token) in [
            ("minecraft:block_pos", "2147483647 0 0"),
            ("minecraft:block_pos", "-2147483648 0 0"),
            ("minecraft:block_pos", "~2147483648 0 0"),
            ("minecraft:block_pos", "^2147483648 ^ ^"),
            ("minecraft:block_pos", " 2 "),
            ("minecraft:column_pos", "~2147483648 0"),
            ("minecraft:vec3", "2147483648 0 0"),
            ("minecraft:vec3", "99999999999999999999999999 0 0"),
        ] {
            assert_eq!(problem(parser, token), None, "{token}");
        }
        for (parser, token, number) in [
            ("minecraft:block_pos", "2147483648 0 0", "2147483648"),
            ("minecraft:block_pos", "-2147483649 0 0", "-2147483649"),
            ("minecraft:block_pos", "0 2147483648 0", "2147483648"),
            ("minecraft:block_pos", "0 0 99999999999", "99999999999"),
            ("minecraft:column_pos", "2147483648 0", "2147483648"),
            ("minecraft:column_pos", "0 2147483648", "2147483648"),
        ] {
            assert_eq!(
                problem(parser, token),
                Some(format!("Block coordinate `{number}` is past 32 bits")),
                "{token}"
            );
        }
    }

    #[test]
    fn negative_zero_is_zero_in_a_selector() {
        for selector in [
            "@e[distance=-0]",
            "@e[distance=-0..]",
            "@e[distance=..-0]",
            "@e[distance=-0..5]",
            "@e[distance=-0.0]",
            "@e[distance=-.0]",
            "@e[level=-0]",
            "@e[level=-0..5]",
            "@e[level=..-0]",
        ] {
            assert_eq!(problem("minecraft:entity", selector), None, "{selector}");
        }
        for (selector, key) in [
            ("@e[distance=-1..]", "distance"),
            ("@e[distance=..-1]", "distance"),
            ("@e[distance=-0.1..1]", "distance"),
            ("@e[level=-1..]", "level"),
            ("@e[level=..-1]", "level"),
        ] {
            assert_eq!(
                problem("minecraft:entity", selector),
                Some(format!("`{key}` can't be negative")),
                "{selector}"
            );
        }
    }

    #[test]
    fn nbt_path_keys_follow_brigadier() {
        let path = "minecraft:nbt_path";
        for token in [
            "$x",
            "a$b",
            "$",
            "a.$",
            "$(x)",
            "\"a\"",
            "'a\\'b'",
            "\"a\\\\b\"",
            "\"a\".b",
            "\"a\"[0]",
            "\"a\"{b:1}",
        ] {
            assert_eq!(problem(path, token), None, "{token}");
        }
        for token in [
            "\"\"",
            "''",
            "\"\".a",
            "a.\"\"",
            "a[0].\"\"",
            "\"\"[0]",
            "\"\\n\"",
            "\"a\\'b\"",
        ] {
            assert_eq!(
                problem(path, token),
                Some(format!("`{token}` is not a valid NBT path")),
                "{token}"
            );
        }
    }

    #[test]
    fn quoted_strings_of_selectors_follow_brigadier() {
        for selector in ["@e[name=\"a\\\\b\"]", "@e[name='a\\'b']", "@e[\"tag\"=a]"] {
            assert_eq!(problem("minecraft:entity", selector), None, "{selector}");
        }
        for (selector, expected) in [
            (
                "@e[name=\"a\\nb\"]",
                "Selector argument `name` takes a name",
            ),
            (
                "@e[name=\"a\\'b\"]",
                "Selector argument `name` takes a name",
            ),
            ("@e[\"tag\\n\"=a]", "A selector argument has a broken name"),
            (
                "@e[advancements={a:b=\"tr\\ue\"}]",
                "`advancements` needs true or false",
            ),
        ] {
            assert_eq!(
                problem("minecraft:entity", selector).as_deref(),
                Some(expected),
                "{selector}"
            );
        }
    }

    #[test]
    fn nbt_path_indexes_have_no_plus() {
        let path = "minecraft:nbt_path";
        for token in ["a[1]", "a[-1]", "a[0]"] {
            assert_eq!(problem(path, token), None, "{token}");
        }
        for token in ["a[+1]", "a[2147483648]", "a[1e1]"] {
            assert!(problem(path, token).is_some(), "{token}");
        }
    }
}
