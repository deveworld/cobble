use crate::ast::{CobbleType, EntityDef, Expression};
use crate::transpiler::{FunctionReturnKind, TranspileError};
use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet};

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
    /// Caller -> callees edges for every lowered user-function call, used to
    /// reject mutually recursive call cycles at the end of `transpile()`.
    /// BTree containers keep the reported cycle deterministic.
    pub(in crate::transpiler) call_edges: BTreeMap<String, BTreeSet<String>>,
}

impl FunctionTable {
    /// Record that `caller` calls `callee`, rejecting a direct self-call at
    /// the call site. Module-level calls (no `caller`) record nothing.
    pub(in crate::transpiler) fn record_call(
        &mut self,
        caller: Option<&str>,
        callee: &str,
    ) -> Result<(), TranspileError> {
        let Some(caller) = caller else {
            return Ok(());
        };
        if caller == callee {
            return Err(format!(
                "Recursive call to '{}' is not supported. Rewrite the recursion as a loop (while/for) instead.",
                callee
            ).into());
        }
        self.call_edges
            .entry(caller.to_string())
            .or_default()
            .insert(callee.to_string());
        Ok(())
    }
}
