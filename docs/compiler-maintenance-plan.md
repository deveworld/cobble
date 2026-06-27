# Compiler Maintenance Plan

Status: post-0.9.0 hardening plan.

This document tracks structural compiler work that should be staged separately
from feature releases because it changes parser, diagnostics, source-map, and
transpiler ownership boundaries.

## AST Spans

Current AST nodes mostly store semantic values without source spans. Diagnostics
and source maps therefore recover source positions through parser diagnostics,
line scans, or command emission context. That works for many cases, but it can
drift when a feature rewrites, unrolls, or moves source statements before
commands are emitted.

Target contract:

- Introduce a small `Span` type with file id, byte range, line, and column.
- Add `Located<T>` or span fields to parser-level statements and expressions.
- Preserve spans through imports, compile-time unrolling, resource helpers, and
  generated command metadata.
- Prefer span-backed diagnostics over string/line heuristics.
- Emit source-map entries from the originating AST span, not from the current
  emitter's best-effort line state.

Migration phases:

1. Add span-capable token/parser plumbing while keeping existing AST fields.
2. Add spans to `Statement`, `Expression`, `Function`, and import nodes.
3. Update diagnostics to consume spans where available.
4. Update transpiler command/resource metadata to carry source spans.
5. Remove obsolete line-scan fallbacks only after source-map regression
   snapshots cover imports, unrolling, resource JSON, and raw commands.

Acceptance criteria:

- Parser diagnostics and semantic diagnostics point to the same source location
  for the same construct.
- Source maps remain stable for formatted source, compile-time unrolling,
  imported files, and generated resource JSON.
- WASM and CLI diagnostics use the same span-backed formatted output.

## Module Boundaries

`src/transpiler/mod.rs` and `src/diagnostics.rs` are intentionally conservative
historical modules, but they now carry enough unrelated behavior that small
features can create broad regression risk.

Current 0.9.0 hardening starts this by extracting Python compatibility helpers
from `src/diagnostics.rs` into `src/diagnostics/python_compat.rs`.

Next extraction targets:

- `diagnostics/imports.rs`: import graph loading, cycle/depth checks, and
  missing-symbol help.
- `diagnostics/symbols.rs`: symbol collection and duplicate-name checks.
- `diagnostics/formatting.rs`: source snippets and compact/human formatting.
- `transpiler/functions.rs`: function registration, current function state, and
  command metadata insertion.
- `transpiler/resources.rs`: data-pack/resource-pack declarations and manifest
  resource entries.
- `transpiler/execute.rs`: execute condition lowering and raw condition guards.

Extraction rules:

- Keep public CLI/WASM behavior unchanged during module moves.
- Move tests with the behavior they cover when possible; otherwise add focused
  regression tests before moving code.
- Do not combine module moves with semantic rewrites unless the rewrite is
  needed to preserve behavior.

## Release Gates

Structural work should keep these gates green before merge:

- `cargo fmt --check`
- `cargo check --locked`
- `cargo test --locked`
- `cargo clippy --locked --all-targets -- -D warnings`
- `cargo test --manifest-path web/wasm/Cargo.toml --locked`
- `node scripts/check_web_metadata.mjs`
- `scripts/qa_09_release_gate.sh`
