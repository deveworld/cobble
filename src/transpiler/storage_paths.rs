//! Shared storage-path planning and dynamic-path lowering.
//!
//! `Transpiler` (statement lowering) and `ExpressionEvaluator` (expression
//! lowering) both resolve `config.count` / `values[i]` / `groups[key]`
//! expressions into storage path plans and emit the dynamic macro-argument
//! plumbing around them. The logic lives here as provided methods on
//! [`StoragePathOps`]; each host supplies its symbol tables through a few
//! accessors.

use crate::ast::{CobbleType, Expression};
use crate::transpiler::container_writes;
use crate::transpiler::data_pack::DataPack;
use crate::transpiler::expression_evaluator::ExpressionEvaluator;
use crate::transpiler::statement_processors::serialize_snbt;
use crate::transpiler::symbols::Symbols;
use crate::transpiler::{
    escape_quoted, GeneratedCommand, GeneratedCommandKind, SourceLocation, TranspileError,
    Transpiler,
};

pub(in crate::transpiler) trait StoragePathOps {
    fn storage_symbols(&self) -> &Symbols;
    /// Parameters of the function currently being lowered.
    fn storage_current_params(&self) -> &[String];
    fn storage_data_pack(&self) -> &DataPack;
    fn storage_data_pack_mut(&mut self) -> &mut DataPack;
    /// Source location attached to generated helper-function commands.
    fn storage_helper_source(&self) -> Option<SourceLocation>;
    fn storage_eval_to_target(
        &mut self,
        expr: &Expression,
        target: &str,
    ) -> Result<Vec<String>, TranspileError>;

    /// Whether `name` is a parameter of the function currently being lowered.
    fn is_storage_current_param(&self, name: &str) -> bool {
        self.storage_current_params()
            .iter()
            .any(|param| param == name)
    }

    /// The type of the value `expr` names: a variable's type, or the type
    /// an item or a field reads as (see [`Self::storage_read_type`]). A
    /// parameter gives `Unknown`.
    fn storage_expr_type(&self, expr: &Expression) -> Option<CobbleType> {
        match expr {
            Expression::Identifier(name) if self.is_storage_current_param(name) => {
                Some(CobbleType::Unknown)
            }
            Expression::Identifier(name) => {
                self.storage_symbols().variable_types.get(name).cloned()
            }
            Expression::Attribute(..) | Expression::Subscript(..) => {
                Some(self.storage_read_type(expr))
            }
            _ => None,
        }
    }

    /// The type of the value `expr` reads from storage, at any depth, as
    /// `grid[r][c]`: the type the last assigned literal has there, when
    /// every write of the program that can reach it has the same type. A
    /// parameter, a list or map Cobble didn't see a literal for, or an item
    /// written with values of other types gives `Unknown`.
    fn storage_read_type(&self, expr: &Expression) -> CobbleType {
        if !matches!(expr, Expression::Subscript(..) | Expression::Attribute(..)) {
            return CobbleType::Unknown;
        }
        let symbols = self.storage_symbols();
        let Some((name, steps)) = container_writes::access_path(expr) else {
            return CobbleType::Unknown;
        };
        if self.is_storage_current_param(name) {
            return CobbleType::Unknown;
        }
        let value_type = |value: &Expression| match value {
            Expression::Number(_) | Expression::Boolean(_) => CobbleType::Integer,
            Expression::String(_) => CobbleType::String,
            Expression::Array(_) => CobbleType::List,
            Expression::Map(_) => CobbleType::Map,
            Expression::Identifier(name) => match symbols.variable_types.get(name) {
                Some(CobbleType::Boolean) => CobbleType::Integer,
                Some(known) => known.clone(),
                None => CobbleType::Unknown,
            },
            Expression::Binary(..) | Expression::Unary(..) => CobbleType::Integer,
            _ => CobbleType::Unknown,
        };
        let literal = symbols
            .variables
            .get(name)
            .map_or(CobbleType::Unknown, |value| {
                container_writes::literal_type(value, &steps, &value_type)
            });
        // A string variable gives characters whatever it was assigned.
        let literal = match symbols.variable_types.get(name) {
            Some(CobbleType::String)
                if steps
                    .iter()
                    .all(|step| matches!(step, container_writes::Step::Item(_))) =>
            {
                CobbleType::String
            }
            _ => literal,
        };
        container_writes::read_type(&symbols.container_writes, name, &steps, literal)
    }

    /// The type of the value `index` gives as a subscript key: `String`,
    /// `Integer`, or `Unknown` when only the running pack knows, as for a
    /// parameter or an item of unknown type.
    fn storage_key_type(&self, index: &Expression) -> CobbleType {
        let symbols = self.storage_symbols();
        match index {
            Expression::String(_) => CobbleType::String,
            Expression::Number(_) | Expression::Boolean(_) => CobbleType::Integer,
            Expression::Identifier(name) => {
                if symbols.compile_time_constants.contains_key(name) {
                    return CobbleType::Integer;
                }
                if self.is_storage_current_param(name) {
                    return CobbleType::Unknown;
                }
                match symbols.variable_types.get(name) {
                    Some(CobbleType::Boolean) => CobbleType::Integer,
                    Some(CobbleType::Unknown) | None
                        if symbols.variable_storage_paths.contains_key(name) =>
                    {
                        CobbleType::Unknown
                    }
                    Some(CobbleType::Unknown) | None => CobbleType::Integer,
                    Some(known) => known.clone(),
                }
            }
            Expression::Subscript(base, _)
                if self.storage_expr_type(base) == Some(CobbleType::String) =>
            {
                CobbleType::String
            }
            Expression::Subscript(..) | Expression::Attribute(..) => {
                match self.storage_read_type(index) {
                    CobbleType::Boolean => CobbleType::Integer,
                    known => known,
                }
            }
            Expression::Call(func, _) if matches!(&**func, Expression::Identifier(name) if name == "str") => {
                CobbleType::String
            }
            Expression::Binary(..) | Expression::Unary(..) | Expression::Call(..) => {
                CobbleType::Integer
            }
            _ => CobbleType::Unknown,
        }
    }

