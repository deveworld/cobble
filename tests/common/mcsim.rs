//! A small Minecraft command simulator for runtime-semantics regression tests.
//!
//! It executes the subset of commands Cobble emits with Minecraft's
//! semantics: i32 wrapping scoreboard arithmetic with floor division and
//! floor modulus, `execute` forking over entities (`as`, `at`, `if`/`unless
//! score`, `store result`), function calls with `return` values, storage
//! paths (`data modify/get/remove`), and macro lines (`$...$(key)...`)
//! expanded from a storage compound. Anything outside that subset panics so a
//! test can never pass by silently skipping a command.
//!
//! Entities are plain names: `@a`/`@e` iterate the configured players in
//! order, `@p` is the first player, and `@s` is the current executor.

#![allow(dead_code)]

use serde_json::{Map, Value};
use std::collections::{HashMap, HashSet};
use std::fs;
use std::path::{Path, PathBuf};
use tempfile::TempDir;

/// Compile one Cobble source file with the library build entry point.
/// Returns the tempdir guard, the output directory, and the namespace.
pub fn compile(source: &str) -> Result<(TempDir, PathBuf), String> {
    let temp_dir = TempDir::new().unwrap();
    let input_file = temp_dir.path().join("main.cbl");
    let output_dir = temp_dir.path().join("output");
    fs::write(&input_file, source).unwrap();

    cobble::commands::build::build(cobble::commands::build::BuildOptions {
        input: Some(input_file),
        output: Some(output_dir.clone()),
        namespace: Some("sim".to_string()),
        pack_format: None,
        description: None,
        verbose: false,
        quiet: true,
        zip: false,
        experimental_resource_pack: false,
        validate: false,
        dry_run: false,
        commands_json: PathBuf::from("data/commands.json"),
    })?;

    Ok((temp_dir, output_dir))
}

pub fn load_functions(output_dir: &Path) -> HashMap<String, Vec<String>> {
    let function_dir = output_dir.join("data/sim/function");
    let mut functions = HashMap::new();
    for entry in fs::read_dir(&function_dir).unwrap() {
        let path = entry.unwrap().path();
        if path.extension().is_some_and(|ext| ext == "mcfunction") {
            let name = path.file_stem().unwrap().to_string_lossy().to_string();
            let commands = fs::read_to_string(&path)
                .unwrap()
                .lines()
                .map(str::to_string)
                .collect();
            functions.insert(name, commands);
        }
    }
    functions
}

fn read_tag(output_dir: &Path, tag: &str) -> Vec<String> {
    let path = output_dir.join(format!("data/minecraft/tags/function/{tag}.json"));
    let Ok(content) = fs::read_to_string(path) else {
        return Vec::new();
    };
    let json: Value = serde_json::from_str(&content).unwrap();
    json["values"]
        .as_array()
        .unwrap()
        .iter()
        .filter_map(|value| value.as_str())
        .map(|id| id.split_once(':').unwrap().1.to_string())
        .collect()
}

/// Outcome of running one command or function body.
enum Flow {
    Continue,
    Return(Option<i32>),
}

#[derive(Clone)]
struct Context {
    executor: Option<String>,
}

pub struct Sim {
    pub functions: HashMap<String, Vec<String>>,
    pub players: Vec<String>,
    pub scores: HashMap<(String, String), i32>,
    /// Objectives created by `scoreboard objectives add`. Reading or writing
    /// a score on any other objective fails the command in Minecraft, so the
    /// simulator panics instead of treating it as an unset score.
    pub objectives: HashSet<String>,
    pub storage: Value,
    /// Chat output: `say` lines as `<executor>: <message>`, `tellraw` lines as
    /// rendered plain text.
    pub log: Vec<String>,
    load_tag: Vec<String>,
    budget: usize,
}

