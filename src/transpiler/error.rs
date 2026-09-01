//! Typed error channel for the transpiler.
//!
//! Every fallible step of the transpile pipeline returns [`TranspileError`]:
//! a diagnostic [`TranspileErrorKind`] plus the user-visible message text.
//! Most errors are plain semantic errors, so `From<String>` / `From<&str>`
//! build the [`TranspileErrorKind::Semantic`] default and keep `format!(...)?`
//! call sites cheap; sites with a meaningful classification construct the
//! error with [`TranspileError::new`]. Consumers that classify errors
//! (`cobble check`, the web demo) read the kind directly instead of sniffing
//! message prefixes.

use std::fmt;

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

/// A transpiler error: a diagnostic kind plus the user-visible message.
///
/// `Display` renders the message exactly, so error text reaching users is
/// unchanged by the typed channel.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TranspileError {
    pub kind: TranspileErrorKind,
    pub message: String,
}

impl TranspileError {
    /// Builds an error with an explicit kind.
    pub fn new(kind: TranspileErrorKind, message: impl Into<String>) -> Self {
        Self {
            kind,
            message: message.into(),
        }
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