    /// Whether `base[index]` picks a map field, a list item or a character,
    /// or one of them chosen when the pack runs. The key's type decides; a
    /// key whose type only the running pack knows goes by the type of the
    /// base, and by the key's own value when that isn't known either. Every
    /// read, write, augmented assignment, `in`, and `pop` asks this.
    fn subscript_kind(&self, base: &Expression, index: &Expression) -> SubscriptKind {
        let base_type = self.storage_expr_type(base).unwrap_or(CobbleType::Unknown);
        match self.storage_key_type(index) {
            CobbleType::String => SubscriptKind::Key,
            // A number is no map key; the key commands report it.
            _ if base_type == CobbleType::Map => SubscriptKind::Key,
            CobbleType::Integer => SubscriptKind::Index,
            _ if matches!(base_type, CobbleType::List | CobbleType::String) => SubscriptKind::Index,
            _ => SubscriptKind::Runtime,
        }
    }

    fn resolve_storage_path_plan(&self, expr: &Expression) -> Option<StoragePathPlan> {
        match expr {
            Expression::Identifier(name) => self
                .storage_symbols()
                .variable_storage_paths
                .get(name)
                .cloned()
                .or_else(|| {
                    self.is_storage_current_param(name)
                        .then(|| format!("args.{}", name))
                })
                .map(StoragePathPlan::static_path),
            Expression::Attribute(obj, attr) => {
                let base_path = self.resolve_storage_path_plan(obj)?;
                Some(base_path.append_field(attr))
            }
            Expression::Subscript(obj, index) => {
                let base_path = self.resolve_storage_path_plan(obj)?;
                Some(match self.subscript_kind(obj, index) {
                    SubscriptKind::Key => match &**index {
                        Expression::String(key) => base_path.append_key(key),
                        _ => base_path.append_dynamic_key((**index).clone()),
                    },
                    SubscriptKind::Index => match &**index {
                        Expression::Number(n) => base_path.append_static_index(*n as i32),
                        index => match static_integer(self.storage_symbols(), index) {
                            Some(n) => base_path.append_static_index(n),
                            None => base_path.append_dynamic_index(index.clone()),
                        },
                    },
                    SubscriptKind::Runtime => base_path.append_runtime_key((**index).clone()),
                })
            }
            _ => None,
        }
    }

    fn add_generated_helper_function(
        &mut self,
        name: String,
        commands: Vec<String>,
        kind: GeneratedCommandKind,
    ) {
        let source = self.storage_helper_source();
        let metadata = commands
            .iter()
            .enumerate()
            .map(|(index, command)| {
                (
                    index,
                    GeneratedCommand::new(command.clone(), source.clone(), kind.clone()),
                )
            })
            .collect();
        self.storage_data_pack_mut()
            .add_function_with_metadata(name, commands, metadata);
    }
}

/// Lowering of Cobble values into `data modify storage` command sequences:
/// static SNBT fast path, shell-plus-runtime-patch for containers holding
/// runtime values, and leaf sets for scalars. Provided methods over a small
/// accessor surface, like [`StoragePathOps`], so the logic lives once here.
pub(in crate::transpiler) trait StorageValueOps: StoragePathOps {
    /// Next unique id for generated temp scores / storage paths.
    fn storage_next_temp_id(&mut self) -> u32;
    /// Whether `name` is a macro parameter of the function currently being lowered.
    fn is_storage_macro_param(&self, name: &str) -> bool;
    /// Objective for a scoreboard-backed identifier, if `name` is one.
    fn storage_scoreboard_objective(&self, name: &str) -> Option<&str>;
    /// Static SNBT serialization for literal/const expressions.
    fn storage_static_snbt(&self, expr: &Expression) -> Result<String, TranspileError>;