impl Sim {
    /// Compile `source`, run the `minecraft:load` tag, and return the sim.
    pub fn load(source: &str, players: &[&str]) -> Sim {
        let (_temp, output_dir) = compile(source).unwrap_or_else(|e| panic!("build failed: {e}"));
        let mut sim = Sim {
            functions: load_functions(&output_dir),
            players: players.iter().map(|p| p.to_string()).collect(),
            scores: HashMap::new(),
            objectives: HashSet::new(),
            storage: Value::Object(Map::new()),
            log: Vec::new(),
            load_tag: read_tag(&output_dir, "load"),
            budget: 1_000_000,
        };
        for handler in sim.load_tag.clone() {
            sim.call(&handler);
        }
        sim
    }

    /// Run a function by name with no executor and return its result.
    pub fn call(&mut self, name: &str) -> Option<i32> {
        self.run_function(name, &Context { executor: None }, None)
    }

    /// Set a score from test setup, creating its objective like a datapack
    /// or player would have.
    pub fn set_score(&mut self, holder: &str, objective: &str, value: i32) {
        self.objectives.insert(objective.to_string());
        self.scores
            .insert((holder.to_string(), objective.to_string()), value);
    }

    fn require_objective(&self, objective: &str) {
        assert!(
            self.objectives.contains(objective),
            "objective `{objective}` does not exist; Minecraft fails the command"
        );
    }

    pub fn score(&self, holder: &str, objective: &str) -> i32 {
        self.scores
            .get(&(holder.to_string(), objective.to_string()))
            .copied()
            .unwrap_or_else(|| panic!("score {holder} {objective} was never set"))
    }

    pub fn var(&self, name: &str) -> i32 {
        self.score(name, "temp")
    }

    pub fn try_score(&self, holder: &str, objective: &str) -> Option<i32> {
        self.scores
            .get(&(holder.to_string(), objective.to_string()))
            .copied()
    }

    pub fn function_text(&self, name: &str) -> String {
        self.functions
            .get(name)
            .unwrap_or_else(|| panic!("function `{name}` not generated"))
            .join("\n")
    }

    fn run_function(
        &mut self,
        name: &str,
        context: &Context,
        macro_args: Option<&Value>,
    ) -> Option<i32> {
        let commands = self
            .functions
            .get(name)
            .unwrap_or_else(|| panic!("function `{name}` not found in generated pack"))
            .clone();
        for command in &commands {
            let command = match command.strip_prefix('$') {
                Some(macro_line) => {
                    // Minecraft refuses to load the function at all.
                    assert!(
                        macro_line.contains("$("),
                        "macro line without variables in `{name}`: {command}"
                    );
                    let args = macro_args.unwrap_or_else(|| {
                        panic!("macro line in `{name}` run without arguments: {command}")
                    });
                    expand_macro(macro_line, args)
                }
                None => command.clone(),
            };
            if let Flow::Return(value) = self.exec(&command, context) {
                return value;
            }
        }
        None
    }

    fn exec(&mut self, command: &str, context: &Context) -> Flow {
        self.budget = self
            .budget
            .checked_sub(1)
            .expect("command budget exhausted (runaway recursion?)");
        let command = command.trim();
        if command.is_empty() || command.starts_with('#') || command.starts_with("gamerule ") {
            return Flow::Continue;
        }
        if let Some(rest) = command.strip_prefix("scoreboard objectives add ") {
            let objective = rest.split_whitespace().next().expect("objective name");
            self.objectives.insert(objective.to_string());
            return Flow::Continue;
        }
        if let Some(rest) = command.strip_prefix("execute ") {
            return self.exec_execute(rest, context);
        }
        if let Some(rest) = command.strip_prefix("return ") {
            if let Some(inner) = rest.strip_prefix("run ") {
                let value = self.eval(inner, context);
                return Flow::Return(value);
            }
            if rest == "fail" {
                return Flow::Return(None);
            }
            return Flow::Return(Some(rest.trim().parse().expect("return value")));
        }
        self.eval(command, context);
        Flow::Continue
    }

