# Cobble 0.6.3 Devlog Outline

Status: draft outline for the 0.6.3 release candidate.

## Theme

Cobble 0.6.3 is a stabilization release. The main story is confidence: generated
data pack output, CLI summaries, validation behavior, and the browser compiler
now have broader regression coverage before larger language work resumes.

## Sections

1. Snapshot-tested generated data packs
   - Explain why output-tree snapshots matter for `.mcfunction`, JSON resources,
     build manifests, and source maps.
   - Mention coverage for the 26.1.2 smoke project, feature matrix, inventory
     example, resource-only fixture, and merge behavior.

2. Stronger CLI workflow checks
   - Cover `doctor`, `build --dry-run`, `build --dry-run --validate`, `inspect`,
     malformed manifest handling, and source-mapped validation diagnostics.
   - Emphasize that tests assert semantic output rather than fragile whitespace.

3. Safer command-tree validation paths
   - Mention custom command-tree fixtures, ready-made `commands.json` download
     fixture coverage, and documented live Mojang manifest E2E.
   - Keep clear that live network generation remains a release-candidate gate,
     not a default unit test.

4. Resource and web compiler parity
   - Cover duplicate resource diagnostics for exact duplicates, invalid
     overwrites, tag declaration conflicts, and declarations across imports.
   - Cover WebAssembly compiler tests for manifest versioning, tag merging, and
     structured diagnostics.
   - Cover `/try` browser E2E and data pack ZIP tests.

5. Repeatable release gates
   - Mention the new QA checklist, Rust CI, pull-request web gates, GitHub
     Pages deployment gates, link checks, package dry-run, and optional
     Minecraft server smoke.
   - Record whether optional real-server QA ran or was skipped for the final
     release.

## Final Release Notes Checklist

- Replace this outline status with final release wording.
- Record the final release date.
- Record the full release gate result from a clean tree.
- Record whether live command-tree E2E passed.
- Record whether optional Minecraft server QA passed or was skipped.

## Current QA Notes

2026-06-03:

- Live command-tree E2E passed in a temporary directory: Cobble generated
  `data/commands.json`, validated `examples/inventory.cbl`, and `doctor`
  reported SHA-1 `18bb0eb6768838b2237821418aa5832d1c837d45` for Minecraft
  26.1.2.
- Optional Minecraft server smoke passed with
  `COBBLE_MINECRAFT_EULA_ACCEPTED=1 scripts/test_minecraft_server.sh`.