    /// Commands that fill the macro arguments of `plan`'s path at
    /// `_cobble_dyn_<id>`, and that path. A key whose kind the running pack
    /// decides also sets the score that picks the path text (see
    /// [`StoragePathPlan::helper_lines`]).
    fn prepare_dynamic_storage_path(
        &mut self,
        id: u32,
        plan: &StoragePathPlan,
    ) -> Result<(Vec<String>, String), TranspileError> {
        let args_path = format!("_cobble_dyn_{}", id);
        let namespace = self.storage_data_pack().namespace.clone();
        let mut commands = vec![format!(
            "data remove storage {}:global {}",
            namespace, args_path
        )];
        let runtime = plan
            .dynamic_args()
            .iter()
            .filter(|arg| arg.kind == DynamicStoragePathArgKind::Runtime)
            .count();
        if runtime > MAX_RUNTIME_KEYS {
            return Err(format!(
                "This path has {runtime} subscripts whose key may be text or a number, and Cobble picks at most {MAX_RUNTIME_KEYS} when the pack runs. Copy some of the keys into variables that hold a known type first, such as `i = int_value + 0` or `k = str(value)`."
            )
            .into());
        }

        for arg in plan.dynamic_args() {
            match arg.kind {
                DynamicStoragePathArgKind::Index => {
                    let temp_score = format!("#cobble_dyn_{}_{}", id, arg.placeholder);
                    commands.extend(self.storage_eval_to_target(&arg.expr, &temp_score)?);
                    commands.push(format!(
                        "execute store result storage {}:global {}.{} int 1 run scoreboard players get {} temp",
                        namespace, args_path, arg.placeholder, temp_score
                    ));
                }
                DynamicStoragePathArgKind::Key => {
                    let target_path = format!("{}.{}", args_path, arg.placeholder);
                    commands
                        .extend(self.dynamic_storage_key_arg_commands(&target_path, &arg.expr)?);
                }
                DynamicStoragePathArgKind::Runtime => {
                    let base = format!("{}.{}", args_path, arg.placeholder);
                    commands.extend(self.runtime_key_commands(
                        &base,
                        &arg.expr,
                        &runtime_kind_score(id, &arg.placeholder),
                    )?);
                }
            }
        }

        Ok((commands, args_path))
    }

    /// Commands that store the key `expr` at `base_k` as it is, its number
    /// at `base_i`, and in `kind_score` 1 when it is a number and 0 when it
    /// is text. A number then picks an item and text a field.
    fn runtime_key_commands(
        &mut self,
        base: &str,
        expr: &Expression,
        kind_score: &str,
    ) -> Result<Vec<String>, TranspileError> {
        let namespace = self.storage_data_pack().namespace.clone();
        let mut commands = self.storage_leaf_set_commands(&format!("{base}_k"), expr)?;
        commands.extend([
            format!(
                "execute store result storage {namespace}:global {base}_i int 1 run data get storage {namespace}:global {base}_k"
            ),
            // A number becomes its text, which is a different value; text
            // stays the same, so the second `set` fails.
            format!(
                "data modify storage {namespace}:global {base}_t set string storage {namespace}:global {base}_k"
            ),
            format!(
                "execute store success score {kind_score} temp run data modify storage {namespace}:global {base}_t set from storage {namespace}:global {base}_k"
            ),
        ]);
        self.storage_data_pack_mut().track_objective("temp");
        Ok(commands)
    }

    /// Commands that store the map key `expr` at `target_path`. A key must
    /// be text: a number is an error.
    fn dynamic_storage_key_arg_commands(
        &mut self,
        target_path: &str,
        expr: &Expression,
    ) -> Result<Vec<String>, TranspileError> {
        let namespace = self.storage_data_pack().namespace.clone();
        match expr {
            Expression::String(value) => {
                let escaped = escape_quoted(value);
                return Ok(vec![format!(
                    "data modify storage {}:global {} set value \"{}\"",
                    namespace, target_path, escaped
                )]);
            }
            Expression::Identifier(name) => {
                let tables = self.storage_symbols();
                if tables.compile_time_constants.contains_key(name) {
                    return Err(format!(
                        "Dynamic map key `{}` must be a string value, not a numeric const",
                        name
                    )
                    .into());
                }
                if let Some(source_path) = tables.variable_storage_paths.get(name) {
                    if tables
                        .variable_types
                        .get(name)
                        .is_some_and(|ty| !matches!(ty, CobbleType::String | CobbleType::Unknown))
                    {
                        return Err(
                            format!("Dynamic map key `{}` must be a string value", name).into()
                        );
                    }
                    return Ok(vec![format!(
                        "data modify storage {}:global {} set from storage {}:global {}",
                        namespace, target_path, namespace, source_path
                    )]);
                }
                if self.is_storage_current_param(name) {
                    return Ok(vec![format!(
                        "data modify storage {}:global {} set from storage {}:global args.{}",
                        namespace, target_path, namespace, name
                    )]);
                }
                return Err(format!(
                    "Dynamic map key `{}` must be a string variable or function parameter",
                    name
                )
                .into());
            }
            _ => {}
        }
        if self.storage_key_type(expr) == CobbleType::Integer {
            return Err(
                "A map key must be text, and this key is a number. Use a string, such as `str(n)`, for a map key."
                    .into(),
            );
        }
        self.storage_leaf_set_commands(target_path, expr)
    }

    /// The storage path of a variable or parameter whose type Cobble
    /// doesn't know, which can hold a list or a string.
    fn unknown_value_path(&self, expr: &Expression) -> Option<String> {
        let Expression::Identifier(name) = expr else {
            return None;
        };
        if self.is_storage_current_param(name) {
            return Some(format!("args.{name}"));
        }
        let symbols = self.storage_symbols();
        let known = matches!(
            symbols.variable_types.get(name),
            Some(CobbleType::String | CobbleType::List | CobbleType::Map)
        );
        if known {
            return None;
        }
        symbols.variable_storage_paths.get(name).cloned()
    }

    /// Whether `base[index]` always picks a map field, rather than an item
    /// or a character.
    fn picks_field(&self, base: &Expression, index: &Expression) -> bool {
        self.subscript_kind(base, index) == SubscriptKind::Key
    }

