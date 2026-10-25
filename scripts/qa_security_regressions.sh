#!/usr/bin/env bash
set -euo pipefail

repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$repo_root"

run() {
  echo "+ $*"
  "$@"
}

# Guard against stale pins: a renamed or removed test would make
# `cargo test <name>` match zero tests and silently pass, so every pinned
# name below must resolve to exactly one test before anything runs. The
# verification mirrors the pinned run's filter semantics: substring match
# by default (unit-test pins carry module-path prefixes, so harness --exact
# on the bare name would never match), and --exact where the run uses it.
verify_pin() {
  # Usage: verify_pin [--test <suite>] [--exact] <pinned-test-name>
  local args=("$@")
  local pin="${args[${#args[@]}-1]}"
  local cargo_args=()
  local harness_args=()
  local arg
  for arg in "${args[@]:0:${#args[@]}-1}"; do
    if [ "$arg" = "--exact" ]; then
      harness_args+=("--exact")
    else
      cargo_args+=("$arg")
    fi
  done
  local listing
  if ! listing="$(cargo test --locked ${cargo_args[@]+"${cargo_args[@]}"} "$pin" -- ${harness_args[@]+"${harness_args[@]}"} --list 2>/dev/null)"; then
    echo "error: failed to list tests while verifying pinned test '$pin'" >&2
    exit 1
  fi
  local count
  count="$(printf '%s\n' "$listing" | grep -c ': test$' || true)"
  if [ "$count" -ne 1 ]; then
    echo "error: stale pinned test '$pin' (matched $count tests, expected exactly 1); update the pin in scripts/qa_security_regressions.sh" >&2
    exit 1
  fi
}

echo "== Verifying pinned test names =="
verify_pin build_rejects_namespace_paths_before_write_or_zip
verify_pin build_rejects_traversal_namespace_from_config_without_deleting_functions
verify_pin build_validate_refuses_to_replace_unowned_existing_output
verify_pin build_validate_preserves_unrelated_files_after_prior_nonvalidated_build
verify_pin direct_datapack_writer_rejects_traversal_namespace_without_deleting_functions
verify_pin direct_datapack_writer_rejects_traversal_json_resource_namespace_without_cleanup
verify_pin --test cli_regression_test cli_build_validate_refuses_existing_file_output_without_deleting_it
verify_pin --test cli_regression_test cli_build_allows_symlink_output_parent_but_refuses_symlink_output
verify_pin --test cli_regression_test cli_build_refuses_symlink_descendant_in_existing_output
verify_pin --test integration_test --exact test_execute_raw_python_gt_max_does_not_overflow
verify_pin --test integration_test --exact test_execute_raw_python_unless_gt_max_does_not_overflow
verify_pin --test integration_test --exact test_execute_raw_python_unless_gt_max_with_selector_score_is_unconditional
verify_pin --test integration_test --exact test_execute_raw_python_unless_or_ignores_impossible_gt_max_branch
verify_pin --test integration_test --exact test_execute_raw_python_unless_or_negates_prefixed_branch
verify_pin --test integration_test --exact test_execute_raw_python_unless_negates_prefixed_not_equal_condition
verify_pin --test integration_test --exact test_execute_raw_python_or_and_unless_and_combination_is_composable
verify_pin --test integration_test --exact test_execute_raw_python_multiple_unless_and_guards_are_all_preserved
verify_pin --test integration_test --exact test_execute_raw_python_unless_and_does_not_become_positive_if_chain
verify_pin symlinks_above_the_root_are_allowed_and_at_or_below_it_refused
verify_pin build_validate_preserves_previous_output_on_validation_failure
verify_pin --test cli_regression_test --exact cli_link_allows_symlinked_parent_but_refuses_symlinked_datapacks_folder
verify_pin --test cli_regression_test --exact cli_link_status_doctor_and_clean_reject_tampered_pack_path
verify_pin --test cli_regression_test --exact cli_link_watch_and_clean_reject_forged_same_namespace_marker
verify_pin --test cli_regression_test --exact cli_watch_link_refuses_unmarked_existing_pack
verify_pin --test cli_regression_test --exact cli_watch_link_refuses_mismatched_marker_namespace
verify_pin --test cli_regression_test --exact cli_doctor_json_rejects_symlinked_link_marker_parent
verify_pin --test cli_regression_test --exact cli_clean_refuses_unmarked_output
verify_pin --test cli_regression_test --exact cli_clean_linked_requires_confirmation_and_removes_marked_linked_output
verify_pin --test cli_regression_test --exact cli_clean_allows_symlink_parent_but_refuses_symlink_descendant
verify_pin --test unrolling_test --exact rejects_unrolls_over_the_limit
verify_pin --test unrolling_test --exact rejects_nested_unrolls_over_the_aggregate_limit
verify_pin --test unrolling_test --exact rejects_unrolled_command_output_over_the_aggregate_limit
verify_pin --test unrolling_test --exact large_unrolls_print_an_expansion_warning
verify_pin --test cli_regression_test --exact cli_check_json_reports_unroll_budget_diagnostics
echo "All pinned test names resolved"

