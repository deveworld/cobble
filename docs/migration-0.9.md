# Migrating From Cobble 0.8 To 0.9

Status: 0.9.0 migration notes.

Cobble 0.9.0 targets Minecraft Java Edition 26.1.2 and requires pack format
`101.1`. The migration path is intentionally report-first: normal build, check,
and watch commands never rewrite project files.

## Recommended Flow

1. Review the project with the migration report:

```bash
cobble migrate --from 0.8 --to 0.9 --json
```

2. Update `cobble.toml` only after reviewing the report:

```bash
cobble migrate --from 0.8 --to 0.9 --apply
```

3. Re-run project checks and a validated build:

```bash
cobble check --json --experimental-python-compat
cobble build --validate
```

`--apply` is deliberately narrow in 0.9.0. It can update supported config
settings such as `[project].pack_format` to `101.1`; it does not rewrite source
files and does not enable experimental features. Before changing
`cobble.toml`, Cobble writes a timestamped backup next to the original file and
reports that path as `config.backup_path`.

The JSON report is designed for CI review. It includes:

- `config.changes[]` with before/after values for supported config updates.
- `actions[]` entries with before/after values for pack-format changes.
- `source.file_details[]` with per-file counts and manual-review locations.
- `suggested_cobble_alternative` hints for unsupported Python-like constructs.

## Items To Review Manually

- `pack_format`: update old values such as `81` to `101.1`.
- `resource_pack.*`: keep `[experimental] resource_pack = true` or pass
  `--experimental-resource-pack` when using resource-pack helpers.
- Python-like source: use `cobble check --experimental-python-compat` to see
  the supported Python-inspired surface and unsupported constructs that remain
  errors.
- Plugins: `plugins/*.toml` manifests are experimental and diagnostics-only in
  0.9.0. Cobble can evaluate built-in declarative rules, but it does not run
  project plugin code.

## CI Gate

For release candidates and larger project upgrades, run:

```bash
scripts/qa_09_release_gate.sh
```

Use `COBBLE_QA_ALLOW_DIRTY=1` only for local rehearsal against uncommitted
changes. The final release gate should run on a clean worktree.
