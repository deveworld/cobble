use crate::ast::{CobbleType, EntityDef, Expression};
use crate::transpiler::container_writes::ContainerWrites;
use crate::transpiler::{FunctionReturnKind, SourceLocation};
use std::collections::{BTreeSet, HashMap, HashSet};

/// What each name in the program refers to. Variables are module-wide
/// scores or storage paths; a function saves and restores parts of this
/// around its body (see `process_function_def`).
#[derive(Clone, Default)]
pub(in crate::transpiler) struct Symbols {
    /// The last value assigned to each variable.
    pub(in crate::transpiler) variables: HashMap<String, Expression>,
    /// The objective that holds each score variable.
    pub(in crate::transpiler) variable_objectives: HashMap<String, String>,
    /// Variables backed by a score.
    pub(in crate::transpiler) scoreboard_variables: HashSet<String>,
    /// Storage path of each list, map, or string variable, such as
    /// `vars.items` for `items`.
    pub(in crate::transpiler) variable_storage_paths: HashMap<String, String>,
    pub(in crate::transpiler) variable_types: HashMap<String, CobbleType>,
    pub(in crate::transpiler) compile_time_constants: HashMap<String, f64>,
    /// Names declared `global` in the current function.
    pub(in crate::transpiler) global_variables: HashSet<String>,
    /// `@Name = @a[...]` definitions.
    pub(in crate::transpiler) selector_aliases: HashMap<String, String>,
    pub(in crate::transpiler) entity_templates: HashMap<String, EntityDef>,
    /// Names assigned at module level in any compiled file. A function
    /// assigning one of them without `global` gets a local variable instead.
    pub(in crate::transpiler) module_variables: BTreeSet<String>,
    /// Module variables recorded ahead of their module-level assignment, so
    /// function bodies above it can use them. Module-level code that runs
    /// before the assignment can't.
    pub(in crate::transpiler) pending_module_declarations: HashSet<String>,
    /// Module variables whose type only their module-level assignment can
    /// decide, before that assignment is lowered.
    pub(in crate::transpiler) late_module_declarations: HashSet<String>,
    /// What the program writes into lists and maps, collected before any
    /// statement is lowered (see `container_writes.rs`).
    pub(in crate::transpiler) container_writes: ContainerWrites,
}

impl Symbols {
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
    /// Where each user function is defined.
    pub(in crate::transpiler) def_sources: HashMap<String, Option<SourceLocation>>,
}
