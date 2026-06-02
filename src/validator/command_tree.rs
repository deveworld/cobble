use serde::Deserialize;
use std::collections::HashMap;
use std::path::Path;

#[derive(Debug, Clone, Deserialize)]
pub struct CommandNode {
    #[serde(rename = "type")]
    pub node_type: String,
    #[serde(default)]
    pub children: HashMap<String, CommandNode>,
    #[serde(default)]
    pub executable: bool,
    pub parser: Option<String>,
    pub properties: Option<serde_json::Value>,
    pub redirect: Option<Vec<String>>,
}

impl CommandNode {
    pub fn from_file(path: &Path) -> Result<Self, String> {
        let content = std::fs::read_to_string(path)
            .map_err(|e| format!("Failed to read commands.json: {}", e))?;
        let mut root: Self = serde_json::from_str(&content)
            .map_err(|e| format!("Failed to parse commands.json: {}", e))?;
        root.fixup_root_redirects();
        Ok(root)
    }

    /// The JSON export from Minecraft omits some command redirects to root.
    /// Add them back so nested commands validate the same way Brigadier does.
    fn fixup_root_redirects(&mut self) {
        self.fixup_root_redirect(&["execute", "run"]);
        self.fixup_root_redirect(&["return", "run"]);
    }

    fn fixup_root_redirect(&mut self, path: &[&str]) {
        let mut node = self;
        for segment in path {
            let Some(child) = node.children.get_mut(*segment) else {
                return;
            };
            node = child;
        }

        if node.redirect.is_none() && node.children.is_empty() {
            node.redirect = Some(vec![]); // empty vec = redirect to root
        }
    }

    pub fn resolve_redirect<'a>(
        &'a self,
        root: &'a CommandNode,
        path: &[String],
    ) -> Option<&'a CommandNode> {
        if path.is_empty() {
            return Some(root); // redirect to root
        }
        let mut node = root;
        for segment in path {
            node = node.children.get(segment)?;
        }
        Some(node)
    }
}
