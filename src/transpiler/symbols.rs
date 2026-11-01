use crate::ast::{CobbleType, EntityDef, Expression};
use crate::transpiler::container_writes::{ContainerWrites, RootValues};
use crate::transpiler::scoped::{Mark, ScopedMap, ScopedSet};
use crate::transpiler::{FunctionReturnKind, SourceLocation};
use std::collections::{BTreeSet, HashMap, HashSet};

/// What each name in the program refers to. Variables are module-wide
/// scores or storage paths; a function restores parts of this after its
/// body (see `process_function_def`). The tables a body can change undo
/// their changes back to a mark (see [`Symbols::open_scope`]) instead of
/// being copied.
#[derive(Clone, Default)]
pub(in crate::transpiler) struct Symbols {
    /// The last value assigned to each variable.
    pub(in crate::transpiler) variables: ScopedMap<String, Expression>,
    /// The objective that holds each score variable.
    pub(in crate::transpiler) variable_objectives: ScopedMap<String, String>,
    /// Variables backed by a score.
    pub(in crate::transpiler) scoreboard_variables: ScopedSet<String>,
    /// Storage path of each list, map, or string variable, such as
    /// `vars.items` for `items`.
    pub(in crate::transpiler) variable_storage_paths: ScopedMap<String, String>,
    pub(in crate::transpiler) variable_types: ScopedMap<String, CobbleType>,
    pub(in crate::transpiler) compile_time_constants: ScopedMap<String, f64>,
    /// Names declared `global` in the current function.
    pub(in crate::transpiler) global_variables: ScopedSet<String>,
    /// `@Name = @a[...]` definitions.
    pub(in crate::transpiler) selector_aliases: ScopedMap<String, String>,
    pub(in crate::transpiler) entity_templates: ScopedMap<String, EntityDef>,
    /// Names assigned at module level in any compiled file. A function
    /// assigning one of them without `global` gets a local variable instead.
    pub(in crate::transpiler) module_variables: BTreeSet<String>,
    /// Module variables recorded ahead of their module-level assignment, so
    /// function bodies above it can use them. Module-level code that runs
    /// before the assignment can't.
    pub(in crate::transpiler) pending_module_declarations: ScopedSet<String>,
    /// Module variables whose type only their module-level assignment can
    /// decide, before that assignment is lowered.
    pub(in crate::transpiler) late_module_declarations: ScopedSet<String>,
    /// What the program writes into lists and maps, collected before any
    /// statement is lowered (see `container_writes.rs`).
    pub(in crate::transpiler) container_writes: ContainerWrites,
    /// What the program assigns to each name as a whole, collected with
    /// `container_writes`.
    pub(in crate::transpiler) root_values: RootValues,
    /// Minecraft conditions of a mixed condition (`entity @a and f()`),
    /// keyed by the identifier that stands for each while the rest is
    /// translated as Python (see `mask_minecraft_conditions`).
    pub(in crate::transpiler) minecraft_conditions: HashMap<String, String>,
}

/// The marks of a scope opened by [`Symbols::open_scope`].
pub(in crate::transpiler) struct SymbolsMark {
    variables: Mark,
    variable_objectives: Mark,
    scoreboard_variables: Mark,
    variable_storage_paths: Mark,
    variable_types: Mark,
    compile_time_constants: Mark,
    global_variables: Mark,
    selector_aliases: Mark,
    entity_templates: Mark,
    pending_module_declarations: Mark,
    late_module_declarations: Mark,
}

impl Symbols {
    /// Opens a scope over the tables a body can change. `module_variables`
    /// and `container_writes` are filled in before any statement is
    /// lowered, and `minecraft_conditions` is restored where it is set.
    pub(in crate::transpiler) fn open_scope(&mut self) -> SymbolsMark {
        SymbolsMark {
            variables: self.variables.open(),
            variable_objectives: self.variable_objectives.open(),
            scoreboard_variables: self.scoreboard_variables.open(),
            variable_storage_paths: self.variable_storage_paths.open(),
            variable_types: self.variable_types.open(),
            compile_time_constants: self.compile_time_constants.open(),
            global_variables: self.global_variables.open(),
            selector_aliases: self.selector_aliases.open(),
            entity_templates: self.entity_templates.open(),
            pending_module_declarations: self.pending_module_declarations.open(),
            late_module_declarations: self.late_module_declarations.open(),
        }
    }

    /// Undoes every change since `mark`.
    pub(in crate::transpiler) fn undo_scope(&mut self, mark: SymbolsMark) {
        self.undo_variables(&mark);
        self.compile_time_constants
            .undo_to(mark.compile_time_constants);
        self.selector_aliases.undo_to(mark.selector_aliases);
        self.entity_templates.undo_to(mark.entity_templates);
    }

    /// Undoes the variables a function body assigned since `mark`, and
    /// keeps the constants, selector aliases, and entity templates it
    /// declared.
    pub(in crate::transpiler) fn end_function_scope(&mut self, mark: SymbolsMark) {
        self.undo_variables(&mark);
        self.compile_time_constants
            .keep_from(mark.compile_time_constants);
        self.selector_aliases.keep_from(mark.selector_aliases);
        self.entity_templates.keep_from(mark.entity_templates);
    }

