use std::collections::HashMap;

/// Canonical registry of stdlib module names that can be activated via
/// `from stdlib import <name>` (or all at once via `import stdlib`).
///
/// Single source of truth shared by:
/// - the transpiler's import handling (`ALL_STDLIB_MODULES` in
///   `src/transpiler/mod.rs` is a re-export of this table);
/// - the diagnostics scanner's `is_known_module_call` / `is_builtin_symbol`
///   in `src/diagnostics.rs`, which layer documented per-site deltas on top
///   of this base (see the constants next to those functions).
///
/// The web/wasm crate includes this file via `#[path]`, so both worlds see
/// the same list.
pub const STDLIB_MODULES: &[&str] = &[
    "text",
    "score",
    "score.objective",
    "random",
    "timer",
    "storage",
    "item_component",
    "schedule",
    "selector",
    "position",
    "bossbar",
    "team",
    "entity",
    "math",
    "event",
    "datapack",
    "resource_pack",
];

/// Standard library module for Cobble
/// Provides runtime support for event handling and built-in functions
pub struct StdLib {
    event_handlers: HashMap<EventType, Vec<String>>,
}

#[derive(Debug, Clone, Hash, PartialEq, Eq)]
pub enum EventType {
    Load, // When datapack is loaded (reset)
    Tick, // Every game tick
    Custom(String),
}

impl Default for StdLib {
    fn default() -> Self {
        Self::new()
    }
}

impl StdLib {
    pub fn new() -> Self {
        Self {
            event_handlers: HashMap::new(),
        }
    }

    /// Register an event handler function
    pub fn add_event_listener(&mut self, event_type: EventType, function_name: String) {
        self.event_handlers
            .entry(event_type)
            .or_default()
            .push(function_name);
    }

    /// Get all handlers for a specific event
    pub fn get_event_handlers(&self, event_type: &EventType) -> Vec<String> {
        self.event_handlers
            .get(event_type)
            .cloned()
            .unwrap_or_default()
    }

    /// Generate the minecraft function tags for load and tick events
    pub fn generate_tags(&self, namespace: &str) -> HashMap<String, Vec<String>> {
        let mut tags = HashMap::new();

        // Load tag (minecraft:load)
        if let Some(handlers) = self.event_handlers.get(&EventType::Load) {
            let load_functions: Vec<String> = handlers
                .iter()
                .map(|f| format!("{}:{}", namespace, f))
                .collect();
            tags.insert("minecraft:load".to_string(), load_functions);
        }

        // Tick tag (minecraft:tick)
        if let Some(handlers) = self.event_handlers.get(&EventType::Tick) {
            let tick_functions: Vec<String> = handlers
                .iter()
                .map(|f| format!("{}:{}", namespace, f))
                .collect();
            tags.insert("minecraft:tick".to_string(), tick_functions);
        }

        tags
    }
}

#[cfg(test)]
mod registry_tests {
    use super::STDLIB_MODULES;
    use crate::diagnostics::{
        is_builtin_symbol, is_known_module_call, NON_MODULE_BUILTIN_SYMBOLS, STDLIB_MODULE_METHODS,
    };

    /// The three views of the stdlib registry (transpiler import table,
    /// diagnostics module-call table, diagnostics builtin-symbol set) must
    /// agree on the shared base, with only the documented per-site deltas.
    #[test]
    fn stdlib_registry_views_agree_on_shared_base() {
        // Transpiler view: a re-export of the canonical table, so it cannot
        // drift.
        assert_eq!(crate::transpiler::ALL_STDLIB_MODULES, STDLIB_MODULES);

        // Diagnostics module-call view.
        for module in STDLIB_MODULES {
            match *module {
                // Documented deltas: "event" has no callable methods
                // (`event.LOAD`/`event.TICK` are attributes) and "datapack"
                // is validated through its resource tables inside
                // `is_known_module_call` instead of a static method list.
                "event" | "datapack" => {
                    assert!(
                        !STDLIB_MODULE_METHODS.iter().any(|(name, _)| name == module),
                        "{module} should not have a static method table"
                    );
                }
                _ => {
                    let (_, methods) = STDLIB_MODULE_METHODS
                        .iter()
                        .find(|(name, _)| name == module)
                        .unwrap_or_else(|| panic!("no method table for {module}"));
                    for method in *methods {
                        assert!(is_known_module_call(module, method));
                    }
                }
            }
        }
        // "datapack" is still recognized through its resource tables.
        assert!(is_known_module_call("datapack", "item_modifier"));
        // Every method-table key is a base module or the documented "stdlib"
        // pseudo-module addition.
        for (name, _) in STDLIB_MODULE_METHODS {
            assert!(
                *name == "stdlib" || STDLIB_MODULES.contains(name),
                "unexpected module {name}"
            );
        }
        assert!(!is_known_module_call("not_a_module", "set"));

        // Diagnostics builtin-symbol view: every non-dotted base module is a
        // builtin symbol; dotted entries can never be bare identifiers.
        for module in STDLIB_MODULES {
            assert_eq!(is_builtin_symbol(module), !module.contains('.'));
        }
        for extra in NON_MODULE_BUILTIN_SYMBOLS {
            assert!(is_builtin_symbol(extra));
        }
        assert!(!is_builtin_symbol("not_a_module"));
    }
}
