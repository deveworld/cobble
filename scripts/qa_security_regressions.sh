#!/usr/bin/env bash
set -euo pipefail

repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$repo_root"

run() {
  echo "+ $*"
  "$@"
}

echo "== Build output and namespace safety regressions =="
run cargo test --locked build_rejects_namespace_paths_before_write_or_zip
run cargo test --locked build_rejects_traversal_namespace_from_config_without_deleting_functions
run cargo test --locked build_validate_refuses_to_replace_unowned_existing_output
run cargo test --locked build_validate_preserves_unrelated_files_after_prior_nonvalidated_build
run cargo test --locked direct_datapack_writer_rejects_traversal_namespace_without_deleting_functions
run cargo test --locked direct_datapack_writer_rejects_traversal_json_resource_namespace_without_cleanup
run cargo test --locked --test cli_regression_test cli_build_validate_refuses_existing_file_output_without_deleting_it
run cargo test --locked --test cli_regression_test cli_build_refuses_symlink_output_parent_component
run cargo test --locked --test cli_regression_test cli_build_refuses_symlink_descendant_in_existing_output

echo
echo "== Execute guard translation regressions =="
run cargo test --locked --test integration_test test_execute_raw_python_gt_max_does_not_overflow -- --exact
run cargo test --locked --test integration_test test_execute_raw_python_unless_gt_max_does_not_overflow -- --exact
run cargo test --locked --test integration_test test_execute_raw_python_unless_gt_max_with_selector_score_is_unconditional -- --exact
run cargo test --locked --test integration_test test_execute_raw_python_unless_or_ignores_impossible_gt_max_branch -- --exact
run cargo test --locked --test integration_test test_execute_raw_python_unless_or_negates_prefixed_branch -- --exact
run cargo test --locked --test integration_test test_execute_raw_python_unless_negates_prefixed_not_equal_condition -- --exact
run cargo test --locked --test integration_test test_execute_raw_python_or_and_unless_and_combination_is_composable -- --exact
run cargo test --locked --test integration_test test_execute_raw_python_multiple_unless_and_guards_are_all_preserved -- --exact
run cargo test --locked --test integration_test test_execute_raw_python_unless_and_does_not_become_positive_if_chain -- --exact

echo
echo "== Link, clean, symlink, and ownership regressions =="
run scripts/qa_07_link_clean_safety.sh

echo
echo "== Compile-time expansion budget regressions =="
run scripts/qa_08_unrolling.sh
(
  cd web
  run npm run test:wasm
)

echo
echo "Security regression QA passed"