    /// Keeps every change since `mark`, as when lowering stopped with an
    /// error.
    pub(in crate::transpiler) fn keep_scope(&mut self, mark: SymbolsMark) {
        self.variables.keep_from(mark.variables);
        self.variable_objectives.keep_from(mark.variable_objectives);
        self.scoreboard_variables
            .keep_from(mark.scoreboard_variables);
        self.variable_storage_paths
            .keep_from(mark.variable_storage_paths);
        self.variable_types.keep_from(mark.variable_types);
        self.compile_time_constants
            .keep_from(mark.compile_time_constants);
        self.global_variables.keep_from(mark.global_variables);
        self.selector_aliases.keep_from(mark.selector_aliases);
        self.entity_templates.keep_from(mark.entity_templates);
        self.pending_module_declarations
            .keep_from(mark.pending_module_declarations);
        self.late_module_declarations
            .keep_from(mark.late_module_declarations);
    }

    fn undo_variables(&mut self, mark: &SymbolsMark) {
        self.variables.undo_to(mark.variables);
        self.variable_objectives.undo_to(mark.variable_objectives);
        self.scoreboard_variables.undo_to(mark.scoreboard_variables);
        self.variable_storage_paths
            .undo_to(mark.variable_storage_paths);
        self.variable_types.undo_to(mark.variable_types);
        self.global_variables.undo_to(mark.global_variables);
        self.pending_module_declarations
            .undo_to(mark.pending_module_declarations);
        self.late_module_declarations
            .undo_to(mark.late_module_declarations);
    }

    /// The objective of the score behind `name`, when a score holds it.
    pub(in crate::transpiler) fn scoreboard_objective(&self, name: &str) -> Option<&str> {
        if self.scoreboard_variables.contains(name)
            || self.variable_objectives.contains_key(name)
            || self.variables.contains_key(name)
        {
            Some(
                self.variable_objectives
                    .get(name)
                    .map(String::as_str)
                    .unwrap_or("temp"),
            )
        } else {
            None
        }
    }
}

/// User functions seen so far.
#[derive(Default)]
pub(in crate::transpiler) struct FunctionTable {
    /// Safe entries for calls from Cobble; public entries retain raw inline
    /// macro compatibility for messages written without a component.
    pub(in crate::transpiler) safe_text_variants: HashMap<String, String>,
    /// Parameter names of each function.
    pub(in crate::transpiler) params: HashMap<String, Vec<String>>,
    /// Whether each function can be used as a value.
    pub(in crate::transpiler) returns: HashMap<String, FunctionReturnKind>,
    /// `module.func` calls of imported modules, mapped to `func`.
    pub(in crate::transpiler) qualified_aliases: HashMap<String, String>,
    /// Functions that can call themselves, directly or through other
    /// functions. Their calls save state on storage stacks.
    pub(in crate::transpiler) recursive: HashSet<String>,
    /// The user function each generated function was lowered for, such as
    /// `main` for `if_temp_3`.
    pub(in crate::transpiler) owners: HashMap<String, String>,
    /// Parameters each function reads in an expression, as a number or a
    /// value, rather than only in command text.
    pub(in crate::transpiler) params_used_as_values: HashMap<String, HashSet<String>>,
    /// Parameters placed in integer arguments of raw Minecraft commands,
    /// also through calls that forward them unchanged.
    pub(in crate::transpiler) params_in_integer_commands: HashMap<String, HashSet<String>>,
    /// Where each user function is defined.
    pub(in crate::transpiler) def_sources: HashMap<String, Option<SourceLocation>>,
}

impl FunctionTable {
    pub(in crate::transpiler) fn call_target<'a>(&'a self, function: &'a str) -> &'a str {
        self.safe_text_variants
            .get(function)
            .map(String::as_str)
            .unwrap_or(function)
    }
    /// Normalize numeric NBT arguments before a raw integer macro sees
    /// them. A string such as `"15"` stays text, rather than becoming its
    /// length. The caller tracks the `temp` objective when this is nonempty.
    pub(in crate::transpiler) fn integer_argument_commands(
        &self,
        function: &str,
        param: &str,
        namespace: &str,
        path: &str,
        argument: &Expression,
    ) -> Vec<String> {
        // Literals were already converted (and warned) by number_argument;
        // a quoted literal must keep the author's exact macro text.
        if matches!(
            argument,
            Expression::Number(_) | Expression::String(_) | Expression::Boolean(_)
        ) {
            return Vec::new();
        }
        if !self
            .params_in_integer_commands
            .get(function)
            .is_some_and(|params| params.contains(param))
        {
            return Vec::new();
        }
        vec![
            format!("execute store success score #cobble_arg_numeric temp run data get storage {namespace}:global {path} 1"),
            format!("execute if score #cobble_arg_numeric temp matches 1 store result storage {namespace}:global {path} int 1 run data get storage {namespace}:global {path} 1"),
        ]
    }

    /// The SNBT a number argument passes to `param` of `function`: a
    /// fraction, as text, to a parameter used only in command text, so it
    /// arrives as written, and otherwise the integer a score holds. The
    /// second value tells whether the number loses its fraction or range.
    pub(in crate::transpiler) fn number_argument(
        &self,
        function: &str,
        param: &str,
        value: f64,
    ) -> (String, bool) {
        let text_only = self
            .params_used_as_values
            .get(function)
            .is_some_and(|used| !used.contains(param));
        if value.fract() != 0.0 && value.is_finite() && text_only {
            return (format!("\"{value}\""), false);
        }
        let exact =
            value.fract() == 0.0 && value >= f64::from(i32::MIN) && value <= f64::from(i32::MAX);
        ((value as i32).to_string(), !exact)
    }
}
