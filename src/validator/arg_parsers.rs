use super::snbt;
use super::string_reader::StringReader;

/// Parse an argument according to the given Brigadier/Minecraft parser type.
/// Returns true and advances the cursor on success, or returns false with cursor unchanged.
pub fn parse_argument(
    reader: &mut StringReader,
    parser_type: &str,
    properties: Option<&serde_json::Value>,
) -> bool {
    // Handle whole-argument macro placeholders. Multi-token parsers try their
    // native parse first so `$(x) $(y) $(z)` is treated as one vec3, not as a
    // single macro followed by stray tokens.
    // Greedy parsers (`message`, greedy strings) take the rest of the line,
    // macros included, so they skip the single-token shortcut.
    if reader.peek() == Some('$')
        && !is_multi_token_parser(parser_type)
        && !is_greedy_parser(parser_type, properties)
    {
        let saved = reader.cursor();
        if reader.try_read_macro() {
            if reader.at_token_boundary() {
                return true;
            }
            reader.set_cursor(saved);
        }
    }

    let saved = reader.cursor();
    if parse_native(reader, parser_type, properties) {
        return true;
    }
    reader.set_cursor(saved);
    // In a macro line, a token that mixes text and placeholders, such as
    // `kills_$(mob)`, only gets its value when the function runs.
    reader.is_macro_line()
        && !is_multi_token_parser(parser_type)
        && !is_greedy_parser(parser_type, properties)
        && (reader.skip_token_with_macro() || reader.skip_open_word_before_macro())
}

fn parse_native(
    reader: &mut StringReader,
    parser_type: &str,
    properties: Option<&serde_json::Value>,
) -> bool {
    if takes_leading_spaces(parser_type) {
        reader.skip_whitespace();
    }
    match parser_type {
        // === Brigadier core types ===
        "brigadier:bool" => parse_bool(reader),
        "brigadier:integer" => parse_integer(reader, properties),
        "brigadier:float" => parse_brigadier_float(reader, properties),
        "brigadier:double" => parse_brigadier_double(reader, properties),
        "brigadier:string" => parse_brigadier_string(reader, properties),

        // === Simple word-accepting types ===
        "minecraft:objective" | "minecraft:team" => parse_word(reader),
        "minecraft:objective_criteria" => !reader.read_until_space().is_empty(),
        "minecraft:scoreboard_slot" => parse_scoreboard_slot(reader),
        "minecraft:swizzle" => parse_swizzle(reader),
        "minecraft:item_slot" => parse_item_slot(reader),
        "minecraft:item_slots" => parse_item_slots(reader),
        "minecraft:hex_color" => parse_hex_color(reader),
        // Team colors (`team modify <team> color`, `waypoint modify ... color`):
        // lowercase names only; `reset` is its own literal since 26.2.
        "minecraft:team_color" => parse_literal_word(
            reader,
            &[
                "black",
                "dark_blue",
                "dark_green",
                "dark_aqua",
                "dark_red",
                "dark_purple",
                "gold",
                "gray",
                "dark_gray",
                "blue",
                "green",
                "aqua",
                "red",
                "light_purple",
                "yellow",
                "white",
            ],
        ),
        "minecraft:gamemode" => {
            parse_literal_word(reader, &["survival", "creative", "adventure", "spectator"])
        }
        "minecraft:entity_anchor" => parse_literal_word(reader, &["eyes", "feet"]),
        "minecraft:heightmap" => parse_literal_word(
            reader,
            &[
                "world_surface",
                "motion_blocking",
                "motion_blocking_no_leaves",
                "ocean_floor",
            ],
        ),
        "minecraft:template_mirror" => {
            parse_literal_word(reader, &["none", "front_back", "left_right"])
        }
        "minecraft:template_rotation" => parse_literal_word(
            reader,
            &["none", "clockwise_90", "180", "counterclockwise_90"],
        ),

        // === Operations ===
        "minecraft:operation" => parse_operation(reader),

        // === Ranges ===
        "minecraft:int_range" => parse_int_range(reader),
        "minecraft:float_range" => parse_float_range(reader),

        // === Time ===
        "minecraft:time" => parse_time(reader, properties),

        // === Entity/player selectors ===
        "minecraft:entity" | "minecraft:score_holder" | "minecraft:game_profile" => {
            parse_entity(reader, parser_type)
        }

        // === Resource locations ===
        // These read an id and look it up later, so an empty one reads.
        "minecraft:resource_location" | "minecraft:dimension" | "minecraft:resource_key" => {
            reader.peek() == Some(' ') || parse_resource_location(reader)
        }
        "minecraft:mob_effect" | "minecraft:resource" => parse_resource_location(reader),

        // `ResourceOrIdArgument`s: a registry ID or an inline SNBT definition.
        "minecraft:loot_modifier"
        | "minecraft:loot_predicate"
        | "minecraft:loot_table"
        | "minecraft:dialog"
        | "minecraft:feature" => parse_resource_or_inline(reader),
        "minecraft:context_int_provider" | "minecraft:context_float_provider" => {
            parse_number_provider(reader)
        }
        // Slot range string (`container.0`, `container.*`, `weapon`) or a
        // `slot_source` ID / inline definition.
        "minecraft:slot_source" => parse_slot_source(reader),
        "minecraft:swing_animation" => parse_literal_word(reader, &["whack", "stab", "none"]),

        // `FunctionArgument` takes a function ID or a `#` function tag, and
        // an empty id, which it looks up later.
        "minecraft:function" => reader.peek() == Some(' ') || parse_resource_or_tag(reader),
        "minecraft:resource_or_tag" | "minecraft:resource_or_tag_key" => {
            parse_resource_or_tag(reader)
        }
        "minecraft:resource_selector" => parse_resource_selector(reader),

        // === Coordinates ===
        "minecraft:block_pos" | "minecraft:vec3" => parse_coordinates(reader, 3),
        "minecraft:column_pos" | "minecraft:vec2" | "minecraft:rotation" => {
            parse_coordinates(reader, 2)
        }

        // === Particle ===
        "minecraft:particle" => parse_particle(reader),

        // === UUID ===
        "minecraft:uuid" => parse_uuid(reader),

        // === JSON/NBT complex types ===
        "minecraft:component" | "minecraft:style" => parse_component(reader),
        "minecraft:nbt_compound_tag" => parse_nbt_compound(reader),
        "minecraft:nbt_tag" => parse_nbt_tag(reader),
        "minecraft:nbt_path" => parse_nbt_path(reader),

        // === Block/item with state/components ===
        "minecraft:block_state" | "minecraft:block_predicate" => parse_block_state(reader),
        "minecraft:item_stack" => parse_item_stack(reader),
        "minecraft:item_predicate" => parse_item_predicate(reader),

        // === Message (greedy) ===
        "minecraft:message" => parse_message(reader),

        // Unknown parser — accept any word liberally
        _ => {
            reader.read_until_space();
            true
        }
    }
}

/// Parsers that read SNBT skip spaces first, and so do item predicates.
fn takes_leading_spaces(parser_type: &str) -> bool {
    matches!(
        parser_type,
        "minecraft:component"
            | "minecraft:style"
            | "minecraft:nbt_tag"
            | "minecraft:nbt_compound_tag"
            | "minecraft:loot_table"
            | "minecraft:loot_modifier"
            | "minecraft:loot_predicate"
            | "minecraft:dialog"
            | "minecraft:feature"
            | "minecraft:context_int_provider"
            | "minecraft:context_float_provider"
            | "minecraft:item_predicate"
    )
}

fn is_greedy_parser(parser_type: &str, properties: Option<&serde_json::Value>) -> bool {
    parser_type == "minecraft:message"
        || (parser_type == "brigadier:string"
            && properties
                .and_then(|properties| properties.get("type"))
                .and_then(|kind| kind.as_str())
                == Some("greedy"))
}

fn is_multi_token_parser(parser_type: &str) -> bool {
    matches!(
        parser_type,
        "minecraft:block_pos"
            | "minecraft:column_pos"
            | "minecraft:vec2"
            | "minecraft:rotation"
            | "minecraft:vec3"
    )
}

// ============================================================================
// Brigadier core types
// ============================================================================

