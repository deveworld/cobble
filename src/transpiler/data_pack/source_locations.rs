use super::{DataPack, SourceLocation};
use std::path::{Path, PathBuf};

fn stable_relative_path(path: &Path, root: &Path) -> PathBuf {
    let canonical_path = path.canonicalize().unwrap_or_else(|_| path.to_path_buf());
    let canonical_root = root.canonicalize().unwrap_or_else(|_| root.to_path_buf());

    if let Ok(relative_path) = canonical_path.strip_prefix(&canonical_root) {
        if !relative_path.as_os_str().is_empty() {
            return relative_path.to_path_buf();
        }
    }

    if path.is_relative() {
        return path.to_path_buf();
    }

    canonical_path
}

impl DataPack {
    pub(in crate::transpiler::data_pack) fn normalize_source_location(
        &self,
        mut source: SourceLocation,
    ) -> SourceLocation {
        if let Some(root) = &self.source_display_root {
            source.file = stable_relative_path(&source.file, root);
        }
        source
    }

    pub(in crate::transpiler::data_pack) fn format_source_location(
        &self,
        source: &SourceLocation,
    ) -> String {
        Self::format_source_location_with_root(&self.source_display_root, source)
    }

    pub(in crate::transpiler::data_pack) fn format_source_location_with_root(
        root: &Option<PathBuf>,
        source: &SourceLocation,
    ) -> String {
        let mut source = source.clone();
        if let Some(root) = root {
            source.file = stable_relative_path(&source.file, root);
        }
        format!(
            "{}:{}:{}",
            source.file.display(),
            source.line,
            source.column
        )
    }
}