    /// Commands that copy the value at `plan` to `target_path`.
    fn copy_storage_commands(
        &mut self,
        target_path: &str,
        plan: &StoragePathPlan,
    ) -> Result<Vec<String>, TranspileError> {
        let namespace = self.storage_data_pack().namespace.clone();
        self.path_commands(plan, |path| {
            format!(
                "data modify storage {namespace}:global {target_path} set from storage {namespace}:global {path}"
            )
        })
    }

    /// Commands that run `command(path)` on the path `plan` names, through a
    /// macro helper when part of the path is known only at run time.
    fn path_commands(
        &mut self,
        plan: &StoragePathPlan,
        command: impl Fn(&str) -> String,
    ) -> Result<Vec<String>, TranspileError> {
        if let Some(path) = plan.as_static() {
            return Ok(vec![command(path)]);
        }
        let namespace = self.storage_data_pack().namespace.clone();
        let id = self.storage_next_temp_id();
        let (mut commands, args_path) = self.prepare_dynamic_storage_path(id, plan)?;
        let helper = format!("_cobble_storage_dyn_{id}");
        let lines = plan.helper_lines(id, |path| vec![format!("${}", command(path))]);
        self.add_generated_helper_function(
            helper.clone(),
            lines,
            GeneratedCommandKind::ControlFlow,
        );
        commands.push(format!(
            "function {namespace}:{helper} with storage {namespace}:global {args_path}"
        ));
        commands.push(format!(
            "data remove storage {namespace}:global {args_path}"
        ));
        Ok(commands)
    }

    /// Commands that store `base[index]` at `target_path`, where Cobble
    /// doesn't know whether the value at `base_path` is a list or a string.
    /// The commands check it when they run: a list gives its item and a
    /// string its character, counted from the end for a negative index. An
    /// index that may also be text, such as a parameter, is checked too:
    /// text picks a field of a map.
    fn unknown_item_commands(
        &mut self,
        target_path: &str,
        base_path: &str,
        index: &Expression,
    ) -> Result<Vec<String>, TranspileError> {
        let namespace = self.storage_data_pack().namespace.clone();
        let id = self.storage_next_temp_id();
        let args_path = format!("_cobble_item_{id}");
        let kind = format!("#item_kind_{id}");
        let start = format!("#item_start_{id}");
        let end = format!("#item_end_{id}");
        let helper = format!("_cobble_item_{id}");
        let mut commands = self.storage_eval_to_target(index, &start)?;
        commands.extend([
            format!(
                "execute store result storage {namespace}:global {args_path}.index int 1 run scoreboard players get {start} temp"
            ),
            format!(
                "execute store result score {end} temp run data get storage {namespace}:global {base_path}"
            ),
            format!(
                "execute if score {start} temp matches ..-1 run scoreboard players operation {start} temp += {end} temp"
            ),
            format!("scoreboard players operation {end} temp = {start} temp"),
            format!("scoreboard players add {end} temp 1"),
            format!(
                "execute store result storage {namespace}:global {args_path}.start int 1 run scoreboard players get {start} temp"
            ),
            format!(
                "execute store result storage {namespace}:global {args_path}.end int 1 run scoreboard players get {end} temp"
            ),
            // `set` fails when nothing changes, which only happens for a
            // string: its text is the value itself.
            format!(
                "data modify storage {namespace}:global {args_path}.text set string storage {namespace}:global {base_path}"
            ),
            format!(
                "execute store success score {kind} temp run data modify storage {namespace}:global {args_path}.text set from storage {namespace}:global {base_path}"
            ),
        ]);
        let mut lines = vec![
            format!(
                "$execute if score {kind} temp matches 1 run data modify storage {namespace}:global {target_path} set from storage {namespace}:global {base_path}[$(index)]"
            ),
            format!(
                "$execute if score {kind} temp matches 0 run data modify storage {namespace}:global {target_path} set string storage {namespace}:global {base_path} $(start) $(end)"
            ),
        ];
        if self.storage_key_type(index) != CobbleType::Integer {
            let key_kind = format!("#item_key_{id}");
            commands.extend(self.runtime_key_commands(
                &format!("{args_path}.key"),
                index,
                &key_kind,
            )?);
            for line in &mut lines {
                *line = line.replacen(
                    "$execute ",
                    &format!("$execute if score {key_kind} temp matches 1 "),
                    1,
                );
            }
            lines.push(format!(
                "$execute if score {key_kind} temp matches 0 run data modify storage {namespace}:global {target_path} set from storage {namespace}:global {base_path}.\"$(key_k)\""
            ));
        }
        commands.extend([
            format!("function {namespace}:{helper} with storage {namespace}:global {args_path}"),
            format!("data remove storage {namespace}:global {args_path}"),
        ]);
        self.add_generated_helper_function(helper, lines, GeneratedCommandKind::ControlFlow);
        self.storage_data_pack_mut().track_objective("temp");
        Ok(commands)
    }

    /// The storage path of `expr` when it names a string variable. A
    /// parameter's type is unknown, so it is not one.
    fn string_variable_path(&self, expr: &Expression) -> Option<String> {
        let Expression::Identifier(name) = expr else {
            return None;
        };
        let symbols = self.storage_symbols();
        if symbols.variable_types.get(name) != Some(&CobbleType::String) {
            return None;
        }
        symbols.variable_storage_paths.get(name).cloned()
    }

