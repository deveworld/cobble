use std::collections::HashMap;

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

/// Built-in functions that can be called from Cobble scripts
pub mod builtins {
    use crate::ast::Expression;

    /// range(n) - Returns a list from 0 to n-1
    pub fn range(n: i32) -> Vec<Expression> {
        (0..n).map(|i| Expression::Number(i as f64)).collect()
    }

    /// len(list) - Returns the length of a list
    pub fn len(list: &[Expression]) -> Expression {
        Expression::Number(list.len() as f64)
    }

    /// str(value) - Converts a value to string
    pub fn to_str(value: &Expression) -> Expression {
        match value {
            Expression::Number(n) => Expression::String(n.to_string()),
            Expression::Boolean(b) => Expression::String(b.to_string()),
            Expression::String(s) => Expression::String(s.clone()),
            _ => Expression::String("None".to_string()),
        }
    }

    /// int(value) - Converts a value to integer
    pub fn to_int(value: &Expression) -> Expression {
        match value {
            Expression::Number(n) => Expression::Number(n.floor()),
            Expression::String(s) => {
                if let Ok(n) = s.parse::<f64>() {
                    Expression::Number(n.floor())
                } else {
                    Expression::Number(0.0)
                }
            }
            Expression::Boolean(b) => Expression::Number(if *b { 1.0 } else { 0.0 }),
            _ => Expression::Number(0.0),
        }
    }
}