    /// Run a non-control command and return its result value.
    fn eval(&mut self, command: &str, context: &Context) -> Option<i32> {
        let command = command.trim();
        if let Some(rest) = command.strip_prefix("function ") {
            let (target, args) = match rest.split_once(" with storage ") {
                Some((target, storage)) => {
                    let (_, path) = storage.split_once(' ').expect("storage path");
                    let args = self
                        .storage_get(path)
                        .cloned()
                        .unwrap_or_else(|| panic!("`function ... with storage` of missing {path}"));
                    (target, Some(args))
                }
                None => (rest, None),
            };
            let name = target.trim().split_once(':').expect("qualified function").1;
            return self.run_function(name, context, args.as_ref());
        }
        if let Some(rest) = command.strip_prefix("scoreboard players ") {
            return self.exec_scoreboard(rest, context);
        }
        if let Some(rest) = command.strip_prefix("data ") {
            return self.exec_data(rest);
        }
        if let Some(message) = command.strip_prefix("say ") {
            let who = context
                .executor
                .clone()
                .unwrap_or_else(|| "Server".to_string());
            self.log.push(format!("{who}: {message}"));
            return Some(1);
        }
        if let Some(rest) = command.strip_prefix("tellraw ") {
            let (_, json) = split_selector(rest);
            let rendered = self.render_text(
                &serde_json::from_str(json.trim())
                    .unwrap_or_else(|e| panic!("tellraw JSON did not parse ({e}): {json}")),
                context,
            );
            self.log.push(rendered);
            return Some(1);
        }
        panic!("simulator cannot execute: {command}");
    }

    fn exec_execute(&mut self, rest: &str, context: &Context) -> Flow {
        let mut contexts = vec![context.clone()];
        let mut stores: Vec<Store> = Vec::new();
        let mut rest = rest.trim_start();
        loop {
            if let Some(after) = rest.strip_prefix("run ") {
                for ctx in &contexts {
                    let flow = if after.starts_with("return ") || after.starts_with("execute ") {
                        self.exec(after, ctx)
                    } else {
                        let value = self.eval(after, ctx);
                        for store in &stores {
                            self.apply_store(store, value, ctx);
                        }
                        Flow::Continue
                    };
                    if let Flow::Return(value) = flow {
                        return Flow::Return(value);
                    }
                }
                return Flow::Continue;
            }
            let (keyword, after) = rest.split_once(' ').expect("execute subcommand");
            match keyword {
                "as" => {
                    let (selector, after) = split_selector(after);
                    contexts = contexts
                        .iter()
                        .flat_map(|ctx| self.resolve_selector(selector, ctx))
                        .map(|executor| Context {
                            executor: Some(executor),
                        })
                        .collect();
                    rest = after;
                }
                "at" => {
                    let (_, after) = split_selector(after);
                    rest = after;
                }
                "if" | "unless" => {
                    let negate = keyword == "unless";
                    let (kind, after) = after.split_once(' ').expect("condition kind");
                    assert_eq!(kind, "score", "unsupported execute condition: {rest}");
                    let tokens: Vec<&str> = after.splitn(6, ' ').collect();
                    self.require_objective(tokens[1]);
                    if tokens[2] != "matches" {
                        self.require_objective(tokens[4]);
                    }
                    contexts.retain(|ctx| {
                        let holder = self.holder(tokens[0], ctx);
                        let value = self.try_score(&holder, tokens[1]);
                        let passed = if tokens[2] == "matches" {
                            value.is_some_and(|v| matches_range(v, tokens[3]))
                        } else {
                            let other_holder = self.holder(tokens[3], ctx);
                            let other = self.try_score(&other_holder, tokens[4]);
                            match (value, other) {
                                (Some(a), Some(b)) => match tokens[2] {
                                    "<" => a < b,
                                    "<=" => a <= b,
                                    "=" => a == b,
                                    ">=" => a >= b,
                                    ">" => a > b,
                                    op => panic!("unsupported comparison `{op}`"),
                                },
                                _ => false,
                            }
                        };
                        passed != negate
                    });
                    let consumed = if tokens[2] == "matches" { 4 } else { 5 };
                    rest = after
                        .splitn(consumed + 1, ' ')
                        .nth(consumed)
                        .expect("execute chain continues");
                }
                "store" => {
                    let tokens: Vec<&str> = after.splitn(7, ' ').collect();
                    assert_eq!(tokens[0], "result", "only `store result` is simulated");
                    match tokens[1] {
                        "score" => {
                            stores.push(Store::Score {
                                holder: tokens[2].to_string(),
                                objective: tokens[3].to_string(),
                            });
                            rest = after.splitn(5, ' ').nth(4).expect("store chain");
                        }
                        "storage" => {
                            stores.push(Store::Storage {
                                path: tokens[3].to_string(),
                                scale: tokens[5].parse().expect("store scale"),
                            });
                            rest = after.splitn(7, ' ').nth(6).expect("store chain");
                        }
                        other => panic!("unsupported store target `{other}`"),
                    }
                }
                other => panic!("unsupported execute subcommand `{other}` in: execute {rest}"),
            }
        }
    }