    /// Whether `base[i]` picks a character: `base` is a string variable or a
    /// string literal, which an unrolled loop over strings leaves.
    fn is_string_base(&self, base: &Expression) -> bool {
        matches!(base, Expression::String(_)) || self.string_variable_path(base).is_some()
    }

    /// Commands that store the character of the string `base` that `index`
    /// picks, as a one-character string at `target_path`. A negative index
    /// counts from the end, as in Python.
    fn string_char_commands(
        &mut self,
        target_path: &str,
        base: &Expression,
        index: &Expression,
    ) -> Result<Vec<String>, TranspileError> {
        let namespace = self.storage_data_pack().namespace.clone();
        let Expression::String(text) = base else {
            let base_path = self
                .string_variable_path(base)
                .ok_or("Only a string can give a character.")?;
            return self.string_char_from_path(target_path, &base_path, index);
        };
        if let Some(index) = static_integer(self.storage_symbols(), index) {
            let chars: Vec<char> = text.chars().collect();
            let at = if index < 0 {
                chars.len() as i64 + i64::from(index)
            } else {
                i64::from(index)
            };
            let Some(character) = usize::try_from(at).ok().and_then(|at| chars.get(at)) else {
                return Err(format!("String index {index} is out of range for \"{text}\".").into());
            };
            return Ok(vec![format!(
                "data modify storage {namespace}:global {target_path} set value \"{}\"",
                escape_quoted(&character.to_string())
            )]);
        }
        let path = format!("_cobble_text_{}", self.storage_next_temp_id());
        let mut commands = vec![format!(
            "data modify storage {namespace}:global {path} set value \"{}\"",
            escape_quoted(text)
        )];
        commands.extend(self.string_char_from_path(target_path, &path, index)?);
        commands.push(format!("data remove storage {namespace}:global {path}"));
        Ok(commands)
    }

    /// Commands that store the character of the string at `base_path` that
    /// `index` picks at `target_path`. An index out of range fails the
    /// command and leaves the target as it was.
    fn string_char_from_path(
        &mut self,
        target_path: &str,
        base_path: &str,
        index: &Expression,
    ) -> Result<Vec<String>, TranspileError> {
        let namespace = self.storage_data_pack().namespace.clone();
        let set_string = format!(
            "data modify storage {namespace}:global {target_path} set string storage {namespace}:global {base_path}"
        );
        if let Some(index) = static_integer(self.storage_symbols(), index) {
            // `-1` has no end that counts from the end, so it runs to the end.
            return Ok(vec![if index == -1 {
                format!("{set_string} -1")
            } else {
                format!("{set_string} {index} {}", index + 1)
            }]);
        }

        let id = self.storage_next_temp_id();
        let start = format!("#char_start_{id}");
        let end = format!("#char_end_{id}");
        let args_path = format!("_cobble_char_{id}");
        let helper = format!("_cobble_char_{id}");
        let mut commands = self.storage_eval_to_target(index, &start)?;
        commands.extend([
            format!(
                "execute store result score {end} temp run data get storage {namespace}:global {base_path}"
            ),
            format!(
                "execute if score {start} temp matches ..-1 run scoreboard players operation {start} temp += {end} temp"
            ),
            format!("scoreboard players operation {end} temp = {start} temp"),
            format!("scoreboard players add {end} temp 1"),
            format!(
                "execute store result storage {namespace}:global {args_path}.start int 1 run scoreboard players get {start} temp"
            ),
            format!(
                "execute store result storage {namespace}:global {args_path}.end int 1 run scoreboard players get {end} temp"
            ),
            format!("function {namespace}:{helper} with storage {namespace}:global {args_path}"),
            format!("data remove storage {namespace}:global {args_path}"),
        ]);
        self.add_generated_helper_function(
            helper,
            vec![format!("${set_string} $(start) $(end)")],
            GeneratedCommandKind::ControlFlow,
        );
        self.storage_data_pack_mut().track_objective("temp");
        Ok(commands)
    }

    fn storage_value_commands(
        &mut self,
        target_path: &str,
        expr: &Expression,
    ) -> Result<Vec<String>, TranspileError> {
        if let Ok(snbt) = self.storage_static_snbt(expr) {
            return Ok(vec![format!(
                "data modify storage {}:global {} set value {}",
                self.storage_data_pack().namespace,
                target_path,
                snbt
            )]);
        }

        match expr {
            Expression::Array(_) | Expression::Map(_) => {
                let shell = self.storage_shell_snbt(expr)?;
                let mut commands = vec![format!(
                    "data modify storage {}:global {} set value {}",
                    self.storage_data_pack().namespace,
                    target_path,
                    shell
                )];
                commands.extend(self.storage_runtime_patch_commands(target_path, expr)?);
                Ok(commands)
            }
            _ => self.storage_leaf_set_commands(target_path, expr),
        }
    }

    fn materialize_storage_value(
        &mut self,
        expr: &Expression,
    ) -> Result<(Vec<String>, String), TranspileError> {
        let id = self.storage_next_temp_id();
        let path = format!("_cobble_value_{}", id);
        let commands = self.storage_value_commands(&path, expr)?;
        Ok((commands, path))
    }

