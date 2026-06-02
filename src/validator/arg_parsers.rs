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
    if reader.peek() == Some('$') && !is_multi_token_parser(parser_type) {
        let saved = reader.cursor();
        if reader.try_read_macro() {
            if reader.at_token_boundary() {
                return true;
            }
            reader.set_cursor(saved);
        }
    }

    match parser_type {
        // === Brigadier core types ===
        "brigadier:bool" => parse_bool(reader),
        "brigadier:integer" => parse_integer(reader, properties),
        "brigadier:float" => parse_brigadier_float(reader, properties),
        "brigadier:double" => parse_brigadier_double(reader, properties),
        "brigadier:string" => parse_brigadier_string(reader, properties),

        // === Simple word-accepting types ===
        "minecraft:objective"
        | "minecraft:team"
        | "minecraft:scoreboard_slot"
        | "minecraft:swizzle"
        | "minecraft:item_slot"
        | "minecraft:item_slots"
        | "minecraft:dialog"
        | "minecraft:objective_criteria" => parse_word(reader),
        "minecraft:hex_color" => parse_hex_color(reader),
        "minecraft:color" => parse_literal_word(
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
                "reset",
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
            &[
                "none",
                "clockwise_90",
                "clockwise_180",
                "counterclockwise_90",
            ],
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
            parse_entity(reader, parser_type, properties)
        }

        // === Resource locations ===
        "minecraft:resource_location"
        | "minecraft:function"
        | "minecraft:dimension"
        | "minecraft:mob_effect"
        | "minecraft:resource"
        | "minecraft:resource_key"
        | "minecraft:loot_modifier"
        | "minecraft:loot_predicate"
        | "minecraft:loot_table" => parse_resource_location(reader),

        "minecraft:resource_or_tag"
        | "minecraft:resource_or_tag_key"
        | "minecraft:resource_selector" => parse_resource_or_tag(reader),

        // === Coordinates ===
        "minecraft:block_pos" => parse_block_pos(reader),
        "minecraft:column_pos" => parse_column_pos(reader),
        "minecraft:vec2" | "minecraft:rotation" => parse_vec2(reader),
        "minecraft:vec3" => parse_vec3(reader),

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
        "minecraft:item_stack" | "minecraft:item_predicate" => parse_item_stack(reader),

        // === Message (greedy) ===
        "minecraft:message" => parse_message(reader),

        // Unknown parser — accept any word liberally
        _ => parse_word(reader),
    }
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

