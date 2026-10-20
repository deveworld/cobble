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

Migration phases (status as of the diagnostics span migration: phases 1-4 and
6 are delivered; phase 5 — transpiler command/resource metadata preferring
located spans — and phase 7 remain):

1. Add span-capable token/parser plumbing while keeping existing AST fields.
   `SourceSpan`, `SpannedToken`, and `tokenize_spanned` now provide byte
   range, line, and column data without changing the existing `tokenize`/parser
   API.
2. Add span-aware diagnostic constructors while preserving the existing
   line/column diagnostic JSON and display shape.
3. Add additive located parser APIs that return `Program` plus statement/import
   provenance sidecars. Keep `parse`, `parse_with_diagnostics`, and
   `token_parser` behavior unchanged. Delivered: `parse_located_with_diagnostics`
   returns a `ProgramSpanTable` whose `statement_tree` mirrors the parsed
   `Program` one node per statement (`elif`/`else` folded into their `if`,
   `case` headers into their `match`), checked by `verify_span_tree_alignment`
   and dropped rather than published when a shape mismatch is detected.
4. Add source-map provenance regression tests before changing behavior:
   imported files, compile-time unrolling, raw commands, CRLF, Unicode, and
   multiline calls.
5. Update transpiler command/resource metadata to prefer located spans with
   existing line-scan/source-key fallbacks.
6. Migrate post-parse semantic diagnostics to consume spans where available.
   Keep pre-parse line-scan checks for indentation, structural syntax, and
   unsupported Python constructs. Delivered: `src/diagnostics/semantic.rs`
   walks statements and span nodes in lockstep, and the pre-parse layer keeps
   only the checks that must guard the parser.
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
- `scripts/qa_10_release_gate.sh`

## Known Debt

Deliberately left in place by the 2026-08 hardening pass; recorded here so
the scope decision is explicit. Entries retired by later structural rounds
stay listed with one-line resolution notes.

- Text-heuristic diagnostics engine — resolved in a later round. Every
  post-parse semantic check now runs as a span-anchored AST pass in
  `src/diagnostics/semantic.rs`; `src/diagnostics.rs` keeps only the
  pre-parse structural, indentation, and unsupported-Python scans plus the
  staged orchestration, and each source is parsed exactly once per analysis.
  End state: `src/diagnostics.rs` 6,391 → 3,148 lines,
  `src/diagnostics/semantic.rs` 4,066 (new), `src/diagnostics/imports.rs`
  1,045 → 951. Verified against a 195-fixture diagnostic corpus captured
  before the migration: byte-identical output except three reviewed deltas
  (one position improvement, two redundant co-reports dropped on sources that
  never parse). Remaining follow-up is listed under Source Provenance And
  Spans phase 5.
- Assignment decision ladder — resolved in this round: `process_assignment`
  now classifies the target (`AssignTargetKind`: storage expression,
  module-level, local variable) and dispatches to per-kind lowering
  functions.
- Execute-block string surgery — resolved in this round: condition
  translation returns a structured `TranslatedCondition` value, so execute
  emission no longer rewrites marker strings in rendered commands.
- Three loop drivers — resolved in this round: runtime `range(...)` `for`,
  storage-list `for`, and `while` all run through one parameterized
  `emit_runtime_loop(RuntimeLoopDriver)` with init/condition/step hooks.
- Error-string-keyed classification — resolved in this round: transpiler
  errors are typed `TranspileError` values carrying a `TranspileErrorKind`,
  classification reads the kind instead of matching message prefixes, and
  errors are rendered to strings only at the CLI/WASM edge. (Spans on errors
  remain future work under Source Provenance And Spans.)
- Watch options — resolved in this round: `WatchOptions` derives clap `Args`
  and is flattened into the `watch` subcommand.