echo
echo "== Build output and namespace safety regressions =="
run cargo test --locked build_rejects_namespace_paths_before_write_or_zip
run cargo test --locked build_rejects_traversal_namespace_from_config_without_deleting_functions
run cargo test --locked build_validate_refuses_to_replace_unowned_existing_output
run cargo test --locked build_validate_preserves_unrelated_files_after_prior_nonvalidated_build
run cargo test --locked direct_datapack_writer_rejects_traversal_namespace_without_deleting_functions
run cargo test --locked direct_datapack_writer_rejects_traversal_json_resource_namespace_without_cleanup
run cargo test --locked --test cli_regression_test cli_build_validate_refuses_existing_file_output_without_deleting_it
run cargo test --locked --test cli_regression_test cli_build_allows_symlink_output_parent_but_refuses_symlink_output
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
run cargo test --locked symlinks_above_the_root_are_allowed_and_at_or_below_it_refused
run cargo test --locked build_validate_preserves_previous_output_on_validation_failure
run cargo test --locked --test cli_regression_test cli_link_allows_symlinked_parent_but_refuses_symlinked_datapacks_folder -- --exact
run cargo test --locked --test cli_regression_test cli_link_status_doctor_and_clean_reject_tampered_pack_path -- --exact
run cargo test --locked --test cli_regression_test cli_link_watch_and_clean_reject_forged_same_namespace_marker -- --exact
run cargo test --locked --test cli_regression_test cli_watch_link_refuses_unmarked_existing_pack -- --exact
run cargo test --locked --test cli_regression_test cli_watch_link_refuses_mismatched_marker_namespace -- --exact
run cargo test --locked --test cli_regression_test cli_doctor_json_rejects_symlinked_link_marker_parent -- --exact
run cargo test --locked --test cli_regression_test cli_clean_refuses_unmarked_output -- --exact
run cargo test --locked --test cli_regression_test cli_clean_linked_requires_confirmation_and_removes_marked_linked_output -- --exact
run cargo test --locked --test cli_regression_test cli_clean_allows_symlink_parent_but_refuses_symlink_descendant -- --exact

echo
echo "== Compile-time expansion budget regressions =="
run cargo test --locked --test unrolling_test rejects_unrolls_over_the_limit -- --exact
run cargo test --locked --test unrolling_test rejects_nested_unrolls_over_the_aggregate_limit -- --exact
run cargo test --locked --test unrolling_test rejects_unrolled_command_output_over_the_aggregate_limit -- --exact
run cargo test --locked --test unrolling_test large_unrolls_print_an_expansion_warning -- --exact
run cargo test --locked --test cli_regression_test cli_check_json_reports_unroll_budget_diagnostics -- --exact
(
  cd web
  run npm run test:wasm
)

echo
echo "Security regression QA passed"