/// Brigadier reads a boolean as a string, so it can have quotes.
fn parse_bool(reader: &mut StringReader) -> bool {
    let saved = reader.cursor();
    let value = reader.read_string();
    (matches!(value.as_deref(), Some("true" | "false")) && reader.at_token_boundary())
        || reset(reader, saved)
}

fn parse_integer(reader: &mut StringReader, properties: Option<&serde_json::Value>) -> bool {
    let saved = reader.cursor();
    if let Some(value) = reader.read_integer() {
        if !integer_in_bounds(value, properties) {
            reader.set_cursor(saved);
            return false;
        }
        if reader.at_token_boundary() {
            return true;
        }
    }
    reader.set_cursor(saved);
    false
}

fn integer_in_bounds(value: i64, properties: Option<&serde_json::Value>) -> bool {
    if i32::try_from(value).is_err() {
        return false;
    }
    let min = properties
        .and_then(|p| p.get("min"))
        .and_then(|value| value.as_i64());
    let max = properties
        .and_then(|p| p.get("max"))
        .and_then(|value| value.as_i64());

    if let Some(min) = min {
        if value < min {
            return false;
        }
    }
    if let Some(max) = max {
        if value > max {
            return false;
        }
    }
    true
}

fn number_bound(properties: Option<&serde_json::Value>, name: &str) -> Option<f64> {
    properties?.get(name)?.as_f64()
}

/// A float reads as 32 bits before it meets its bounds, and too many digits
/// make it infinite, which is above any maximum.
fn parse_brigadier_float(
    reader: &mut StringReader,
    properties: Option<&serde_json::Value>,
) -> bool {
    let saved = reader.cursor();
    let in_bounds = reader.read_f32().is_some_and(|value| {
        value.is_finite()
            && number_bound(properties, "min").is_none_or(|min| value >= min as f32)
            && number_bound(properties, "max").is_none_or(|max| value <= max as f32)
    });
    (in_bounds && reader.at_token_boundary()) || reset(reader, saved)
}

fn parse_brigadier_double(
    reader: &mut StringReader,
    properties: Option<&serde_json::Value>,
) -> bool {
    let saved = reader.cursor();
    let in_bounds = reader.read_float().is_some_and(|value| {
        value.is_finite()
            && number_bound(properties, "min").is_none_or(|min| value >= min)
            && number_bound(properties, "max").is_none_or(|max| value <= max)
    });
    (in_bounds && reader.at_token_boundary()) || reset(reader, saved)
}

fn parse_brigadier_string(
    reader: &mut StringReader,
    properties: Option<&serde_json::Value>,
) -> bool {
    let string_type = properties
        .and_then(|p| p.get("type"))
        .and_then(|t| t.as_str())
        .unwrap_or("word");

    match string_type {
        "greedy" => {
            if reader.can_read() {
                reader.read_greedy();
                true
            } else {
                false
            }
        }
        "phrase" => {
            // Quotable phrase: quoted string or single word
            let saved = reader.cursor();
            if reader.read_string().is_some() && reader.at_token_boundary() {
                true
            } else {
                reader.set_cursor(saved);
                false
            }
        }
        // "word" — single unquoted word
        _ => parse_word(reader),
    }
}

// ============================================================================
// Simple word parser
// ============================================================================

/// An unquoted word, which can be empty.
fn parse_word(reader: &mut StringReader) -> bool {
    let saved = reader.cursor();
    reader.read_unquoted_string();
    if reader.at_token_boundary() {
        return true;
    }
    reader.set_cursor(saved);
    false
}

fn parse_literal_word(reader: &mut StringReader, allowed: &[&str]) -> bool {
    let saved = reader.cursor();
    let value = reader.read_unquoted_string();
    if allowed.contains(&value.as_str()) && reader.at_token_boundary() {
        return true;
    }
    reader.set_cursor(saved);
    false
}

fn parse_scoreboard_slot(reader: &mut StringReader) -> bool {
    let saved = reader.cursor();
    let value = reader.read_unquoted_string();
    let valid = matches!(value.as_str(), "list" | "sidebar" | "below_name")
        || value
            .strip_prefix("sidebar.team.")
            .is_some_and(is_scoreboard_team_color);

    if valid && reader.at_token_boundary() {
        return true;
    }
    reader.set_cursor(saved);
    false
}

fn is_scoreboard_team_color(value: &str) -> bool {
    matches!(
        value,
        "black"
            | "dark_blue"
            | "dark_green"
            | "dark_aqua"
            | "dark_red"
            | "dark_purple"
            | "gold"
            | "gray"
            | "dark_gray"
            | "blue"
            | "green"
            | "aqua"
            | "red"
            | "light_purple"
            | "yellow"
            | "white"
    )
}

fn parse_swizzle(reader: &mut StringReader) -> bool {
    let saved = reader.cursor();
    let value = reader.read_unquoted_string();
    let mut seen = [false; 3];

    if value.is_empty() || value.len() > 3 {
        reader.set_cursor(saved);
        return false;
    }

    for ch in value.chars() {
        let index = match ch {
            'x' => 0,
            'y' => 1,
            'z' => 2,
            _ => {
                reader.set_cursor(saved);
                return false;
            }
        };
        if seen[index] {
            reader.set_cursor(saved);
            return false;
        }
        seen[index] = true;
    }

    if reader.at_token_boundary() {
        return true;
    }
    reader.set_cursor(saved);
    false
}

fn parse_item_slot(reader: &mut StringReader) -> bool {
    parse_item_slot_value(reader, false)
}

fn parse_item_slots(reader: &mut StringReader) -> bool {
    parse_item_slot_value(reader, true)
}

fn parse_item_slot_value(reader: &mut StringReader, allow_groups: bool) -> bool {
    let saved = reader.cursor();
    if is_item_slot_value(&reader.read_until_space(), allow_groups) {
        return true;
    }
    reader.set_cursor(saved);
    false
}

pub(super) fn is_item_slot_value(value: &str, allow_groups: bool) -> bool {
    if value.is_empty() {
        return false;
    }

    // The names of `SlotRanges` in Minecraft 26.3.
    if allow_groups
        && matches!(
            value,
            "armor.*"
                | "container.*"
                | "enderchest.*"
                | "horse.*"
                | "hotbar.*"
                | "inventory.*"
                | "mob.inventory.*"
                | "player.crafting.*"
                | "weapon.*"
        )
    {
        return true;
    }

    if matches!(
        value,
        "contents"
            | "weapon"
            | "weapon.mainhand"
            | "weapon.offhand"
            | "armor.head"
            | "armor.chest"
            | "armor.legs"
            | "armor.feet"
            | "armor.body"
            | "saddle"
            | "horse.chest"
            | "player.cursor"
    ) {
        return true;
    }

    let Some((prefix, index)) = value.rsplit_once('.') else {
        return false;
    };
    // Slots are fixed names, so `hotbar.08` and `hotbar.+8` aren't any.
    let Ok(index) = index.parse::<u32>() else {
        return false;
    };
    if value != format!("{prefix}.{index}") {
        return false;
    }

    match prefix {
        "container" => index <= 53,
        "hotbar" => index <= 8,
        "inventory" => index <= 26,
        "enderchest" => index <= 26,
        "mob.inventory" => index <= 7,
        "horse" => index <= 14,
        "player.crafting" => index <= 3,
        _ => false,
    }
}

/// Three or six characters, read one or two at a time as hex numbers, which
/// can have a sign in Java as they can here.
fn parse_hex_color(reader: &mut StringReader) -> bool {
    let saved = reader.cursor();
    let color = reader.read_unquoted_string();
    let valid = matches!(color.len(), 3 | 6)
        && color.as_bytes().chunks(color.len() / 3).all(|group| {
            std::str::from_utf8(group).is_ok_and(|group| i32::from_str_radix(group, 16).is_ok())
        });
    (valid && reader.at_token_boundary()) || reset(reader, saved)
}

// ============================================================================
// Operation parser
// ============================================================================

fn parse_operation(reader: &mut StringReader) -> bool {
    let saved = reader.cursor();
    let ops = ["+=", "-=", "*=", "/=", "%=", "><", "=", "<", ">"];
    for op in ops {
        reader.set_cursor(saved);
        let chars: Vec<char> = op.chars().collect();
        let mut matched = true;
        for &ch in &chars {
            if reader.peek() == Some(ch) {
                reader.read_char();
            } else {
                matched = false;
                break;
            }
        }
        if matched && reader.at_token_boundary() {
            return true;
        }
    }
    reader.set_cursor(saved);
    false
}

