use super::{DataPack, SourceLocation};
use std::cell::RefCell;
use std::collections::HashMap;
use std::path::{Path, PathBuf};

/// Upper bound for the canonicalization cache so long-lived processes
/// (e.g. `cobble watch`) stay small; once full, lookups fall through to
/// `Path::canonicalize` directly.
const CANONICALIZE_CACHE_LIMIT: usize = 1024;

thread_local! {
    /// Cache of successful `Path::canonicalize` results.
    /// `stable_relative_path` runs once per generated command line and per
    /// JSON source-map entry, and canonicalizing the unchanging display root
    /// (plus the small set of source files) on every call dominated sys time
    /// on large builds. Only successful canonicalizations are cached so
    /// missing paths are retried.
    static CANONICALIZE_CACHE: RefCell<HashMap<PathBuf, PathBuf>> = RefCell::new(HashMap::new());
}

fn canonicalize_cached(path: &Path) -> Option<PathBuf> {
    if let Some(hit) = CANONICALIZE_CACHE.with(|cache| cache.borrow().get(path).cloned()) {
        return Some(hit);
    }
    let canonical = path.canonicalize().ok()?;
    CANONICALIZE_CACHE.with(|cache| {
        let mut cache = cache.borrow_mut();
        if cache.len() < CANONICALIZE_CACHE_LIMIT {
            cache.insert(path.to_path_buf(), canonical.clone());
        }
    });
    Some(canonical)
}

fn stable_relative_path(path: &Path, root: &Path) -> PathBuf {
    let canonical_path = canonicalize_cached(path).unwrap_or_else(|| path.to_path_buf());
    let canonical_root = canonicalize_cached(root).unwrap_or_else(|| root.to_path_buf());

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
