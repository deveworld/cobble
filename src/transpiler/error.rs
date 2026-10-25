//! Typed error channel for the transpiler.
//!
//! Every fallible step of the transpile pipeline returns [`TranspileError`]:
//! a diagnostic [`TranspileErrorKind`] plus the user-visible message text.
//! Most errors are plain semantic errors, so `From<String>` / `From<&str>`
//! build the [`TranspileErrorKind::Semantic`] default and keep `format!(...)?`
//! call sites cheap; sites with a meaningful classification construct the
//! error with [`TranspileError::new`]. Consumers that classify errors
//! (`cobble check`, the web demo) read the kind directly.

use std::fmt;

use super::SourceLocation;

/// Classification of a transpiler error.
///
/// [`TranspileErrorKind::slug`] maps each kind onto the stable diagnostic
/// slug reported as `error[<slug>]` by `cobble check` and the web demo.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TranspileErrorKind {
    /// Default kind: a semantic error with no dedicated classification.
    Semantic,
    /// A single literal loop expands past the per-loop unroll limit.
    UnrollLimit,
    /// Nested unrolling expands past the aggregate iteration limit.
    UnrollNested,
    /// Unrolling generated more commands than the command budget allows.
    UnrollCommandLimit,
    /// A `for` loop iterable is not a literal `range(...)`/array, so it
    /// cannot be unrolled. The loop processor catches this kind and falls
    /// back to the runtime `for` lowerings where they apply.
    UnrollNonLiteral,
    /// A literal `range(...)` whose bounds cannot expand.
    UnrollBadRange,
    /// A literal `range(...)`/`by` step that is invalid.
    UnrollBadStep,
    /// The runtime `for` lowering does not support this loop shape.
    RuntimeForUnsupported,
    /// A runtime `for` step that is invalid.
    RuntimeForBadStep,
    /// A runtime `for` range/iterable that is invalid.
    RuntimeForBadRange,
    /// Internal invariant failure in the runtime `for` lowering.
    RuntimeForInternal,
    /// A stdlib helper was called without importing its module.
    MissingStdlibModule,
    /// `from stdlib import ...` named a module that does not exist.
    UnknownStdlibModule,
    /// `resource_pack.*` was called without the experimental opt-in.
    ResourcePackNotEnabled,
    /// A `resource_pack.*` resource ID is not a valid `namespace:path`.
    ResourcePackBadId,
    /// A `resource_pack.*` JSON payload is not an object.
    ResourcePackNonObject,
    /// Two `resource_pack.*` declarations target one path with different JSON.
    ResourcePackDuplicate,
    /// A `datapack.*` JSON resource uses a shape the targeted Minecraft
    /// version no longer loads (for example a predicate `condition` key).
    DatapackResourceFormat,
}

impl TranspileErrorKind {
    /// Stable diagnostic slug for this kind, as surfaced in
    /// `error[<slug>]` diagnostics.
    pub fn slug(self) -> &'static str {
        match self {
            Self::Semantic => "semantic",
            Self::UnrollLimit | Self::UnrollNested | Self::UnrollCommandLimit => {
                "unroll-limit-exceeded"
            }
            Self::UnrollNonLiteral => "unroll-non-literal",
            Self::UnrollBadRange => "unroll-bad-range",
            Self::UnrollBadStep => "unroll-bad-step",
            Self::RuntimeForUnsupported => "runtime-for-unsupported",
            Self::RuntimeForBadStep => "runtime-for-bad-step",
            Self::RuntimeForBadRange => "runtime-for-bad-range",
            Self::RuntimeForInternal => "runtime-for-internal",
            Self::MissingStdlibModule => "missing-stdlib-module",
            Self::UnknownStdlibModule => "unknown-stdlib-module",
            Self::ResourcePackNotEnabled => "resource-pack-not-enabled",
            Self::ResourcePackBadId => "resource-pack-bad-id",
            Self::ResourcePackNonObject => "resource-pack-non-object",
            Self::ResourcePackDuplicate => "resource-pack-duplicate",
            Self::DatapackResourceFormat => "datapack-resource-format",
        }
    }

    /// True for the unroll family of kinds (loop annotation applies).
    pub fn is_unroll(self) -> bool {
        matches!(
            self,
            Self::UnrollLimit
                | Self::UnrollNested
                | Self::UnrollCommandLimit
                | Self::UnrollNonLiteral
                | Self::UnrollBadRange
                | Self::UnrollBadStep
        )
    }

    /// True for the runtime-for family of kinds (loop annotation applies).
    pub fn is_runtime_for(self) -> bool {
        matches!(
            self,
            Self::RuntimeForUnsupported
                | Self::RuntimeForBadStep
                | Self::RuntimeForBadRange
                | Self::RuntimeForInternal
        )
    }
}

/// A problem the transpiler reports without failing the build, such as a
/// local variable with the name of a module variable.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TranspileWarning {
    /// The diagnostic kind, reported as `warning[<kind>]`.
    pub kind: &'static str,
    pub message: String,
    pub help: Option<String>,
    /// The statement the warning is about, when the transpiler knew its
    /// location. As for [`TranspileError::location`], the file can be an
    /// imported file.
    pub location: Option<SourceLocation>,
}

/// A transpiler error: a diagnostic kind, the user-visible message, and the
/// source statement being lowered when it was raised.
///
/// `Display` renders exactly the message text.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TranspileError {
    pub kind: TranspileErrorKind,
    pub message: String,
    /// The innermost statement being lowered when the error was raised, if
    /// the transpiler knew its location. The file is the canonical path of
    /// the file that contains the statement, which can be an imported file.
    pub location: Option<SourceLocation>,
}

impl TranspileError {
    /// Builds an error with an explicit kind.
    pub fn new(kind: TranspileErrorKind, message: impl Into<String>) -> Self {
        Self {
            kind,
            message: message.into(),
            location: None,
        }
    }

    /// Records `location` unless the error already carries one, so the
    /// innermost statement wins as the error travels up nested blocks.
    pub fn with_location_if_missing(mut self, location: Option<SourceLocation>) -> Self {
        if self.location.is_none() {
            self.location = location;
        }
        self
    }

    /// Re-classifies an unclassified (`Semantic`) error, leaving errors that
    /// already carry a specific kind untouched. Used where a shared helper
    /// raises a generic error that the calling context can classify, such as
    /// resource-ID parsing reached from `resource_pack.*`.
    pub fn classify_semantic(mut self, kind: TranspileErrorKind) -> Self {
        if self.kind == TranspileErrorKind::Semantic {
            self.kind = kind;
        }
        self
    }
}

impl fmt::Display for TranspileError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.message)
    }
}

impl std::error::Error for TranspileError {}

impl From<String> for TranspileError {
    fn from(message: String) -> Self {
        Self::new(TranspileErrorKind::Semantic, message)
    }
}

impl From<&str> for TranspileError {
    fn from(message: &str) -> Self {
        Self::new(TranspileErrorKind::Semantic, message)
    }
}