// ============================================================================
// Range parsers
// ============================================================================

fn parse_int_range(reader: &mut StringReader) -> bool {
    let saved = reader.cursor();
    (reader.read_int_range() && reader.at_token_boundary()) || reset(reader, saved)
}

fn parse_float_range(reader: &mut StringReader) -> bool {
    let saved = reader.cursor();
    (reader.read_float_range() && reader.at_token_boundary()) || reset(reader, saved)
}

// ============================================================================
// Time parser
// ============================================================================

/// Java's `Math.round`, which takes halves up and stops at the ends of `i32`.
fn java_round(value: f32) -> i32 {
    let floor = value.floor();
    (if value - floor >= 0.5 {
        floor + 1.0
    } else {
        floor
    }) as i32
}

/// A float and a unit. The ticks they round to have to reach the minimum.
fn parse_time(reader: &mut StringReader, properties: Option<&serde_json::Value>) -> bool {
    let saved = reader.cursor();
    let Some(value) = reader.read_f32() else {
        return false;
    };
    let ticks_per_unit = match reader.read_unquoted_string().as_str() {
        "" | "t" => 1.0,
        "s" => 20.0,
        "d" => 24000.0,
        _ => return reset(reader, saved),
    };
    let ticks = java_round(value * ticks_per_unit);
    (number_bound(properties, "min").is_none_or(|min| f64::from(ticks) >= min)
        && reader.at_token_boundary())
        || reset(reader, saved)
}

// ============================================================================
// Entity/selector parsers
// ============================================================================

fn parse_entity(reader: &mut StringReader, parser_type: &str) -> bool {
    let saved = reader.cursor();

    // Try selector (@a, @s, @p, @e, @r, etc.). `values` checks what is in it.
    if reader.peek() == Some('@') {
        if reader.read_selector() && reader.at_token_boundary() {
            return true;
        }
        reader.set_cursor(saved);
    }

    // `ScoreHolderArgument` takes everything up to the next space as a name,
    // which is why `$x` and `#a>b` are common fake player names.
    if parser_type == "minecraft:score_holder" {
        if reader.peek() == Some('@') {
            return false;
        }
        reader.read_until_space();
        return true;
    }

    if parser_type == "minecraft:entity" {
        reader.set_cursor(saved);
        if reader.try_read_macro() {
            if reader.can_read() && reader.peek() == Some('[') && !reader.read_nbt() {
                reader.set_cursor(saved);
                return false;
            }
            if reader.at_token_boundary() {
                return true;
            }
        }
        reader.set_cursor(saved);
    }

    // `GameProfileArgument` also takes everything up to the next space.
    if parser_type == "minecraft:game_profile" {
        reader.set_cursor(saved);
        if reader.try_read_macro() {
            if reader.can_read() && reader.peek() == Some('[') && !reader.read_nbt() {
                reader.set_cursor(saved);
                return false;
            }
            if reader.at_token_boundary() {
                return true;
            }
        }
        reader.set_cursor(saved);
        return reader.peek() != Some('@') && !reader.read_until_space().is_empty();
    }

    // A name or a UUID, which can have quotes. `values` checks what it holds.
    let read = reader.read_string().is_some() && reader.cursor() > saved;
    (read && reader.at_token_boundary()) || reset(reader, saved)
}

/// Splits `args` at each `separator` outside quotes and brackets.
pub(super) fn split_top_level(args: &str, separator: char) -> Vec<String> {
    let mut parts = Vec::new();
    let mut current = String::new();
    let mut depth = 0usize;
    let mut quote: Option<char> = None;
    let mut escape_next = false;

    for ch in args.chars() {
        if escape_next {
            current.push(ch);
            escape_next = false;
            continue;
        }
        if ch == '\\' && quote.is_some() {
            current.push(ch);
            escape_next = true;
            continue;
        }
        if let Some(active_quote) = quote {
            current.push(ch);
            if ch == active_quote {
                quote = None;
            }
            continue;
        }
        match ch {
            '"' | '\'' => {
                quote = Some(ch);
                current.push(ch);
            }
            '[' | '{' | '(' => {
                depth += 1;
                current.push(ch);
            }
            ']' | '}' | ')' => {
                depth = depth.saturating_sub(1);
                current.push(ch);
            }
            _ if ch == separator && depth == 0 => {
                parts.push(current.trim().to_string());
                current.clear();
            }
            _ => current.push(ch),
        }
    }

    if !current.trim().is_empty() {
        parts.push(current.trim().to_string());
    }
    parts
}

// ============================================================================
// Resource location parsers
// ============================================================================

/// Whether `text` reads as an id: a path, or `namespace:path`.
pub(super) fn is_id(text: &str) -> bool {
    let (namespace, path) = text.split_once(':').unwrap_or(("", text));
    let is_path_char = |ch: char| {
        ch.is_ascii_lowercase() || ch.is_ascii_digit() || matches!(ch, '_' | '.' | '-' | '/')
    };
    namespace.chars().all(|ch| ch != '/' && is_path_char(ch)) && path.chars().all(is_path_char)
}

/// Read a resource location without checking for a token boundary.
/// Returns true if at least one character was consumed.
fn read_resource_location_chars(reader: &mut StringReader) -> bool {
    let mut has_chars = false;
    let mut has_colon = false;
    let mut has_slash = false;
    while reader.can_read() {
        let ch = reader.peek().unwrap();
        if ch == '$' {
            let saved = reader.cursor();
            if reader.try_read_macro() {
                has_chars = true;
                continue;
            }
            reader.set_cursor(saved);
            break;
        }
        if ch.is_ascii_lowercase()
            || ch.is_ascii_digit()
            || ch == '_'
            || ch == '.'
            || ch == '-'
            || ch == '/'
            || (ch == ':' && !has_colon && !has_slash)
        {
            has_colon |= ch == ':';
            has_slash |= ch == '/';
            reader.read_char();
            has_chars = true;
        } else {
            break;
        }
    }
    has_chars
}

/// A registry ID or an inline SNBT value (`{type:"minecraft:uniform",...}`),
/// as accepted by Minecraft's `ResourceOrIdArgument`.
fn parse_resource_or_inline(reader: &mut StringReader) -> bool {
    let saved = reader.cursor();
    match reader.peek() {
        Some('{') | Some('[') => {
            if reader.read_nbt() && reader.at_token_boundary() {
                return true;
            }
        }
        _ => {
            if parse_resource_location(reader) {
                return true;
            }
        }
    }
    reader.set_cursor(saved);
    false
}

/// An id can't start with `+`, so Minecraft reads `+1` as the number it is. A
/// number without the sign reads as an id, such as `minecraft:1`.
fn parse_number_provider(reader: &mut StringReader) -> bool {
    if reader.peek() != Some('+') {
        return parse_resource_or_inline(reader);
    }
    let saved = reader.cursor();
    let word = reader.read_until_space();
    if snbt::parse(&word).is_ok_and(|value| value.is_number()) {
        return true;
    }
    reader.set_cursor(saved);
    false
}

/// `minecraft:slot_source` tries a slot range first, like Minecraft's
/// `SlotSourceArgument`, then falls back to a slot source ID or inline value.
/// Those two can follow extra spaces, and a slot name can't.
fn parse_slot_source(reader: &mut StringReader) -> bool {
    if parse_item_slots(reader) {
        return true;
    }
    reader.skip_whitespace();
    parse_resource_or_inline(reader)
}

fn parse_resource_location(reader: &mut StringReader) -> bool {
    let saved = reader.cursor();

    if read_resource_location_chars(reader) && reader.at_token_boundary() {
        return true;
    }

    reader.set_cursor(saved);
    false
}

fn parse_resource_or_tag(reader: &mut StringReader) -> bool {
    let saved = reader.cursor();

    // Optional # prefix for tags, which reads with an empty id too.
    if reader.peek() == Some('#') {
        reader.read_char();
        if reader.at_token_boundary() {
            return true;
        }
    }

    if parse_resource_location(reader) {
        return true;
    }

    reader.set_cursor(saved);
    false
}