    fn apply_store(&mut self, store: &Store, value: Option<i32>, ctx: &Context) {
        // A failed command still stores: `execute store result` writes 0.
        let value = value.unwrap_or(0);
        match store {
            Store::Score { holder, objective } => {
                self.require_objective(objective);
                let holder = self.holder(holder, ctx);
                self.scores.insert((holder, objective.clone()), value);
            }
            Store::Storage { path, scale } => {
                let scaled = (value as f64 * scale) as i64;
                self.storage_set(path, Value::from(scaled));
            }
        }
    }

    fn exec_scoreboard(&mut self, rest: &str, ctx: &Context) -> Option<i32> {
        let tokens: Vec<&str> = rest.split_whitespace().collect();
        let holder = self.holder(tokens[1], ctx);
        self.require_objective(tokens[2]);
        if tokens[0] == "operation" {
            self.require_objective(tokens[5]);
        }
        let key = (holder.clone(), tokens[2].to_string());
        match tokens[0] {
            "set" => {
                let value: i32 = tokens[3].parse().unwrap();
                self.scores.insert(key, value);
                Some(value)
            }
            "add" | "remove" => {
                let delta: i32 = tokens[3].parse().unwrap();
                let current = self.scores.get(&key).copied().unwrap_or(0);
                let value = if tokens[0] == "add" {
                    current.wrapping_add(delta)
                } else {
                    current.wrapping_sub(delta)
                };
                self.scores.insert(key, value);
                Some(value)
            }
            "get" => Some(
                *self
                    .scores
                    .get(&key)
                    .unwrap_or_else(|| panic!("scoreboard get of unset score {key:?}")),
            ),
            "reset" => {
                self.scores.remove(&key);
                Some(0)
            }
            "operation" => {
                let source_holder = self.holder(tokens[4], ctx);
                let source = *self
                    .scores
                    .get(&(source_holder.clone(), tokens[5].to_string()))
                    .unwrap_or_else(|| {
                        panic!("operation reads unset score {source_holder} {}", tokens[5])
                    });
                let current = self.scores.get(&key).copied().unwrap_or(0);
                let value = match tokens[3] {
                    "=" => source,
                    "+=" => current.wrapping_add(source),
                    "-=" => current.wrapping_sub(source),
                    "*=" => current.wrapping_mul(source),
                    "/=" => {
                        assert_ne!(source, 0, "division by zero fails the command in Minecraft");
                        floor_div(current, source)
                    }
                    "%=" => {
                        assert_ne!(source, 0, "modulo by zero fails the command in Minecraft");
                        floor_mod(current, source)
                    }
                    "<" => current.min(source),
                    ">" => current.max(source),
                    "><" => {
                        self.scores
                            .insert((source_holder, tokens[5].to_string()), current);
                        source
                    }
                    op => panic!("unsupported operation `{op}`"),
                };
                self.scores.insert(key, value);
                Some(value)
            }
            op => panic!("unsupported scoreboard subcommand `{op}`"),
        }
    }

