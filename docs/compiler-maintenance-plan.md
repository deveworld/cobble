# Compiler Maintenance Plan

Status: post-0.9.0 hardening plan.

This document tracks structural compiler work that should be staged separately
from feature releases because it changes parser, diagnostics, source-map, and
transpiler ownership boundaries.

## Source Provenance And Spans

Current AST nodes store semantic values without source provenance. Diagnostics
and source maps therefore recover source positions through parser diagnostics,
line scans, normalized statement keys, or command emission context. That works
for many cases, but it can drift when a feature rewrites, unrolls, imports, or
moves source statements before commands are emitted.

Target contract:

- Keep the public `Program`, `Statement`, and `Expression` AST shape stable
  during 0.9.x hardening. These are public serde-derived API types and should
  not gain direct span fields in a patch release.
- Introduce additive located parser APIs such as `LocatedProgram` and
  `SpanTable` sidecars. Sidecars should mirror source/AST structure explicitly;
  they must not rely on pointer identity or semantic node hashing because later
  passes clone and rewrite AST nodes.
- Track file identity outside `SourceSpan`. `SourceSpan` is a byte/line/column
  range within one source; multi-file provenance needs a file id/path supplied
  by `ParsedSourceFile`, transpiler source context, or source-map metadata.
- Preserve provenance through imports, compile-time unrolling, resource helpers,
  and generated command metadata.
- Prefer span-backed diagnostics over string/line heuristics.
- Emit source-map entries from the originating source span when available, not
  from the current emitter's best-effort line state.
- Keep `.cobble/source_map.json` stable initially as
  `source: { file, line, column }`. Ranges or file ids in serialized metadata
  should be optional or schema-versioned.

Migration phases:

1. Add span-capable token/parser plumbing while keeping existing AST fields.
   `SourceSpan`, `SpannedToken`, and `tokenize_spanned` now provide byte
   range, line, and column data without changing the existing `tokenize`/parser
   API.
2. Add span-aware diagnostic constructors while preserving the existing
   line/column diagnostic JSON and display shape.
3. Add additive located parser APIs that return `Program` plus statement/import
   provenance sidecars. Keep `parse`, `parse_with_diagnostics`, and
   `token_parser` behavior unchanged.
4. Add source-map provenance regression tests before changing behavior:
   imported files, compile-time unrolling, raw commands, CRLF, Unicode, and
   multiline calls.
5. Update transpiler command/resource metadata to prefer located spans with
   existing line-scan/source-key fallbacks.
6. Migrate post-parse semantic diagnostics to consume spans where available.
   Keep pre-parse line-scan checks for indentation, structural syntax, and
   unsupported Python constructs.
7. Remove obsolete line-scan fallbacks only after source-map regression
   snapshots cover imports, unrolling, resource JSON, and raw commands.

Acceptance criteria:

- Parser diagnostics and semantic diagnostics point to the same source location
  for the same construct.
- Source maps remain stable for formatted source, compile-time unrolling,
  imported files, and generated resource JSON.
- WASM and CLI diagnostics use the same span-backed formatted output.

Release split:

- `0.9.x`: safe bug/security fixes, docs, metadata sync, and
  behavior-preserving module moves only. Do not change public AST shape or
  source-map schema.
- `0.10.0`: source provenance stabilization. Add located parser APIs, file
  identity, parser/semantic diagnostic parity, and transpiler/source-map span
  propagation with fallbacks.
- `1.0.0`: only after one release cycle proves span-backed diagnostics and
  source maps stable.

## Module Boundaries

`src/transpiler/mod.rs` and `src/diagnostics.rs` are intentionally conservative
historical modules, but they now carry enough unrelated behavior that small
features can create broad regression risk.

Current 0.9.0 hardening starts this by extracting Python compatibility helpers
from `src/diagnostics.rs` into `src/diagnostics/python_compat.rs` and
diagnostic reporting types/formatting into `src/diagnostics/reporting.rs`.

Next extraction targets:

- `diagnostics/imports.rs`: import graph loading, cycle/depth checks, and
  missing-symbol help.
- `diagnostics/symbols.rs`: symbol collection and duplicate-name checks.
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

## Known Debt

Deliberately left in place by the 2026-08 hardening pass; recorded here so
the scope decision is explicit. Line anchors are as of that pass.

- Text-heuristic diagnostics engine — `src/diagnostics.rs` (~6.4k lines
  including tests) scans source line-by-line without an AST, and the
  expression/keyword skip lists are replicated across several scan helpers,
  so each new syntax form must be registered in multiple places. Extraction
  plan: move post-parse semantic checks onto the span-backed AST checker
  described under Source Provenance And Spans, keeping only the pre-parse
  structural/indentation scans as text passes.
- Assignment decision ladder —
  `src/transpiler/statement_processors/assignment.rs:180`
  (`process_assignment`, ~1,000 lines of ordered special cases deciding
  storage vs. scoreboard lowering). Extraction plan: split into a target
  classification step plus per-kind lowering functions once
  `ExpressionEvaluator` is the single scoreboard codegen path.
- Execute-block string surgery —
  `src/transpiler/statement_processors/execute_processor.rs:17`
  (`process_execute_block_impl`) rewrites condition markers and scoreboard
  holders through string substitution on rendered commands. Extraction plan:
  lower execute conditions to a structured condition value before emission
  (the `transpiler/execute.rs` extraction target above).
- Three loop drivers — `src/transpiler/statement_processors/loop_processor.rs:203`
  (`process_runtime_range_for`), `:353` (`process_runtime_storage_list_for`),
  and `:1186` (`process_while`) repeat the recursive-helper loop shape with
  copy-paste variation. Extraction plan: one parameterized loop-driver
  builder with init/condition/step hooks.
- Error-string-keyed classification — now centralized in
  `src/diagnostics/reporting.rs:145` (`transpiler_error_is_recognized`) and
  `:156` (`classify_transpiler_error`), but transpiler errors are still
  `String`s keyed by prefix/wording, and
  `src/transpiler/statement_processors/loop_processor.rs:75` branches on the
  `unroll-non-literal` prefix. Extraction plan: a typed transpiler error enum
  carrying kind plus span, rendered to strings only at the CLI/WASM edge.
- Watch options — `cobble watch` passes a `WatchOptions` struct internally
  (`src/commands/watch.rs:20`, constructed at `src/main.rs:345`), but the
  clap `Watch` variant still declares each field individually. Extraction
  plan: derive clap `Args` on the options struct and flatten it into the
  subcommand.