    fn storage_shell_snbt(&self, expr: &Expression) -> Result<String, TranspileError> {
        match expr {
            Expression::Number(_)
            | Expression::String(_)
            | Expression::Boolean(_)
            | Expression::None => self.storage_static_snbt(expr),
            Expression::Identifier(name) => {
                if self
                    .storage_symbols()
                    .compile_time_constants
                    .contains_key(name)
                {
                    self.storage_static_snbt(expr)
                } else {
                    Ok(self.default_snbt_for_runtime_identifier(name))
                }
            }
            Expression::Array(items) => {
                let mut result = String::from("[");
                for (index, item) in items.iter().enumerate() {
                    if index > 0 {
                        result.push(',');
                    }
                    result.push_str(&self.storage_shell_snbt(item)?);
                }
                result.push(']');
                Ok(result)
            }
            Expression::Map(entries) => {
                let mut result = String::from("{");
                for (index, (key, value)) in entries.iter().enumerate() {
                    if index > 0 {
                        result.push(',');
                    }
                    result.push_str(&Transpiler::serialize_snbt_key(key));
                    result.push(':');
                    result.push_str(&self.storage_shell_snbt(value)?);
                }
                result.push('}');
                Ok(result)
            }
            _ => Ok("0".to_string()),
        }
    }

    fn default_snbt_for_runtime_identifier(&self, name: &str) -> String {
        match self.storage_symbols().variable_types.get(name) {
            Some(CobbleType::String) => "\"\"".to_string(),
            Some(CobbleType::List) => "[]".to_string(),
            Some(CobbleType::Map) => "{}".to_string(),
            Some(CobbleType::Boolean) => "0b".to_string(),
            _ => "0".to_string(),
        }
    }

    fn storage_runtime_patch_commands(
        &mut self,
        target_path: &str,
        expr: &Expression,
    ) -> Result<Vec<String>, TranspileError> {
        match expr {
            Expression::Array(items) => {
                let mut commands = Vec::new();
                for (index, item) in items.iter().enumerate() {
                    commands.extend(self.storage_runtime_patch_commands(
                        &format!("{}[{}]", target_path, index),
                        item,
                    )?);
                }
                Ok(commands)
            }
            Expression::Map(entries) => {
                let mut commands = Vec::new();
                for (key, value) in entries {
                    commands.extend(self.storage_runtime_patch_commands(
                        &storage_child_path(target_path, key),
                        value,
                    )?);
                }
                Ok(commands)
            }
            Expression::Number(_)
            | Expression::String(_)
            | Expression::Boolean(_)
            | Expression::None => Ok(Vec::new()),
            Expression::Identifier(name)
                if self
                    .storage_symbols()
                    .compile_time_constants
                    .contains_key(name) =>
            {
                Ok(Vec::new())
            }
            _ => self.storage_leaf_set_commands(target_path, expr),
        }
    }

    fn storage_leaf_set_commands(
        &mut self,
        target_path: &str,
        expr: &Expression,
    ) -> Result<Vec<String>, TranspileError> {
        let namespace = self.storage_data_pack().namespace.clone();
        match expr {
            Expression::Identifier(name) => {
                if let Some(source_path) = self.storage_symbols().variable_storage_paths.get(name) {
                    return Ok(vec![format!(
                        "data modify storage {}:global {} set from storage {}:global {}",
                        namespace, target_path, namespace, source_path
                    )]);
                }
                if self.is_storage_macro_param(name) {
                    return Ok(vec![format!(
                        "data modify storage {}:global {} set from storage {}:global args.{}",
                        namespace, target_path, namespace, name
                    )]);
                }
                if let Some(objective) = self.storage_scoreboard_objective(name) {
                    return Ok(vec![format!(
                        "execute store result storage {}:global {} int 1 run scoreboard players get {} {}",
                        namespace, target_path, name, objective
                    )]);
                }
                Err(format!(
                    "Identifier '{}' cannot be stored in SNBT here; use a literal, const, runtime score, storage-backed value, or function parameter",
                    name
                ).into())
            }
            Expression::Number(_)
            | Expression::String(_)
            | Expression::Boolean(_)
            | Expression::Array(_)
            | Expression::Map(_) => self.storage_value_commands(target_path, expr),
            Expression::None => self.storage_static_snbt(expr).map(|_| Vec::new()),
            // A character of a string, and an item or field that isn't a
            // number, are copied in storage.
            Expression::Subscript(base, index) if self.is_string_base(base) => {
                self.string_char_commands(target_path, base, index)
            }
            Expression::Subscript(base, index)
                if !self.picks_field(base, index) && self.unknown_value_path(base).is_some() =>
            {
                let base_path = self.unknown_value_path(base).unwrap_or_default();
                self.unknown_item_commands(target_path, &base_path, index)
            }
            Expression::Subscript(..) | Expression::Attribute(..)
                if self.storage_read_type(expr) != CobbleType::Integer
                    && self.resolve_storage_path_plan(expr).is_some() =>
            {
                let plan = self.resolve_storage_path_plan(expr).expect("checked above");
                self.copy_storage_commands(target_path, &plan)
            }
            _ => {
                let id = self.storage_next_temp_id();
                let temp_score = format!("#cobble_value_{}", id);
                let mut commands = self.storage_eval_to_target(expr, &temp_score)?;
                commands.push(format!(
                    "execute store result storage {}:global {} int 1 run scoreboard players get {} temp",
                    namespace, target_path, temp_score
                ));
                Ok(commands)
            }
        }
    }
}