/// An id that can hold the wildcards `*` and `?`. What it matches is known
/// when the function loads, and a pack can add to it.
fn parse_resource_selector(reader: &mut StringReader) -> bool {
    let saved = reader.cursor();
    while reader
        .peek()
        .is_some_and(|ch| ch.is_ascii_lowercase() || ch.is_ascii_digit() || "_:/.-*?".contains(ch))
    {
        reader.read_char();
    }
    (reader.cursor() > saved && reader.at_token_boundary()) || reset(reader, saved)
}

// ============================================================================
// Coordinate parsers
// ============================================================================

/// `count` coordinates, one space apart, or a placeholder for all of them.
/// One that ends the line isn't read here: what follows it in the final
/// command is unknown, and `walk_node` stops checking at such a word.
fn parse_coordinates(reader: &mut StringReader, count: usize) -> bool {
    let saved = reader.cursor();
    let read = reader.read_coordinate()
        && (1..count).all(|_| reader.skip_separator() && reader.read_coordinate());
    if read && reader.at_token_boundary() {
        return true;
    }
    reader.set_cursor(saved);
    (parse_whole_macro(reader) && reader.can_read()) || reset(reader, saved)
}

fn parse_whole_macro(reader: &mut StringReader) -> bool {
    let saved = reader.cursor();
    if reader.try_read_macro() && reader.at_token_boundary() {
        return true;
    }
    reader.set_cursor(saved);
    false
}

// ============================================================================
// Particle parser
// ============================================================================

fn parse_particle(reader: &mut StringReader) -> bool {
    let saved = reader.cursor();

    // Read the particle type (resource location); options may follow it
    // without a space, so no token boundary is required here.
    if !read_resource_location_chars(reader) {
        reader.set_cursor(saved);
        return false;
    }

    // Since 1.20.5 particle options are an SNBT compound glued to the id:
    // `dust{color:[1.0,0.0,0.0],scale:1.0}`.
    if (reader.peek() == Some('{') && !reader.read_nbt()) || !reader.at_token_boundary() {
        reader.set_cursor(saved);
        return false;
    }
    true
}

// ============================================================================
// UUID parser
// ============================================================================

/// The hex digits and dashes at the cursor, in Java's short form of a UUID.
fn parse_uuid(reader: &mut StringReader) -> bool {
    let saved = reader.cursor();
    while reader
        .peek()
        .is_some_and(|ch| ch.is_ascii_hexdigit() || ch == '-')
    {
        reader.read_char();
    }
    (snbt::is_uuid(&reader.slice(saved, reader.cursor())) && reader.at_token_boundary())
        || reset(reader, saved)
}

// ============================================================================
// JSON/NBT complex types
// ============================================================================

fn parse_component(reader: &mut StringReader) -> bool {
    if !reader.can_read() {
        return false;
    }
    let saved = reader.cursor();
    let ch = reader.peek().unwrap();

    // JSON object or array
    if ch == '{' || ch == '[' {
        if reader.read_nbt() {
            return true;
        }
        reader.set_cursor(saved);
        return false;
    }

    // Quoted string (text component shorthand)
    if ch == '"' || ch == '\'' {
        if reader.read_quoted_string() {
            return true;
        }
        reader.set_cursor(saved);
        return false;
    }

    // Accept unquoted too for liberal parsing (e.g., plain text components)
    let s = reader.read_unquoted_string();
    if !s.is_empty() {
        return true;
    }

    reader.set_cursor(saved);
    false
}

fn parse_nbt_compound(reader: &mut StringReader) -> bool {
    if reader.peek() != Some('{') {
        return false;
    }
    let saved = reader.cursor();
    if reader.read_nbt() && reader.at_token_boundary() {
        return true;
    }
    reader.set_cursor(saved);
    false
}

fn parse_nbt_tag(reader: &mut StringReader) -> bool {
    if !reader.can_read() {
        return false;
    }
    let saved = reader.cursor();
    let ch = reader.peek().unwrap();

    // Compound or list
    if ch == '{' || ch == '[' {
        if reader.read_nbt() && reader.at_token_boundary() {
            return true;
        }
        reader.set_cursor(saved);
        return false;
    }

    // Quoted string
    if ch == '"' || ch == '\'' {
        if reader.read_quoted_string() && reader.at_token_boundary() {
            return true;
        }
        reader.set_cursor(saved);
        return false;
    }

    // A number, a word, or an operation such as `bool(1)`, which may hold
    // spaces before and inside its parentheses. `snbt` reads what this finds.
    let mut depth = 0usize;
    while reader.can_read() {
        if depth == 0
            && reader.at_token_boundary()
            && !reader.remaining().trim_start().starts_with('(')
        {
            break;
        }
        match reader.read_char() {
            Some('(') => depth += 1,
            Some(')') => depth = depth.saturating_sub(1),
            _ => {}
        }
    }
    reader.cursor() > saved
}

fn parse_nbt_path(reader: &mut StringReader) -> bool {
    if !reader.can_read() {
        return false;
    }
    let saved = reader.cursor();

    // NBT path: sequence of identifiers, dots, brackets, and quoted strings
    // e.g., foo.bar[0].baz, "quoted key".value, {filter}.field
    let mut has_content = false;

    while reader.can_read() {
        let ch = reader.peek().unwrap();

        if ch == ' ' {
            break;
        }

        // Without a placeholder, `$` is a character of a key.
        if ch == '$' && reader.try_read_macro() {
            has_content = true;
            continue;
        }

        if ch == '"' || ch == '\'' {
            if reader.read_string().is_none() {
                return reset(reader, saved);
            }
            has_content = true;
            continue;
        }

        if ch == '{' || ch == '[' {
            if !reader.read_nbt() {
                return reset(reader, saved);
            }
            has_content = true;
            continue;
        }

        if ch == ']' || ch == '}' {
            break;
        }
        reader.read_char();
        has_content = true;
    }

    // An empty path reads, and `nbt_path_problem` checks the rest.
    if (has_content || reader.peek() == Some(' ')) && reader.at_token_boundary() {
        return true;
    }

    reader.set_cursor(saved);
    false
}

// ============================================================================
// Block/item state parsers
// ============================================================================

fn parse_block_state(reader: &mut StringReader) -> bool {
    let saved = reader.cursor();

    // Optional # for predicates (tags)
    if reader.peek() == Some('#') {
        reader.read_char();
    }

    // Resource location (don't require token boundary yet — [...] or {...} may follow)
    if !read_resource_location_chars(reader) {
        reader.set_cursor(saved);
        return false;
    }

    // Optional block states [key=value,...]
    if reader.can_read() && reader.peek() == Some('[') && !reader.read_nbt() {
        reader.set_cursor(saved);
        return false;
    }

    // Optional NBT data {...}
    if reader.can_read() && reader.peek() == Some('{') && !reader.read_nbt() {
        reader.set_cursor(saved);
        return false;
    }

    if reader.at_token_boundary() {
        return true;
    }

    reader.set_cursor(saved);
    false
}

/// An optional `#` and an id, up to its `[...]`.
fn read_item_id(reader: &mut StringReader) -> bool {
    if reader.peek() == Some('#') {
        reader.read_char();
    }
    read_resource_location_chars(reader)
}

fn parse_item_stack(reader: &mut StringReader) -> bool {
    let saved = reader.cursor();

    // Optional components [...] (1.20.5+ format)
    if read_item_id(reader)
        && (reader.peek() != Some('[') || reader.read_nbt())
        && reader.at_token_boundary()
    {
        return true;
    }

    reader.set_cursor(saved);
    false
}

/// Item predicates (`ItemPredicateArgument`) also accept the `*` wildcard
/// before their optional `[...]` tests (`*[minecraft:custom_data~{id:"x"}]`),
/// and spaces can stand before the tests.
fn parse_item_predicate(reader: &mut StringReader) -> bool {
    let saved = reader.cursor();
    let read_item = if reader.peek() == Some('*') {
        reader.read_char().is_some()
    } else {
        read_item_id(reader)
    };
    if read_item {
        let after_item = reader.cursor();
        reader.skip_whitespace();
        if reader.peek() != Some('[') {
            reader.set_cursor(after_item);
            return reader.at_token_boundary() || reset(reader, saved);
        }
        if reader.read_nbt() && reader.at_token_boundary() {
            return true;
        }
    }
    reset(reader, saved)
}

fn reset(reader: &mut StringReader, position: usize) -> bool {
    reader.set_cursor(position);
    false
}

// ============================================================================
// Message (greedy) parser
// ============================================================================