fn parse_bool(reader: &mut StringReader) -> bool {
    let saved = reader.cursor();
    if reader.try_read_literal("true") || reader.try_read_literal("false") {
        return true;
    }
    reader.set_cursor(saved);
    false
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

fn float_in_bounds(value: f64, properties: Option<&serde_json::Value>) -> bool {
    let min = properties
        .and_then(|p| p.get("min"))
        .and_then(|value| value.as_f64());
    let max = properties
        .and_then(|p| p.get("max"))
        .and_then(|value| value.as_f64());

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

fn parse_brigadier_float(
    reader: &mut StringReader,
    properties: Option<&serde_json::Value>,
) -> bool {
    let saved = reader.cursor();
    if let Some(value) = reader.read_float() {
        if !float_in_bounds(value, properties) {
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

fn parse_brigadier_double(
    reader: &mut StringReader,
    properties: Option<&serde_json::Value>,
) -> bool {
    let saved = reader.cursor();
    if let Some(value) = reader.read_float() {
        if !float_in_bounds(value, properties) {
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
            if reader.read_string() && reader.at_token_boundary() {
                true
            } else {
                reader.set_cursor(saved);
                false
            }
        }
        _ => {
            // "word" — single unquoted word
            let saved = reader.cursor();
            let s = reader.read_unquoted_string();
            if !s.is_empty() && reader.at_token_boundary() {
                true
            } else {
                reader.set_cursor(saved);
                false
            }
        }
    }
}

// ============================================================================
// Simple word parser
// ============================================================================

fn parse_word(reader: &mut StringReader) -> bool {
    let saved = reader.cursor();
    let s = reader.read_unquoted_string();
    if !s.is_empty() && reader.at_token_boundary() {
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

fn parse_hex_color(reader: &mut StringReader) -> bool {
    let saved = reader.cursor();
    if reader.peek() != Some('#') {
        return false;
    }
    reader.read_char();
    for _ in 0..6 {
        match reader.peek() {
            Some(ch) if ch.is_ascii_hexdigit() => {
                reader.read_char();
            }
            _ => {
                reader.set_cursor(saved);
                return false;
            }
        }
    }
    if reader.at_token_boundary() {
        return true;
    }
    reader.set_cursor(saved);
    false
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

    // Try: N..N, N.., ..N, or just N
    let has_left = reader.read_integer().is_some();

    if reader.can_read() && reader.peek() == Some('.') {
        let dot_pos = reader.cursor();
        reader.read_char(); // first dot
        if reader.peek() == Some('.') {
            reader.read_char(); // second dot
                                // Try to read right side
            let has_right = reader.read_integer().is_some();
            if (has_left || has_right) && reader.at_token_boundary() {
                return true;
            }
        }
        // Not a range, backtrack to dots
        reader.set_cursor(dot_pos);
    }

    if has_left && reader.at_token_boundary() {
        return true;
    }

    reader.set_cursor(saved);
    false
}

fn parse_float_range(reader: &mut StringReader) -> bool {
    let saved = reader.cursor();

    let has_left = reader.read_float().is_some();

    if reader.can_read() && reader.peek() == Some('.') {
        let dot_pos = reader.cursor();
        reader.read_char();
        if reader.peek() == Some('.') {
            reader.read_char();
            let has_right = reader.read_float().is_some();
            if (has_left || has_right) && reader.at_token_boundary() {
                return true;
            }
        }
        reader.set_cursor(dot_pos);
    }

    if has_left && reader.at_token_boundary() {
        return true;
    }

    reader.set_cursor(saved);
    false
}

// ============================================================================
// Time parser
// ============================================================================

fn parse_time(reader: &mut StringReader, properties: Option<&serde_json::Value>) -> bool {
    let saved = reader.cursor();
    if let Some(value) = reader.read_float() {
        if !float_in_bounds(value, properties) {
            reader.set_cursor(saved);
            return false;
        }
        // Optional suffix: d, s, t
        if reader.can_read() {
            let ch = reader.peek().unwrap();
            if ch == 'd' || ch == 's' || ch == 't' {
                reader.read_char();
            }
        }
        if reader.at_token_boundary() {
            return true;
        }
    }
    reader.set_cursor(saved);
    false
}

// ============================================================================
// Entity/selector parsers
// ============================================================================

fn parse_entity(
    reader: &mut StringReader,
    parser_type: &str,
    properties: Option<&serde_json::Value>,
) -> bool {
    let saved = reader.cursor();

    // Try selector (@a, @s, @p, @e, @r, etc.)
    if reader.peek() == Some('@') {
        let selector_start = reader.cursor();
        if reader.read_selector() && reader.at_token_boundary() {
            let selector = reader.slice(selector_start, reader.cursor());
            if selector_allowed(&selector, parser_type, properties) {
                return true;
            }
        }
        reader.set_cursor(saved);
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
    }

    if parser_type == "minecraft:score_holder" {
        reader.set_cursor(saved);
        if reader.try_read_macro() {
            return true;
        }
        reader.set_cursor(saved);
    }

    // For score_holder, accept *
    if parser_type == "minecraft:score_holder" && reader.peek() == Some('*') {
        reader.read_char();
        if reader.at_token_boundary() {
            return true;
        }
        reader.set_cursor(saved);
    }

    // Try quoted string (player name can be quoted)
    if reader.peek() == Some('"') || reader.peek() == Some('\'') {
        if reader.read_quoted_string() && reader.at_token_boundary() {
            return true;
        }
        reader.set_cursor(saved);
    }

    // Try UUID format
    let uuid_saved = reader.cursor();
    if try_read_uuid(reader) && reader.at_token_boundary() {
        return true;
    }
    reader.set_cursor(uuid_saved);

    // Player name (unquoted word, can include hyphens for UUIDs)
    let s = reader.read_unquoted_string();
    if !s.is_empty() && reader.at_token_boundary() {
        return true;
    }

    reader.set_cursor(saved);
    false
}

fn selector_allowed(
    selector: &str,
    parser_type: &str,
    properties: Option<&serde_json::Value>,
) -> bool {
    let selector_kind = selector.chars().nth(1);
    if !matches!(
        selector_kind,
        Some('p') | Some('a') | Some('r') | Some('s') | Some('e') | Some('n')
    ) {
        return false;
    }

    if !selector_arguments_allowed(selector) {
        return false;
    }

    if parser_type == "minecraft:score_holder" {
        return true;
    }

    if parser_type == "minecraft:game_profile" && selector.starts_with("@e") {
        return false;
    }

    let entity_type = properties
        .and_then(|p| p.get("type"))
        .and_then(|value| value.as_str());
    let amount = properties
        .and_then(|p| p.get("amount"))
        .and_then(|value| value.as_str());

    if entity_type == Some("players") && selector_kind == Some('e') {
        return false;
    }

    if amount == Some("single") && matches!(selector_kind, Some('a') | Some('e')) {
        return selector_contains_limit_one(selector);
    }

    true
}

fn selector_arguments_allowed(selector: &str) -> bool {
    let Some(args_start) = selector.find('[') else {
        return true;
    };
    let args = selector[args_start + 1..].trim_end_matches(']');
    if args.trim().is_empty() {
        return true;
    }

    split_selector_arguments(args)
        .iter()
        .all(|part| selector_argument_allowed(part))
}

fn selector_argument_allowed(part: &str) -> bool {
    let part = part.trim();
    let Some((key, value)) = part.split_once('=') else {
        let key = part.trim_start_matches('!');
        return selector_key_allowed(key);
    };
    let key = key.trim().trim_start_matches('!');
    let value = value.trim();
    if !selector_key_allowed(key) {
        return false;
    }

    match key {
        "limit" => value.parse::<i32>().is_ok_and(|limit| limit >= 1),
        "distance" | "x_rotation" | "y_rotation" => {
            let mut reader = StringReader::new(value.trim_start_matches('!'));
            parse_float_range(&mut reader) && !reader.can_read()
        }
        "level" => {
            let mut reader = StringReader::new(value.trim_start_matches('!'));
            parse_int_range(&mut reader) && !reader.can_read()
        }
        "sort" => matches!(value, "nearest" | "furthest" | "random" | "arbitrary"),
        "gamemode" => matches!(
            value.trim_start_matches('!'),
            "survival" | "creative" | "adventure" | "spectator"
        ),
        _ => true,
    }
}

fn selector_key_allowed(key: &str) -> bool {
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

fn split_selector_arguments(args: &str) -> Vec<String> {
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
            ',' if depth == 0 => {
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

fn selector_contains_limit_one(selector: &str) -> bool {
    let Some(args_start) = selector.find('[') else {
        return false;
    };
    let args = selector[args_start + 1..].trim_end_matches(']');
    args.split(',').any(|part| {
        let mut pieces = part.splitn(2, '=');
        let key = pieces.next().map(str::trim);
        let value = pieces.next().map(str::trim);
        key == Some("limit") && value == Some("1")
    })
}

// ============================================================================
// Resource location parsers
// ============================================================================

/// Read a resource location without checking for a token boundary.
/// Returns true if at least one character was consumed.
fn read_resource_location_chars(reader: &mut StringReader) -> bool {
    let mut has_chars = false;
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
            || ch == ':'
        {
            reader.read_char();
            has_chars = true;
        } else {
            break;
        }
    }
    has_chars
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

    // Optional # prefix for tags
    if reader.peek() == Some('#') {
        reader.read_char();
    }

    if parse_resource_location(reader) {
        return true;
    }

    reader.set_cursor(saved);
    false
}

// ============================================================================
// Coordinate parsers
// ============================================================================

fn parse_block_pos(reader: &mut StringReader) -> bool {
    let saved = reader.cursor();
    if reader.read_coordinate() {
        if !reader.skip_required_whitespace() {
            reader.set_cursor(saved);
            return false;
        }
        if reader.read_coordinate() {
            if !reader.skip_required_whitespace() {
                reader.set_cursor(saved);
                return false;
            }
            if reader.read_coordinate() && reader.at_token_boundary() {
                return true;
            }
        }
    }
    reader.set_cursor(saved);
    if parse_whole_macro(reader) {
        return true;
    }
    reader.set_cursor(saved);
    false
}

fn parse_column_pos(reader: &mut StringReader) -> bool {
    let saved = reader.cursor();
    if reader.read_coordinate() {
        if !reader.skip_required_whitespace() {
            reader.set_cursor(saved);
            return false;
        }
        if reader.read_coordinate() && reader.at_token_boundary() {
            return true;
        }
    }
    reader.set_cursor(saved);
    if parse_whole_macro(reader) {
        return true;
    }
    reader.set_cursor(saved);
    false
}

fn parse_vec2(reader: &mut StringReader) -> bool {
    let saved = reader.cursor();
    if reader.read_coordinate() {
        if !reader.skip_required_whitespace() {
            reader.set_cursor(saved);
            return false;
        }
        if reader.read_coordinate() && reader.at_token_boundary() {
            return true;
        }
    }
    reader.set_cursor(saved);
    if parse_whole_macro(reader) {
        return true;
    }
    reader.set_cursor(saved);
    false
}

fn parse_vec3(reader: &mut StringReader) -> bool {
    let saved = reader.cursor();
    if reader.read_coordinate() {
        if !reader.skip_required_whitespace() {
            reader.set_cursor(saved);
            return false;
        }
        if reader.read_coordinate() {
            if !reader.skip_required_whitespace() {
                reader.set_cursor(saved);
                return false;
            }
            if reader.read_coordinate() && reader.at_token_boundary() {
                return true;
            }
        }
    }
    reader.set_cursor(saved);
    if parse_whole_macro(reader) {
        return true;
    }
    reader.set_cursor(saved);
    false
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

    // Read the particle type (resource location)
    if !parse_resource_location(reader) {
        reader.set_cursor(saved);
        return false;
    }

    // Some particles have extra parameters. We liberally accept remaining
    // non-whitespace tokens until the next argument context would take over.
    // For now, just accept the resource location part.
    // Parameters like dust 1.0 0.0 0.0 1.0 would be handled by the
    // tree structure having additional argument nodes.
    true
}

// ============================================================================
// UUID parser
// ============================================================================

fn try_read_uuid(reader: &mut StringReader) -> bool {
    // UUID format: 8-4-4-4-12 hex digits with dashes
    // Or just a hex string. Be liberal.
    let saved = reader.cursor();
    let mut count = 0;
    while reader.can_read() {
        let ch = reader.peek().unwrap();
        if ch.is_ascii_hexdigit() || ch == '-' {
            reader.read_char();
            count += 1;
        } else {
            break;
        }
    }
    // A UUID should be at least 32 hex chars + 4 dashes = 36 chars
    // But be liberal: accept anything with hex and dashes that's long enough
    if count >= 32 && reader.at_token_boundary() {
        return true;
    }
    reader.set_cursor(saved);
    false
}

fn parse_uuid(reader: &mut StringReader) -> bool {
    let saved = reader.cursor();
    if try_read_uuid(reader) {
        return true;
    }
    reader.set_cursor(saved);
    false
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

    // Number (with optional type suffix like 1b, 2s, 3L, 4.0f, 5.0d)
    if ch == '-' || ch == '+' || ch.is_ascii_digit() {
        let _ = reader.read_float();
        // Skip optional NBT type suffix
        if reader.can_read() {
            let suffix = reader.peek().unwrap();
            if "bBsSlLfFdD".contains(suffix) {
                reader.read_char();
            }
        }
        if reader.at_token_boundary() {
            return true;
        }
        reader.set_cursor(saved);
        return false;
    }

    // Unquoted string
    let s = reader.read_unquoted_string();
    if !s.is_empty() && reader.at_token_boundary() {
        return true;
    }

    reader.set_cursor(saved);
    false
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

        if ch == '"' || ch == '\'' {
            if !reader.read_quoted_string() {
                break;
            }
            has_content = true;
            continue;
        }

        if ch == '{' || ch == '[' {
            if !reader.read_nbt() {
                break;
            }
            has_content = true;
            continue;
        }

        if ch == '.' || ch == '_' || ch.is_ascii_alphanumeric() || ch == '-' {
            reader.read_char();
            has_content = true;
            continue;
        }

        break;
    }

    if has_content && reader.at_token_boundary() {
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

fn parse_item_stack(reader: &mut StringReader) -> bool {
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

    // Optional components [...] (1.20.5+ format)
    if reader.can_read() && reader.peek() == Some('[') && !reader.read_nbt() {
        reader.set_cursor(saved);
        return false;
    }

    // Optional NBT data {...} (legacy format)
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

// ============================================================================
// Message (greedy) parser
// ============================================================================

fn parse_message(reader: &mut StringReader) -> bool {
    if reader.can_read() {
        reader.read_greedy();
        true
    } else {
        false
    }
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
    fn test_parse_entity_selector() {
        let mut r = StringReader::new("@a[tag=foo] rest");
        assert!(parse_argument(&mut r, "minecraft:entity", None));
        assert_eq!(r.remaining(), " rest");

        for selector in [
            "@e[limit=abc]",
            "@e[limit=0]",
            "@e[distance=abc]",
            "@e[sort=sideways]",
            "@e[gamemode=flying]",
        ] {
            let mut invalid = StringReader::new(selector);
            assert!(
                !parse_argument(&mut invalid, "minecraft:entity", None),
                "selector should be rejected: {selector}"
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
    fn test_parse_message() {
        let mut r = StringReader::new("hello world @a");
        assert!(parse_argument(&mut r, "minecraft:message", None));
        assert!(r.remaining().is_empty());
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
    fn test_parse_uuid_rejects_plain_words() {
        let mut invalid = StringReader::new("not-a-uuid rest");
        assert!(!parse_argument(&mut invalid, "minecraft:uuid", None));

        let mut valid = StringReader::new("123e4567-e89b-12d3-a456-426614174000 rest");
        assert!(parse_argument(&mut valid, "minecraft:uuid", None));
        assert_eq!(valid.remaining(), " rest");
    }
}