fn storage_child_path(base: &str, key: &str) -> String {
    if !key.is_empty()
        && key
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-' || c == '+')
    {
        format!("{}.{}", base, key)
    } else {
        let escaped = escape_quoted(key);
        format!("{}.\"{}\"", base, escaped)
    }
}

impl StorageValueOps for Transpiler {
    fn storage_next_temp_id(&mut self) -> u32 {
        self.next_temp_id()
    }

    fn is_storage_macro_param(&self, name: &str) -> bool {
        self.is_current_macro_param(name)
    }

    fn storage_scoreboard_objective(&self, name: &str) -> Option<&str> {
        self.symbols.scoreboard_objective(name)
    }

    fn storage_static_snbt(&self, expr: &Expression) -> Result<String, TranspileError> {
        self.serialize_to_snbt(expr)
    }
}

impl StoragePathOps for Transpiler {
    fn storage_symbols(&self) -> &Symbols {
        &self.symbols
    }

    fn storage_current_params(&self) -> &[String] {
        &self.current_context.params
    }

    fn storage_data_pack(&self) -> &DataPack {
        &self.data_pack
    }

    fn storage_data_pack_mut(&mut self) -> &mut DataPack {
        &mut self.data_pack
    }

    fn storage_helper_source(&self) -> Option<SourceLocation> {
        self.current_statement_source.clone()
    }

    fn storage_eval_to_target(
        &mut self,
        expr: &Expression,
        target: &str,
    ) -> Result<Vec<String>, TranspileError> {
        self.evaluate_expression_to_target(expr, target)
    }
}

impl<'a> StorageValueOps for ExpressionEvaluator<'a> {
    fn storage_next_temp_id(&mut self) -> u32 {
        self.next_temp_id()
    }

    fn is_storage_macro_param(&self, name: &str) -> bool {
        self.is_storage_current_param(name)
    }

    fn storage_scoreboard_objective(&self, name: &str) -> Option<&str> {
        self.symbols.scoreboard_objective(name)
    }

    fn storage_static_snbt(&self, expr: &Expression) -> Result<String, TranspileError> {
        serialize_snbt(self.symbols, expr)
    }
}

impl<'a> StoragePathOps for ExpressionEvaluator<'a> {
    fn storage_symbols(&self) -> &Symbols {
        self.symbols
    }

    fn storage_current_params(&self) -> &[String] {
        self.current_params
    }

    fn storage_data_pack(&self) -> &DataPack {
        &*self.data_pack
    }

    fn storage_data_pack_mut(&mut self) -> &mut DataPack {
        self.data_pack
    }

    fn storage_helper_source(&self) -> Option<SourceLocation> {
        self.source.clone()
    }

    fn storage_eval_to_target(
        &mut self,
        expr: &Expression,
        target: &str,
    ) -> Result<Vec<String>, TranspileError> {
        self.evaluate_expression_to_target(expr, target)
    }
}

/// The integer `expr` always is: a literal, a negated literal, or a
/// constant.
fn static_integer(symbols: &Symbols, expr: &Expression) -> Option<i32> {
    match expr {
        Expression::Number(value) if value.fract() == 0.0 => Some(*value as i32),
        Expression::Unary(crate::ast::UnaryOp::Neg, inner) => {
            static_integer(symbols, inner).map(|value| -value)
        }
        Expression::Identifier(name) => symbols
            .compile_time_constants
            .get(name)
            .filter(|value| value.fract() == 0.0)
            .map(|value| *value as i32),
        _ => None,
    }
}

#[derive(Debug, Clone)]
pub(in crate::transpiler) struct DynamicStoragePathArg {
    pub(in crate::transpiler) placeholder: String,
    pub(in crate::transpiler) expr: Expression,
    pub(in crate::transpiler) kind: DynamicStoragePathArgKind,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(in crate::transpiler) enum DynamicStoragePathArgKind {
    Index,
    Key,
    /// A key that picks an item when the pack finds a number in it, and a
    /// field when it finds text.
    Runtime,
}

/// What a subscript picks; see [`StoragePathOps::subscript_kind`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(in crate::transpiler) enum SubscriptKind {
    /// An item of a list, or a character of a string.
    Index,
    /// A field of a map.
    Key,
    /// An item for a number and a field for text, decided when the pack
    /// runs.
    Runtime,
}

/// How many keys of one path the running pack can decide; each one doubles
/// the lines of the path's helper.
const MAX_RUNTIME_KEYS: usize = 3;

/// The score that holds 1 when the key behind `placeholder` is a number.
fn runtime_kind_score(id: u32, placeholder: &str) -> String {
    format!("#cobble_dyn_{id}_{placeholder}")
}

/// Where a key decided at run time goes in a path template.
fn runtime_marker(placeholder: &str) -> String {
    format!("\u{1}{placeholder}\u{1}")
}

#[derive(Debug, Clone)]
pub(in crate::transpiler) enum StoragePathPlan {
    Static(String),
    Dynamic {
        template: String,
        args: Vec<DynamicStoragePathArg>,
    },
}

impl StoragePathPlan {
    pub(in crate::transpiler) fn static_path(path: impl Into<String>) -> Self {
        Self::Static(path.into())
    }