    fn exec_data(&mut self, rest: &str) -> Option<i32> {
        if let Some(after) = rest.strip_prefix("modify storage ") {
            let (_, after) = after.split_once(' ').expect("storage id");
            let (path, after) = after.split_once(' ').expect("storage path");
            let value = if let Some(snbt) = after.strip_prefix("set value ") {
                parse_snbt(snbt.trim())
            } else if let Some(source) = after.strip_prefix("set from storage ") {
                let (_, source_path) = source.split_once(' ').expect("source path");
                self.storage_get(source_path.trim())
                    .cloned()
                    .unwrap_or_else(|| panic!("data copy from missing path {source_path}"))
            } else {
                panic!("unsupported data modify: {rest}");
            };
            self.storage_set(path, value);
            return Some(1);
        }
        if let Some(after) = rest.strip_prefix("remove storage ") {
            let (_, path) = after.split_once(' ').expect("storage path");
            self.storage_remove(path.trim());
            return Some(1);
        }
        if let Some(after) = rest.strip_prefix("get storage ") {
            let (_, path_and_scale) = after.split_once(' ').expect("storage path");
            let (path, scale) = match path_and_scale.trim().split_once(' ') {
                Some((path, scale)) => (path, Some(scale.parse::<f64>().expect("data get scale"))),
                None => (path_and_scale.trim(), None),
            };
            let value = self
                .storage_get(path)
                .unwrap_or_else(|| panic!("data get of missing path {path}"));
            // With a scale, `data get` only accepts numbers and fails on a
            // list, compound, or string.
            if let Some(scale) = scale {
                return match value {
                    Value::Number(n) => Some((n.as_f64().unwrap() * scale).floor() as i32),
                    Value::Bool(b) => Some((*b as i32 as f64 * scale).floor() as i32),
                    _ => None,
                };
            }
            return Some(match value {
                Value::Number(n) => n.as_f64().unwrap() as i32,
                Value::Bool(b) => *b as i32,
                Value::String(s) => s.chars().count() as i32,
                Value::Array(items) => items.len() as i32,
                Value::Object(map) => map.len() as i32,
                Value::Null => 0,
            });
        }
        panic!("unsupported data command: data {rest}");
    }

    fn holder(&self, holder: &str, ctx: &Context) -> String {
        if holder == "@s" {
            ctx.executor
                .clone()
                .expect("`@s` used without an executing entity")
        } else {
            assert!(
                !holder.starts_with('@'),
                "score holder selector `{holder}` is not simulated"
            );
            holder.to_string()
        }
    }

    fn resolve_selector(&self, selector: &str, ctx: &Context) -> Vec<String> {
        match selector {
            "@a" | "@e" => self.players.clone(),
            "@p" => self.players.first().cloned().into_iter().collect(),
            "@s" => ctx.executor.clone().into_iter().collect(),
            other => panic!("unsupported selector `{other}`"),
        }
    }

    fn render_text(&self, value: &Value, ctx: &Context) -> String {
        match value {
            Value::String(text) => text.clone(),
            Value::Array(items) => items
                .iter()
                .map(|item| self.render_text(item, ctx))
                .collect(),
            Value::Object(map) => {
                if let Some(text) = map.get("text") {
                    text.as_str().unwrap_or_default().to_string()
                } else if let Some(score) = map.get("score") {
                    let holder = self.holder(score["name"].as_str().unwrap(), ctx);
                    self.try_score(&holder, score["objective"].as_str().unwrap())
                        .map(|v| v.to_string())
                        .unwrap_or_default()
                } else if let Some(path) = map.get("nbt") {
                    match self.storage_get(path.as_str().unwrap()) {
                        Some(Value::String(s)) => s.clone(),
                        Some(other) => other.to_string(),
                        None => String::new(),
                    }
                } else {
                    panic!("unsupported text component: {value}");
                }
            }
            other => other.to_string(),
        }
    }

    fn storage_get(&self, path: &str) -> Option<&Value> {
        let mut current = &self.storage;
        for segment in parse_path(path) {
            current = match segment {
                Segment::Key(key) => current.get(&key)?,
                Segment::Index(index) => {
                    let items = current.as_array()?;
                    let index = if index < 0 {
                        items.len() as i64 + index
                    } else {
                        index
                    };
                    items.get(usize::try_from(index).ok()?)?
                }
            };
        }
        Some(current)
    }