/// Minecraft rejects `message` arguments (`say`, `msg`, `me`, `teammsg`)
/// longer than this many UTF-16 code units.
const MAX_MESSAGE_LENGTH: usize = 256;

fn parse_message(reader: &mut StringReader) -> bool {
    if !reader.can_read() {
        return false;
    }
    // In a macro line the values replace `$(name)`, so only the fixed text
    // is known to count against the limit.
    let message = reader.read_greedy();
    without_macro_placeholders(&message).encode_utf16().count() <= MAX_MESSAGE_LENGTH
}

fn without_macro_placeholders(text: &str) -> String {
    let mut result = String::new();
    let mut rest = text;
    while let Some(start) = rest.find("$(") {
        let Some(end) = rest[start..].find(')') else {
            break;
        };
        result.push_str(&rest[..start]);
        rest = &rest[start + end + 1..];
    }
    result.push_str(rest);
    result
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_bool() {
        let mut r = StringReader::new("true rest");
        assert!(parse_argument(&mut r, "brigadier:bool", None));
        assert_eq!(r.remaining(), " rest");
    }

    #[test]
    fn test_parse_bool_reads_a_string() {
        for value in ["true rest", "false rest", "\"true\" rest", "'false' rest"] {
            let mut r = StringReader::new(value);
            assert!(parse_argument(&mut r, "brigadier:bool", None), "{value}");
            assert_eq!(r.remaining(), " rest", "{value}");
        }
        for value in [
            "\"True\" rest",
            "True rest",
            "TRUE rest",
            "1 rest",
            "\"\" rest",
            "\"true rest",
            "true\" rest",
            "\"true\"x rest",
            "\"tr\\ue\" rest",
        ] {
            let mut r = StringReader::new(value);
            assert!(!parse_argument(&mut r, "brigadier:bool", None), "{value}");
            assert_eq!(r.cursor(), 0, "{value}");
        }
    }

    #[test]
    fn test_parse_integer() {
        let mut r = StringReader::new("42 rest");
        assert!(parse_argument(&mut r, "brigadier:integer", None));
        assert_eq!(r.remaining(), " rest");
    }

    #[test]
    fn test_parse_integer_bounds() {
        let props = serde_json::json!({"min": 1, "max": 64});
        let mut valid = StringReader::new("64 rest");
        assert!(parse_argument(
            &mut valid,
            "brigadier:integer",
            Some(&props)
        ));

        let mut too_low = StringReader::new("0 rest");
        assert!(!parse_argument(
            &mut too_low,
            "brigadier:integer",
            Some(&props)
        ));
        assert_eq!(too_low.cursor(), 0);

        let mut too_high = StringReader::new("65 rest");
        assert!(!parse_argument(
            &mut too_high,
            "brigadier:integer",
            Some(&props)
        ));
        assert_eq!(too_high.cursor(), 0);
    }

    #[test]
    fn test_parse_float() {
        let mut r = StringReader::new("3.14 rest");
        assert!(parse_argument(&mut r, "brigadier:float", None));
        assert_eq!(r.remaining(), " rest");
    }

    #[test]
    fn test_parse_float_bounds() {
        let props = serde_json::json!({"min": 0.0});
        let mut valid = StringReader::new("0.5 rest");
        assert!(parse_argument(&mut valid, "brigadier:float", Some(&props)));

        let mut invalid = StringReader::new("-0.5 rest");
        assert!(!parse_argument(
            &mut invalid,
            "brigadier:float",
            Some(&props)
        ));
        assert_eq!(invalid.cursor(), 0);
    }

    #[test]
    fn test_parse_float_reads_32_bits() {
        let parse = |text: &str, properties: Option<&serde_json::Value>| {
            parse_argument(&mut StringReader::new(text), "brigadier:float", properties)
        };
        let range = serde_json::json!({"min": 1.0, "max": 2.0});
        let small = serde_json::json!({"min": 1e-05});
        for text in ["1.00000001", "2.00000001", "2.0000001", "0.99999999"] {
            assert!(parse(text, Some(&range)), "{text}");
        }
        for text in ["2.1", "0.9"] {
            assert!(!parse(text, Some(&range)), "{text}");
        }
        assert!(parse("0.00001", Some(&small)));
        assert!(!parse("0.000009999999", Some(&small)));

        // Too large a float is infinite, and that is past any bound.
        let positive = serde_json::json!({"min": 0.0});
        for properties in [None, Some(&positive)] {
            assert!(parse("340282356779733661637539395458142568447", properties));
            for text in [
                "340282356779733661637539395458142568448",
                "340282366920938463463374607431768211456",
            ] {
                assert!(!parse(text, properties), "{text}");
            }
        }
        assert!(!parse("-340282356779733661637539395458142568448", None));
    }

    #[test]
    fn test_parse_double_can_not_be_infinite() {
        let parse =
            |text: &str| parse_argument(&mut StringReader::new(text), "brigadier:double", None);
        assert!(parse("340282366920938463463374607431768211456"));
        assert!(!parse(&format!("1{}", "0".repeat(400))));
        assert!(!parse(&format!("-1{}", "0".repeat(400))));
    }

    #[test]
    fn test_parse_greedy_string() {
        let props = serde_json::json!({"type": "greedy"});
        let mut r = StringReader::new("hello world rest");
        assert!(parse_argument(&mut r, "brigadier:string", Some(&props)));
        assert!(r.remaining().is_empty());
    }

    #[test]
    fn test_parse_word_string() {
        let props = serde_json::json!({"type": "word"});
        let mut r = StringReader::new("hello rest");
        assert!(parse_argument(&mut r, "brigadier:string", Some(&props)));
        assert_eq!(r.remaining(), " rest");

        let mut invalid = StringReader::new("hello{\"text\":\"bad\"}");
        assert!(!parse_argument(
            &mut invalid,
            "brigadier:string",
            Some(&props)
        ));
        assert_eq!(invalid.cursor(), 0);
    }

    #[test]
    fn test_parse_words_stop_at_characters_of_ids() {
        let word = serde_json::json!({"type": "word"});
        let phrase = serde_json::json!({"type": "phrase"});
        let (word, phrase) = (Some(&word), Some(&phrase));
        let parsers = [
            ("brigadier:string", word),
            ("brigadier:string", phrase),
            ("minecraft:objective", None),
            ("minecraft:team", None),
            ("minecraft:entity", None),
        ];
        for (parser, properties) in parsers {
            let mut valid = StringReader::new("a.b+c-d_e rest");
            assert!(parse_argument(&mut valid, parser, properties), "{parser}");
            assert_eq!(valid.remaining(), " rest", "{parser}");

            for text in ["a:b rest", "a/b rest", "a#b rest", "a*b rest"] {
                let mut invalid = StringReader::new(text);
                assert!(
                    !parse_argument(&mut invalid, parser, properties),
                    "{parser} {text}"
                );
                assert_eq!(invalid.cursor(), 0, "{parser} {text}");
            }
        }

        // A quoted word can hold them.
        let mut quoted = StringReader::new("\"file/mypack\" rest");
        assert!(parse_argument(&mut quoted, "brigadier:string", phrase));
        assert_eq!(quoted.remaining(), " rest");
    }

    #[test]
    fn test_parse_criteria_and_slots_keep_their_own_characters() {
        for (parser, value) in [
            (
                "minecraft:objective_criteria",
                "minecraft.killed:minecraft.zombie rest",
            ),
            ("minecraft:objective_criteria", "a:b rest"),
            ("minecraft:item_slots", "container.* rest"),
            ("minecraft:slot_source", "weapon.* rest"),
            ("minecraft:slot_source", "ns:hands rest"),
        ] {
            let mut valid = StringReader::new(value);
            assert!(parse_argument(&mut valid, parser, None), "{parser} {value}");
            assert_eq!(valid.remaining(), " rest", "{parser} {value}");
        }
    }

    #[test]
    fn test_parse_hex_color() {
        for color in [
            "ff0000 rest",
            "fff rest",
            "FF00aa rest",
            "+f0000 rest",
            "ff-000 rest",
        ] {
            let mut valid = StringReader::new(color);
            assert!(
                parse_argument(&mut valid, "minecraft:hex_color", None),
                "{color}"
            );
            assert_eq!(valid.remaining(), " rest", "{color}");
        }
        for color in [
            "#ff0000 rest",
            "#fff rest",
            "ff00 rest",
            "ff rest",
            "fffffff rest",
            "ggg rest",
            "00000g rest",
            "00000+ rest",
            "+ff rest",
            "\"fff\" rest",
            " rest",
        ] {
            let mut invalid = StringReader::new(color);
            assert!(
                !parse_argument(&mut invalid, "minecraft:hex_color", None),
                "{color}"
            );
            assert_eq!(invalid.cursor(), 0, "{color}");
        }
    }

    #[test]
    fn test_parse_numbers_have_no_exponent_and_no_plus() {
        for (parser, value) in [
            ("brigadier:integer", "+1"),
            ("brigadier:integer", "1e3"),
            ("brigadier:float", "1e3"),
            ("brigadier:float", "+1"),
            ("brigadier:double", "1e-3"),
            ("brigadier:double", "1E3"),
            ("brigadier:double", "+1"),
            ("minecraft:int_range", "+1"),
            ("minecraft:int_range", "1..+2"),
            ("minecraft:float_range", "1e3"),
            ("minecraft:float_range", "1..+2"),
            ("minecraft:time", "1e3"),
            ("minecraft:time", "1e0t"),
            ("minecraft:time", "+1"),
            ("minecraft:vec3", "1e-3 0 0"),
            ("minecraft:vec3", "~1e1 ~ ~"),
            ("minecraft:vec3", "~+1 ~ ~"),
            ("minecraft:block_pos", "+1 2 3"),
            ("minecraft:vec2", "1e1 2"),
        ] {
            let mut invalid = StringReader::new(value);
            assert!(
                !parse_argument(&mut invalid, parser, None),
                "{parser} {value}"
            );
            assert_eq!(invalid.cursor(), 0, "{parser} {value}");
        }
        for (parser, value) in [
            ("brigadier:integer", "-1"),
            ("brigadier:double", "0.001"),
            ("brigadier:double", ".5"),
            ("brigadier:float", "5."),
            ("minecraft:int_range", "-5..-1"),
            ("minecraft:float_range", ".5...7"),
            ("minecraft:time", "1.5t"),
            ("minecraft:vec3", "~1 ^.5 -3"),
        ] {
            let mut valid = StringReader::new(value);
            assert!(parse_argument(&mut valid, parser, None), "{parser} {value}");
            assert!(!valid.can_read(), "{parser} {value}");
        }
    }

    #[test]
    fn test_is_id() {
        for id in ["a", "a:b", "a.b-c_d:e/f", "a/b", ":a", "a:", ""] {
            assert!(is_id(id), "{id}");
        }
        for id in ["A", "a:B", "a/b:c", "a:b:c", "{a", "a b"] {
            assert!(!is_id(id), "{id}");
        }
    }

    #[test]
    fn test_parse_quoted_strings_take_two_escapes() {
        let phrase = serde_json::json!({"type": "phrase"});
        for (parser, properties) in [
            ("brigadier:string", Some(&phrase)),
            ("minecraft:entity", None),
            ("minecraft:nbt_path", None),
        ] {
            for text in [
                "'a\\'b' rest",
                "\"a\\\"b\" rest",
                "\"a\\\\b\" rest",
                "\"a b\" rest",
            ] {
                let mut valid = StringReader::new(text);
                assert!(parse_argument(&mut valid, parser, properties), "{text}");
                assert_eq!(valid.remaining(), " rest", "{parser} {text}");
            }
            for text in ["\"a\\nb\"", "\"a\\'b\"", "'a\\\"b'", "\"a\\b\""] {
                let mut invalid = StringReader::new(text);
                assert!(!parse_argument(&mut invalid, parser, properties), "{text}");
                assert_eq!(invalid.cursor(), 0, "{parser} {text}");
            }
        }
        // SNBT and text components have more escapes of their own.
        for parser in ["minecraft:nbt_tag", "minecraft:component"] {
            let mut r = StringReader::new("\"a\\nb\" rest");
            assert!(parse_argument(&mut r, parser, None), "{parser}");
            assert_eq!(r.remaining(), " rest", "{parser}");
        }
    }

    #[test]
    fn test_parse_phrase_string_requires_boundary() {
        let props = serde_json::json!({"type": "phrase"});
        let mut invalid = StringReader::new("test{\"text\":\"bad\"}");
        assert!(!parse_argument(
            &mut invalid,
            "brigadier:string",
            Some(&props)
        ));
        assert_eq!(invalid.cursor(), 0);
    }

    #[test]
    fn test_parse_operation() {
        for op in &["+=", "-=", "*=", "/=", "%=", "><", "=", "<", ">"] {
            let input = format!("{} rest", op);
            let mut r = StringReader::new(&input);
            assert!(
                parse_argument(&mut r, "minecraft:operation", None),
                "Failed for op: {}",
                op
            );
        }

        for op in &["<=", ">="] {
            let input = format!("{} rest", op);
            let mut r = StringReader::new(&input);
            assert!(
                !parse_argument(&mut r, "minecraft:operation", None),
                "Invalid op should be rejected: {}",
                op
            );
            assert_eq!(r.cursor(), 0);
        }
    }

    #[test]
    fn test_parse_scoreboard_slot() {
        for slot in ["sidebar rest", "below_name rest", "sidebar.team.red rest"] {
            let mut valid = StringReader::new(slot);
            assert!(parse_argument(
                &mut valid,
                "minecraft:scoreboard_slot",
                None
            ));
            assert_eq!(valid.remaining(), " rest");
        }

        let mut invalid = StringReader::new("sideways rest");
        assert!(!parse_argument(
            &mut invalid,
            "minecraft:scoreboard_slot",
            None
        ));
        assert_eq!(invalid.cursor(), 0);
    }

    #[test]
    fn test_parse_swizzle() {
        for axes in ["x rest", "zy rest", "xyz rest"] {
            let mut valid = StringReader::new(axes);
            assert!(parse_argument(&mut valid, "minecraft:swizzle", None));
            assert_eq!(valid.remaining(), " rest");
        }

        for axes in ["xx rest", "xyq rest", "xyzz rest"] {
            let mut invalid = StringReader::new(axes);
            assert!(!parse_argument(&mut invalid, "minecraft:swizzle", None));
            assert_eq!(invalid.cursor(), 0);
        }
    }

    #[test]
    fn test_parse_item_slots() {
        for slot in [
            "armor.head rest",
            "container.53 rest",
            "weapon.offhand rest",
            "inventory.26 rest",
        ] {
            let mut valid = StringReader::new(slot);
            assert!(parse_argument(&mut valid, "minecraft:item_slot", None));
            assert_eq!(valid.remaining(), " rest");
        }

        for slot in ["armor.tail rest", "container.54 rest", "garbage rest"] {
            let mut invalid = StringReader::new(slot);
            assert!(!parse_argument(&mut invalid, "minecraft:item_slot", None));
            assert_eq!(invalid.cursor(), 0);
        }

        let mut group = StringReader::new("armor.* rest");
        assert!(parse_argument(&mut group, "minecraft:item_slots", None));
        assert_eq!(group.remaining(), " rest");
    }

    #[test]
    fn test_slot_names_have_one_spelling() {
        for slot in ["hotbar.0", "hotbar.8", "container.0", "container.10"] {
            assert!(is_item_slot_value(slot, false), "{slot}");
        }
        for slot in [
            "hotbar.08",
            "container.00",
            "container.01",
            "container.+1",
            "hotbar.-0",
        ] {
            assert!(!is_item_slot_value(slot, false), "{slot}");
        }
    }

    #[test]
    fn test_parse_resource_selector_takes_wildcards() {
        let parser = "minecraft:resource_selector";
        for selector in [
            "* rest",
            "?",
            "a:* rest",
            "*:* rest",
            "minecraft:always_* rest",
            "a?b",
        ] {
            let mut valid = StringReader::new(selector);
            assert!(parse_argument(&mut valid, parser, None), "{selector}");
            assert!(valid.at_token_boundary(), "{selector}");
        }
        for selector in ["#a rest", "\"a\" rest", "A rest", "a*B rest", " rest"] {
            let mut invalid = StringReader::new(selector);
            assert!(!parse_argument(&mut invalid, parser, None), "{selector}");
            assert_eq!(invalid.cursor(), 0, "{selector}");
        }
    }

    #[test]
    fn test_parse_tags_can_have_an_empty_id() {
        for parser in [
            "minecraft:function",
            "minecraft:resource_or_tag",
            "minecraft:resource_or_tag_key",
        ] {
            for tag in ["# rest", "#", "#a rest", "#:a rest", "#a: rest"] {
                let mut valid = StringReader::new(tag);
                assert!(parse_argument(&mut valid, parser, None), "{parser} {tag}");
                assert!(valid.at_token_boundary(), "{parser} {tag}");
            }
            for tag in ["#A rest", "##a rest"] {
                let mut invalid = StringReader::new(tag);
                assert!(
                    !parse_argument(&mut invalid, parser, None),
                    "{parser} {tag}"
                );
                assert_eq!(invalid.cursor(), 0, "{parser} {tag}");
            }
        }
    }

    #[test]
    fn test_parse_entity_selector() {
        let mut r = StringReader::new("@a[tag=foo] rest");
        assert!(parse_argument(&mut r, "minecraft:entity", None));
        assert_eq!(r.remaining(), " rest");
    }

    #[test]
    fn test_parse_game_profile_takes_everything_up_to_a_space() {
        for name in [
            "Steve rest",
            "a:b rest",
            "a/b rest",
            "\"a\" rest",
            "@a rest",
        ] {
            let mut valid = StringReader::new(name);
            assert!(
                parse_argument(&mut valid, "minecraft:game_profile", None),
                "{name}"
            );
            assert_eq!(valid.remaining(), " rest", "{name}");
        }
        for name in [" rest", "@ rest"] {
            let mut invalid = StringReader::new(name);
            assert!(
                !parse_argument(&mut invalid, "minecraft:game_profile", None),
                "{name}"
            );
        }
    }

    #[test]
    fn test_parse_resource_location() {
        let mut r = StringReader::new("minecraft:stone rest");
        assert!(parse_argument(&mut r, "minecraft:resource_location", None));
        assert_eq!(r.remaining(), " rest");
    }

    #[test]
    fn test_parse_block_pos() {
        let mut r = StringReader::new("~ ~1 ~ rest");
        assert!(parse_argument(&mut r, "minecraft:block_pos", None));
        assert_eq!(r.remaining(), " rest");

        let mut invalid = StringReader::new("~~~ rest");
        assert!(!parse_argument(&mut invalid, "minecraft:block_pos", None));
        assert_eq!(invalid.cursor(), 0);
    }

    #[test]
    fn test_parse_vec3() {
        let mut r = StringReader::new("1.0 2.5 3.0 rest");
        assert!(parse_argument(&mut r, "minecraft:vec3", None));
        assert_eq!(r.remaining(), " rest");

        let mut invalid = StringReader::new("1.02.53.0 rest");
        assert!(!parse_argument(&mut invalid, "minecraft:vec3", None));
        assert_eq!(invalid.cursor(), 0);
    }

    #[test]
    fn test_parse_int_range() {
        let mut r = StringReader::new("1..5 rest");
        assert!(parse_argument(&mut r, "minecraft:int_range", None));
        assert_eq!(r.remaining(), " rest");
    }

    #[test]
    fn test_parse_nbt_compound() {
        let mut r = StringReader::new("{key:\"value\"} rest");
        assert!(parse_argument(&mut r, "minecraft:nbt_compound_tag", None));
        assert_eq!(r.remaining(), " rest");
    }

    #[test]
    fn test_parse_block_state() {
        let mut r = StringReader::new("minecraft:oak_stairs[facing=north]{Items:[]} rest");
        assert!(parse_argument(&mut r, "minecraft:block_state", None));
        assert_eq!(r.remaining(), " rest");
    }

    #[test]
    fn test_parse_score_holder_takes_everything_up_to_a_space() {
        for holder in [
            "$x rest",
            "#a>b rest",
            "a:b rest",
            "* rest",
            "a@b rest",
            "a[b] rest",
            "\"a rest",
            "{x} rest",
            "é rest",
            "@e[limit=1] rest",
        ] {
            let mut valid = StringReader::new(holder);
            assert!(
                parse_argument(&mut valid, "minecraft:score_holder", None),
                "{holder}"
            );
            assert_eq!(valid.remaining(), " rest", "{holder}");
        }

        let mut empty = StringReader::new(" rest");
        assert!(parse_argument(&mut empty, "minecraft:score_holder", None));
        assert_eq!(empty.cursor(), 0);

        // Only a word that starts with `@` is a selector, and it has to be one.
        for holder in ["@ rest", "@bad rest", "@e[ rest"] {
            let mut invalid = StringReader::new(holder);
            assert!(
                !parse_argument(&mut invalid, "minecraft:score_holder", None),
                "{holder}"
            );
            assert_eq!(invalid.cursor(), 0, "{holder}");
        }
    }

    #[test]
    fn test_parse_ids_take_one_colon_and_none_after_a_slash() {
        for id in ["a:b rest", "a:b/c rest", "b/c rest", "a.b-c_d:e rest"] {
            let mut valid = StringReader::new(id);
            assert!(
                parse_argument(&mut valid, "minecraft:resource_location", None),
                "{id}"
            );
            assert_eq!(valid.remaining(), " rest", "{id}");
        }

        for (parser, id) in [
            ("minecraft:resource_location", "a:b:c rest"),
            ("minecraft:resource_location", "a/b:c rest"),
            ("minecraft:function", "a:b:c rest"),
            ("minecraft:function", "#a/b:c rest"),
            ("minecraft:dimension", "a/b:c rest"),
            ("minecraft:item_stack", "minecraft:stone:x rest"),
            ("minecraft:block_state", "minecraft:stone:x rest"),
            ("minecraft:particle", "a:b:c rest"),
            ("minecraft:loot_table", "a:b:c rest"),
        ] {
            let mut invalid = StringReader::new(id);
            assert!(!parse_argument(&mut invalid, parser, None), "{parser} {id}");
            assert_eq!(invalid.cursor(), 0, "{parser} {id}");
        }

        // A placeholder can fill in either part.
        for id in ["$(ns):$(name) rest", "a:$(name) rest", "$(ns):b rest"] {
            let mut valid = StringReader::for_macro_line(id);
            assert!(
                parse_argument(&mut valid, "minecraft:function", None),
                "{id}"
            );
            assert_eq!(valid.remaining(), " rest", "{id}");
        }
    }

    #[test]
    fn test_parse_argument_at_a_second_space() {
        let word = serde_json::json!({"type": "word"});
        let phrase = serde_json::json!({"type": "phrase"});
        let (word, phrase) = (Some(&word), Some(&phrase));

        for (parser, properties) in [
            ("brigadier:string", word),
            ("brigadier:string", phrase),
            ("minecraft:objective", None),
            ("minecraft:team", None),
            ("minecraft:score_holder", None),
            ("minecraft:nbt_path", None),
            ("minecraft:resource_location", None),
            ("minecraft:function", None),
            ("minecraft:dimension", None),
            ("minecraft:resource_key", None),
        ] {
            let mut empty = StringReader::new(" rest");
            assert!(parse_argument(&mut empty, parser, properties), "{parser}");
            assert_eq!(empty.cursor(), 0, "{parser}");
        }

        for parser in [
            "minecraft:nbt_tag",
            "minecraft:component",
            "minecraft:style",
            "minecraft:loot_table",
            "minecraft:dialog",
            "minecraft:feature",
            "minecraft:item_predicate",
        ] {
            let mut r = StringReader::new("  a rest");
            assert!(parse_argument(&mut r, parser, None), "{parser}");
            assert_eq!(r.remaining(), " rest", "{parser}");
        }
        let mut r = StringReader::new("  {} rest");
        assert!(parse_argument(&mut r, "minecraft:nbt_compound_tag", None));
        assert_eq!(r.remaining(), " rest");

        for parser in [
            "brigadier:integer",
            "brigadier:float",
            "brigadier:double",
            "brigadier:bool",
            "minecraft:time",
            "minecraft:entity",
            "minecraft:game_profile",
            "minecraft:objective_criteria",
            "minecraft:resource",
            "minecraft:resource_or_tag",
            "minecraft:block_state",
            "minecraft:item_stack",
            "minecraft:item_slot",
            "minecraft:gamemode",
        ] {
            let mut r = StringReader::new(" 1 rest");
            assert!(!parse_argument(&mut r, parser, None), "{parser}");
            assert_eq!(r.cursor(), 0, "{parser}");
        }
    }

    #[test]
    fn test_parse_template_rotation() {
        for rotation in [
            "none rest",
            "clockwise_90 rest",
            "180 rest",
            "counterclockwise_90 rest",
        ] {
            let mut valid = StringReader::new(rotation);
            assert!(
                parse_argument(&mut valid, "minecraft:template_rotation", None),
                "{rotation}"
            );
            assert_eq!(valid.remaining(), " rest", "{rotation}");
        }

        for rotation in ["clockwise_180 rest", "90 rest", "None rest"] {
            let mut invalid = StringReader::new(rotation);
            assert!(
                !parse_argument(&mut invalid, "minecraft:template_rotation", None),
                "{rotation}"
            );
            assert_eq!(invalid.cursor(), 0, "{rotation}");
        }
    }

    #[test]
    fn test_parse_nbt_tag_finds_the_end_of_a_bare_value() {
        for value in [
            "abc rest",
            "true rest",
            "a.b rest",
            "1ub rest",
            "1_000 rest",
            "0x10 rest",
            "1. rest",
            "bool(1) rest",
            "bool( 1 ) rest",
            "bool (1) rest",
            "uuid(\"0-0-0-0-0\") rest",
            "\"a b\" rest",
            "{a:1} rest",
            "[1, 2] rest",
        ] {
            let mut r = StringReader::new(value);
            assert!(parse_argument(&mut r, "minecraft:nbt_tag", None), "{value}");
            assert_eq!(r.remaining(), " rest", "{value}");
        }

        // `snbt` refuses these from what was read here.
        for (value, read) in [("1abc rest", "1abc"), ("a:b rest", "a:b"), ("- rest", "-")] {
            let mut r = StringReader::new(value);
            assert!(parse_argument(&mut r, "minecraft:nbt_tag", None), "{value}");
            assert_eq!(r.slice(0, r.cursor()), read, "{value}");
        }

        for value in ["", "{a:1 rest", "[1 rest", "\"a rest"] {
            let mut invalid = StringReader::new(value);
            assert!(
                !parse_argument(&mut invalid, "minecraft:nbt_tag", None),
                "{value}"
            );
            assert_eq!(invalid.cursor(), 0, "{value}");
        }
    }

    #[test]
    fn test_parse_integer_stops_at_32_bits() {
        for number in ["2147483647 rest", "-2147483648 rest"] {
            let mut valid = StringReader::new(number);
            assert!(
                parse_argument(&mut valid, "brigadier:integer", None),
                "{number}"
            );
            assert_eq!(valid.remaining(), " rest", "{number}");
        }

        let wide = serde_json::json!({"min": -9_000_000_000_i64, "max": 9_000_000_000_i64});
        for number in [
            "2147483648 rest",
            "-2147483649 rest",
            "99999999999999999999 rest",
        ] {
            for properties in [None, Some(&wide)] {
                let mut invalid = StringReader::new(number);
                assert!(
                    !parse_argument(&mut invalid, "brigadier:integer", properties),
                    "{number}"
                );
                assert_eq!(invalid.cursor(), 0, "{number}");
            }
        }
    }

    #[test]
    fn test_parse_item_stack_rejects_legacy_nbt() {
        let mut invalid = StringReader::new("minecraft:diamond{Enchantments:[]} rest");
        assert!(!parse_argument(&mut invalid, "minecraft:item_stack", None));
        assert_eq!(invalid.cursor(), 0);
    }

    #[test]
    fn test_parse_message() {
        let mut r = StringReader::new("hello world @a");
        assert!(parse_argument(&mut r, "minecraft:message", None));
        assert!(r.remaining().is_empty());
    }

    #[test]
    fn test_parse_message_rejects_text_over_256_characters() {
        let fits = "a".repeat(256);
        assert!(parse_argument(
            &mut StringReader::new(&fits),
            "minecraft:message",
            None
        ));
        let too_long = "a".repeat(257);
        assert!(!parse_argument(
            &mut StringReader::new(&too_long),
            "minecraft:message",
            None
        ));
        // Macro values are unknown, so only the fixed text counts.
        let macro_line = format!("{} $(name)", "a".repeat(250));
        assert!(parse_argument(
            &mut StringReader::new(&macro_line),
            "minecraft:message",
            None
        ));
    }

    #[test]
    fn test_parse_time() {
        let mut r = StringReader::new("5t rest");
        assert!(parse_argument(&mut r, "minecraft:time", None));
        assert_eq!(r.remaining(), " rest");

        let mut r = StringReader::new("10 rest");
        assert!(parse_argument(&mut r, "minecraft:time", None));
        assert_eq!(r.remaining(), " rest");
    }

    #[test]
    fn test_parse_time_rounds_its_ticks() {
        let at_least = |min: i32, time: &str| {
            let properties = serde_json::json!({ "min": min });
            parse_argument(
                &mut StringReader::new(time),
                "minecraft:time",
                Some(&properties),
            )
        };
        for time in [
            "1",
            "0.5",
            "0.5t",
            "0.025s",
            "0.05s",
            "0.00003d",
            "1000000000000",
        ] {
            assert!(at_least(1, time), "{time}");
        }
        for time in ["0", "0.4t", "0.49999997", "0.024s", "0.00002d"] {
            assert!(!at_least(1, time), "{time}");
        }
        for time in ["0", "0t", "0d", "-0.5", "-0.00002d"] {
            assert!(at_least(0, time), "{time}");
        }
        assert!(!at_least(0, "-0.51"));
        // Past the end of an int, the ticks stay at the end.
        for time in [
            "-2147483648",
            "-2147483649",
            "2147483648",
            "99999999999999999",
        ] {
            assert!(at_least(i32::MIN, time), "{time}");
        }
    }

    #[test]
    fn test_parse_time_takes_three_units() {
        for time in ["1x", "1tt", "1t.", "1T", "1D", "1-", "t", ".", ".t", "1\""] {
            let mut r = StringReader::new(time);
            assert!(!parse_argument(&mut r, "minecraft:time", None), "{time}");
            assert_eq!(r.cursor(), 0, "{time}");
        }
    }

    #[test]
    fn test_java_round() {
        for (value, rounded) in [
            (0.5, 1),
            (0.49999997, 0),
            (-0.5, 0),
            (-0.51, -1),
            (1.5, 2),
            (2.5, 3),
            (f32::INFINITY, i32::MAX),
            (f32::NEG_INFINITY, i32::MIN),
            (1e12, i32::MAX),
        ] {
            assert_eq!(java_round(value), rounded, "{value}");
        }
    }

    #[test]
    fn test_parse_uuid_takes_java_forms() {
        for uuid in [
            "123e4567-e89b-12d3-a456-426614174000 rest",
            "0-0-0-0-0 rest",
            "A-B-C-D-E rest",
            "7fffffffffffffff-0-0-0-0 rest",
        ] {
            let mut valid = StringReader::new(uuid);
            assert!(parse_argument(&mut valid, "minecraft:uuid", None), "{uuid}");
            assert_eq!(valid.remaining(), " rest", "{uuid}");
        }
        for uuid in [
            "not-a-uuid rest",
            "00000000000000000000000000000000 rest",
            "8000000000000000-0-0-0-0 rest",
            "0-0-0-0 rest",
            "0-0-0-0-0-0 rest",
            "-0-0-0-0 rest",
            "+0-0-0-0-0 rest",
            "0-+0-0-0-0 rest",
            "g-0-0-0-0 rest",
            "\"0-0-0-0-0\" rest",
            "0-0-0-0-0x rest",
            "0-0-0-0-000000000000000000000000000000 rest",
        ] {
            let mut invalid = StringReader::new(uuid);
            assert!(
                !parse_argument(&mut invalid, "minecraft:uuid", None),
                "{uuid}"
            );
            assert_eq!(invalid.cursor(), 0, "{uuid}");
        }
    }

    #[test]
    fn test_parse_nbt_path_takes_a_dollar() {
        for path in ["$x rest", "a$b rest", "$ rest", "a.$ rest"] {
            let mut r = StringReader::new(path);
            assert!(parse_argument(&mut r, "minecraft:nbt_path", None), "{path}");
            assert_eq!(r.remaining(), " rest", "{path}");
        }
    }
}