    pub(in crate::transpiler) fn as_static(&self) -> Option<&str> {
        match self {
            Self::Static(path) => Some(path),
            Self::Dynamic { .. } => None,
        }
    }

    /// The lines of a macro helper that runs on this path, prepared with
    /// `id` by `prepare_dynamic_storage_path`. `build(path)` gives the lines
    /// for one path text, and its lines that start with `$` use it. A path
    /// with keys the running pack decides has a text for each choice, so each
    /// such line is repeated once per text, guarded by the scores that pick
    /// it.
    pub(in crate::transpiler) fn helper_lines(
        &self,
        id: u32,
        build: impl Fn(&str) -> Vec<String>,
    ) -> Vec<String> {
        let (template, args) = match self {
            Self::Static(path) => return build(path),
            Self::Dynamic { template, args } => (template, args),
        };
        let runtime: Vec<&str> = args
            .iter()
            .filter(|arg| arg.kind == DynamicStoragePathArgKind::Runtime)
            .map(|arg| arg.placeholder.as_str())
            .collect();
        if runtime.is_empty() {
            return build(template);
        }
        let choices: Vec<(String, Vec<String>)> = (0..1usize << runtime.len())
            .map(|choice| {
                let mut path = template.clone();
                let mut guard = String::new();
                for (bit, placeholder) in runtime.iter().enumerate() {
                    let index = choice & (1 << bit) != 0;
                    let text = if index {
                        format!("[$({placeholder}_i)]")
                    } else {
                        format!(".\"$({placeholder}_k)\"")
                    };
                    path = path.replace(&runtime_marker(placeholder), &text);
                    guard.push_str(&format!(
                        "if score {} temp matches {} ",
                        runtime_kind_score(id, placeholder),
                        u8::from(index)
                    ));
                }
                (guard, build(&path))
            })
            .collect();
        let mut lines = Vec::new();
        for (position, line) in choices[0].1.iter().enumerate() {
            if !line.starts_with('$') {
                lines.push(line.clone());
                continue;
            }
            for (guard, choice_lines) in &choices {
                let command = &choice_lines[position][1..];
                lines.push(match command.strip_prefix("execute ") {
                    Some(rest) => format!("$execute {guard}{rest}"),
                    None => format!("$execute {guard}run {command}"),
                });
            }
        }
        lines
    }

    pub(in crate::transpiler) fn dynamic_args(&self) -> &[DynamicStoragePathArg] {
        match self {
            Self::Static(_) => &[],
            Self::Dynamic { args, .. } => args,
        }
    }

    pub(in crate::transpiler) fn append_field(self, field: &str) -> Self {
        match self {
            Self::Static(path) => Self::Static(format!("{}.{}", path, field)),
            Self::Dynamic { template, args } => Self::Dynamic {
                template: format!("{}.{}", template, field),
                args,
            },
        }
    }

    /// Appends a map key, quoted when it isn't a plain name.
    pub(in crate::transpiler) fn append_key(self, key: &str) -> Self {
        match self {
            Self::Static(path) => Self::Static(storage_child_path(&path, key)),
            Self::Dynamic { template, args } => Self::Dynamic {
                template: storage_child_path(&template, key),
                args,
            },
        }
    }

    pub(in crate::transpiler) fn append_static_index(self, index: i32) -> Self {
        match self {
            Self::Static(path) => Self::Static(format!("{}[{}]", path, index)),
            Self::Dynamic { template, args } => Self::Dynamic {
                template: format!("{}[{}]", template, index),
                args,
            },
        }
    }

    pub(in crate::transpiler) fn append_dynamic_index(self, expr: Expression) -> Self {
        let (template, mut args) = match self {
            Self::Static(path) => (path, Vec::new()),
            Self::Dynamic { template, args } => (template, args),
        };
        let placeholder = format!("idx{}", args.len());
        args.push(DynamicStoragePathArg {
            placeholder: placeholder.clone(),
            expr,
            kind: DynamicStoragePathArgKind::Index,
        });
        Self::Dynamic {
            template: format!("{}[$({})]", template, placeholder),
            args,
        }
    }

    /// Appends a key that picks an item or a field by what the running pack
    /// finds in it.
    pub(in crate::transpiler) fn append_runtime_key(self, expr: Expression) -> Self {
        let (template, mut args) = match self {
            Self::Static(path) => (path, Vec::new()),
            Self::Dynamic { template, args } => (template, args),
        };
        let placeholder = format!("seg{}", args.len());
        args.push(DynamicStoragePathArg {
            placeholder: placeholder.clone(),
            expr,
            kind: DynamicStoragePathArgKind::Runtime,
        });
        Self::Dynamic {
            template: format!("{}{}", template, runtime_marker(&placeholder)),
            args,
        }
    }

    pub(in crate::transpiler) fn append_dynamic_key(self, expr: Expression) -> Self {
        let (template, mut args) = match self {
            Self::Static(path) => (path, Vec::new()),
            Self::Dynamic { template, args } => (template, args),
        };
        let placeholder = format!("key{}", args.len());
        args.push(DynamicStoragePathArg {
            placeholder: placeholder.clone(),
            expr,
            kind: DynamicStoragePathArgKind::Key,
        });
        Self::Dynamic {
            // The value may hold spaces, dots, and colons, as in `prices[id]`.
            template: format!("{}.\"$({})\"", template, placeholder),
            args,
        }
    }
}