    fn storage_set(&mut self, path: &str, value: Value) {
        let segments = parse_path(path);
        let mut current = &mut self.storage;
        for (position, segment) in segments.iter().enumerate() {
            let last = position + 1 == segments.len();
            current = match segment {
                Segment::Key(key) => {
                    if !current.is_object() {
                        *current = Value::Object(Map::new());
                    }
                    let map = current.as_object_mut().unwrap();
                    if last {
                        map.insert(key.clone(), value);
                        return;
                    }
                    map.entry(key.clone())
                        .or_insert_with(|| Value::Object(Map::new()))
                }
                Segment::Index(index) => {
                    let items = current
                        .as_array_mut()
                        .unwrap_or_else(|| panic!("index into non-list at {path}"));
                    let index = if *index < 0 {
                        items.len() as i64 + index
                    } else {
                        *index
                    };
                    let slot = items
                        .get_mut(index as usize)
                        .unwrap_or_else(|| panic!("list index out of range at {path}"));
                    if last {
                        *slot = value;
                        return;
                    }
                    slot
                }
            };
        }
    }

    fn storage_remove(&mut self, path: &str) {
        let mut segments = parse_path(path);
        let Some(last) = segments.pop() else {
            return;
        };
        let mut current = &mut self.storage;
        for segment in segments {
            current = match segment {
                Segment::Key(key) => match current.get_mut(&key) {
                    Some(next) => next,
                    None => return,
                },
                Segment::Index(index) => match current.get_mut(index as usize) {
                    Some(next) => next,
                    None => return,
                },
            };
        }
        match last {
            Segment::Key(key) => {
                if let Some(map) = current.as_object_mut() {
                    map.remove(&key);
                }
            }
            Segment::Index(index) => {
                if let Some(items) = current.as_array_mut() {
                    if (index as usize) < items.len() {
                        items.remove(index as usize);
                    }
                }
            }
        }
    }
}

enum Store {
    Score { holder: String, objective: String },
    Storage { path: String, scale: f64 },
}

enum Segment {
    Key(String),
    Index(i64),
}

fn parse_path(path: &str) -> Vec<Segment> {
    let mut segments = Vec::new();
    let chars: Vec<char> = path.trim().chars().collect();
    let mut i = 0;
    let mut current = String::new();
    while i < chars.len() {
        match chars[i] {
            '.' => {
                if !current.is_empty() {
                    segments.push(Segment::Key(std::mem::take(&mut current)));
                }
            }
            '"' => {
                i += 1;
                while i < chars.len() && chars[i] != '"' {
                    current.push(chars[i]);
                    i += 1;
                }
            }
            '[' => {
                if !current.is_empty() {
                    segments.push(Segment::Key(std::mem::take(&mut current)));
                }
                let mut index = String::new();
                i += 1;
                while chars[i] != ']' {
                    index.push(chars[i]);
                    i += 1;
                }
                segments.push(Segment::Index(index.trim().parse().expect("list index")));
            }
            ch => current.push(ch),
        }
        i += 1;
    }
    if !current.is_empty() {
        segments.push(Segment::Key(current));
    }
    segments
}

/// Parse the SNBT subset Cobble emits: ints (optionally suffixed `b`, `s`,
/// `l`), doubles, quoted strings, lists, and compounds.
fn parse_snbt(text: &str) -> Value {
    let chars: Vec<char> = text.chars().collect();
    let mut position = 0;
    let value = parse_snbt_value(&chars, &mut position);
    assert_eq!(position, chars.len(), "trailing SNBT input in {text}");
    value
}

fn parse_snbt_value(chars: &[char], position: &mut usize) -> Value {
    skip_spaces(chars, position);
    match chars[*position] {
        '"' | '\'' => {
            let quote = chars[*position];
            *position += 1;
            let mut text = String::new();
            while chars[*position] != quote {
                if chars[*position] == '\\' {
                    *position += 1;
                }
                text.push(chars[*position]);
                *position += 1;
            }
            *position += 1;
            Value::String(text)
        }
        '[' => {
            *position += 1;
            let mut items = Vec::new();
            loop {
                skip_spaces(chars, position);
                if chars[*position] == ']' {
                    *position += 1;
                    break;
                }
                items.push(parse_snbt_value(chars, position));
                skip_spaces(chars, position);
                if chars[*position] == ',' {
                    *position += 1;
                }
            }
            Value::Array(items)
        }
        '{' => {
            *position += 1;
            let mut map = Map::new();
            loop {
                skip_spaces(chars, position);
                if chars[*position] == '}' {
                    *position += 1;
                    break;
                }
                let key = match parse_snbt_value(chars, position) {
                    Value::String(key) => key,
                    other => other.to_string(),
                };
                skip_spaces(chars, position);
                assert_eq!(chars[*position], ':', "SNBT compound key separator");
                *position += 1;
                let value = parse_snbt_value(chars, position);
                map.insert(key, value);
                skip_spaces(chars, position);
                if chars[*position] == ',' {
                    *position += 1;
                }
            }
            Value::Object(map)
        }
        _ => {
            let mut token = String::new();
            while *position < chars.len() && !",]}:".contains(chars[*position]) {
                token.push(chars[*position]);
                *position += 1;
            }
            let token = token.trim();
            let numeric = token.trim_end_matches(['b', 's', 'l', 'B', 'S', 'L']);
            if let Ok(int) = numeric.parse::<i64>() {
                Value::from(int)
            } else if let Ok(float) = token.trim_end_matches(['d', 'f', 'D', 'F']).parse::<f64>() {
                Value::from(float)
            } else if token == "true" {
                Value::from(1)
            } else if token == "false" {
                Value::from(0)
            } else {
                Value::String(token.to_string())
            }
        }
    }
}

fn skip_spaces(chars: &[char], position: &mut usize) {
    while *position < chars.len() && chars[*position] == ' ' {
        *position += 1;
    }
}

fn expand_macro(line: &str, args: &Value) -> String {
    let mut result = String::new();
    let mut rest = line;
    while let Some(start) = rest.find("$(") {
        result.push_str(&rest[..start]);
        let end = rest[start..].find(')').expect("closed macro placeholder") + start;
        let key = &rest[start + 2..end];
        let value = args
            .get(key)
            .unwrap_or_else(|| panic!("macro argument `{key}` missing for: {line}"));
        match value {
            Value::String(text) => result.push_str(text),
            other => result.push_str(&other.to_string()),
        }
        rest = &rest[end + 1..];
    }
    result.push_str(rest);
    result
}

/// Split a leading selector (with an optional `[...]` argument list) from the
/// rest of a command.
fn split_selector(text: &str) -> (&str, &str) {
    let text = text.trim_start();
    let mut depth = 0;
    for (index, ch) in text.char_indices() {
        match ch {
            '[' => depth += 1,
            ']' => depth -= 1,
            ' ' if depth == 0 => return (&text[..index], &text[index + 1..]),
            _ => {}
        }
    }
    (text, "")
}

fn matches_range(value: i32, range: &str) -> bool {
    if let Some(index) = range.find("..") {
        let (low, high) = (&range[..index], &range[index + 2..]);
        let low_ok = low.is_empty() || value >= low.parse::<i32>().unwrap();
        let high_ok = high.is_empty() || value <= high.parse::<i32>().unwrap();
        low_ok && high_ok
    } else {
        value == range.parse::<i32>().unwrap()
    }
}

/// Minecraft `/=` is `Mth.floorDiv`.
pub fn floor_div(a: i32, b: i32) -> i32 {
    let q = a.wrapping_div(b);
    if a.wrapping_rem(b) != 0 && ((a < 0) != (b < 0)) {
        q - 1
    } else {
        q
    }
}

/// Minecraft `%=` is `Mth.positiveModulo` (a floor modulus).
pub fn floor_mod(a: i32, b: i32) -> i32 {
    a.wrapping_sub(floor_div(a, b).wrapping_mul(b))
}
