# Test Decomposition Analysis — faber

Generated: 2026-07-19

## Summary

| Lens | Findings | Critical | High | Medium | Low |
|---|---|---|---|---|---|
| Coverage gaps | 28 | 3 | 8 | 12 | 5 |
| Missing negatives | 22 | 2 | 5 | 10 | 5 |
| Redundancy | 16 | 0 | 2 | 8 | 6 |
| Setup complexity | 12 | 1 | 3 | 5 | 3 |

## Top 10 recommendations (ranked by impact)

1. **[Critical] `src/package/mir.rs` — massive untested surface (~5000 lines, 2 unit tests)**
   The MIR module owns package linking, FMIR image building, artifact manifest generation, binary bundle construction, and the entire `with_lowered_package_mir` pipeline. The only two unit tests (`fmir_runtime_cli_binding_skips_superset_decoy_record` and one more at line 142) cover a fraction of one internal MIR-to-CLI function. The `build_package_mir_artifact`, `build_package_fmir_image`, `build_package_fmir_text_image`, `build_package_fmir_binary_bundle`, `run_package_fmir_image`, and the entire `PackageMirConsumer` / `CliPlanningMode` dispatch surface have no direct unit tests. All MIR artifact behavior is exercised only through `package_test.rs` integration tests.

2. **[Critical] `src/commands/run.rs` — untested command dispatch (~500 lines, 0 unit tests for cmd_run)**
   `cmd_run` has 8 target branches (Rust, Go, Scena, FmirText, Fmir, FmirBin, + unsupported targets). `run_test.rs` only tests `should_interpret` (policy decision helper) and `run_config`. The actual command handler `cmd_run`, `cmd_run_go`, `cmd_run_scena`, `cmd_run_fmir_text`, `cmd_run_fmir`, `cmd_run_fmir_bin`, `cmd_run_compiled` have zero unit tests. Go and FMIR target paths that compile, write binaries, and exec are untested code paths that spawn real processes.

3. **[Critical] `src/package/compile.rs` — near-absent unit test coverage (~800 lines, 1 test for `ensure_go_import`)**
   `analyze_package`, `compile_package`, `check_package`, `rust_runtime_plan_for_package`, `generate_library_unit_rust`, and the entire `AnalyzedPackage` + `AnalyzedPackageUnit` construction surface are tested only through `package_test.rs` integration tests. There are no unit tests for the HIR lowering, import graph building, mount planning, or library resolution within the compile path.

4. **[High] `src/commands/format.rs` — all tests are subprocess/integration (0 unit-level)**
   `format_test.rs` has 12 tests, but every one drives the formatter through the `faber` binary subprocess (`run_faber_format_stdout`) or calls the radix `test_gate` helpers. `cmd_format` itself is never tested directly. `resolve_format_paths`, `format_session`, and `formatted_source_for_write` have no dedicated tests.

5. **[High] `src/package/binding_probe.rs` — timeout, spawn-failure, and probe-cache paths untested**
   `binding_probe_test.rs` tests `probe_manifest` generation (4 tests). The `run_rust_binding_probe` function — which spawns `cargo check` subprocesses with a 60-second timeout, manages a global cache, and handles cleanup failures — has zero tests. The probe timeout path, spawn failure path, cache-hit path, and cleanup error path are all untested.

6. **[High] `src/package/frontmatter.rs` — conflict detection and test-selection merging untested**
   `validate_frontmatter_against_manifest` checks 6 conflict dimensions (target, kind, name, version, source, entry), and `merge_entry_test_selection` merges CLI and frontmatter test selectors with mutual exclusion logic. Neither function has a unit test.

7. **[High] Untested source files (no companion test file, no inline tests)**
   These source files have zero dedicated tests: `src/cli/emit.rs`, `src/commands/archive.rs`, `src/commands/explain.rs`, `src/commands/host.rs`, `src/commands/init.rs`, `src/commands/targets.rs`, `src/commands/test.rs`, `src/explain_render.rs`, `src/package/cmd.rs`, `src/package/codegen.rs`, `src/package/discovery.rs`, `src/package/dispatch.rs`, `src/package/file_interface.rs`, `src/package/import_graph.rs`, `src/package/library.rs`, `src/package/manifest.rs`, `src/package/paths.rs` (inline only, 2 test fns), `src/package/product.rs`, `src/package/reader.rs`, `src/package/source_files.rs`, `src/script/trap.rs`. Some are tested indirectly through `package_test.rs`, but many critical functions (like `read_manifest`, `validate_manifest`, `resolve_import`, `discover_package`) are verified only through integration tests.

8. **[High] `src/package/discovery.rs` — `discover_package` and `discover_build_layout` untested at unit level**
   These are the primary entry points for package resolution. `discover_package` resolves `PackageSpec` from path input (manifest-backed, directory, legacy). `discover_build_layout` builds the `BuildLayout`. Both are foundational to every package test but never tested in isolation.

9. **[Medium] `src/package/binding.rs` — missing negative: probe-gate poisoned, manifest with missing keys, empty function list**
   `binding_test.rs` has excellent coverage for the happy path and common errors (missing symbol, signature mismatch, async mismatch, error channel, nested methods). But the `PROBE_GATE` poisoned mutex path, an empty or malformed binding TOML file, and the `shim`-present-but-missing-on-disk case are untested.

10. **[Medium] `src/reference_test.rs` — missing negative: corrupted index, empty exempla, bad PACK.toml**
    `reference_test.rs` has good coverage for normal operations and some error paths. Missing: `index.toml` with `registry_terms` count mismatch (the error path at line 191-198 of `reference.rs`), `PACK.toml` with invalid TOML syntax, an exempla directory with no `.fab` files, and a legacy redirect pointing to itself.

---

## Per-file details

### `src/package/mir.rs` / `src/package/mir_test.rs`
- **Coverage gaps (Critical):** ~5000-line module with only 2 unit tests, both for internal `fmir_runtime_cli_binding_*` functions. The following public/crate functions are untested at unit level:
  - `with_lowered_package_mir` (line 49)
  - `build_package_mir_artifact` — builds MIR artifact with manifest
  - `build_package_fmir_text_image` — FMIR text image generation
  - `build_package_fmir_image` — FMIR binary image generation
  - `build_package_fmir_binary_bundle` — FMIR binary bundle with runner crate
  - `run_package_fmir_image` — FMIR image execution
  - `run_fmir_image_path` — FMIR image path execution
  - `run_package_fmir_text_image` — FMIR text execution
  - Entire `PackageMirConsumer` dispatch (line ~200+)
  - `CliPlanningMode` variants (Parsed, AsCliProgram, AsFmirText, AsFmir)
- **Missing negatives (Critical):** No test for: MIR validation failure, duplicate function IDs, missing entry function, unsupported package shapes, symlink escape in artifact paths, missing files in manifest.
- **Redundancy:** N/A (too little coverage to have redundancy).
- **Setup complexity (High):** The `fmir_runtime_cli_binding_skips_superset_decoy_record` test has ~70 lines of setup constructing MIR programs by hand. This suggests the MIR construction API could use builder helpers.

### `src/commands/run.rs` / `src/commands/run_test.rs`
- **Coverage gaps (Critical):** `run_test.rs` tests `should_interpret` (5 tests) and `run_config` (3 tests). The following are untested:
  - `cmd_run` (line 37) — entry point with reader-locale validation, interpret-vs-compile dispatch
  - `cmd_run_go` (line 133) — `go build` + exec path
  - `cmd_run_scena` — MIR artifact run
  - `cmd_run_fmir_text` — FMIR text run
  - `cmd_run_fmir` — FMIR binary run
  - `cmd_run_fmir_bin` — FMIR binary native run
  - `cmd_run_compiled` — Rust compiled run path
  - `run_target_name` — all targets except Scena/Fmir are tested only implicitly
- **Missing negatives (High):** No test for: unsupported target error, reader-locale-with-interpret conflict, compile failure in `cmd_run_go`, Go output with wrong variant, missing binary after build.
- **Redundancy:** Low — tests are lean and focused.
- **Setup complexity (Medium):** `should_interpret` tests are clean. `run_scena_package_forwards_argv_through_artifact` has ~30 lines of setup.

### `src/package/compile.rs` / `src/package/compile_test.rs`
- **Coverage gaps (Critical):** 1 test total (`ensure_go_import_ignores_matching_string_literals`). The following are untested at unit level:
  - `analyze_package` (builds `AnalyzedPackage` from filesystem)
  - `compile_package` (HIR analysis + codegen + output assembly)
  - `check_package` (diagnostics mode without codegen)
  - `rust_runtime_plan_for_package` (async detection, route analysis, host selection)
  - `generate_library_unit_rust` (codegen for library packages)
  - `compile_package_with_test_selection` (test filtering)
  - The entire `GeneratedPackageRust` assembly pipeline
- **Missing negatives (Critical):** No test for: import resolution failure, cyclic imports, missing source files, frontmatter/manifest conflicts in the compile path, large package with many units.
- **Redundancy:** N/A.
- **Setup complexity (High):** Integration tests in `package_test.rs` show 20-40 lines of filesystem setup per test case.

### `src/commands/format.rs` / `src/commands/format_test.rs`
- **Coverage gaps (High):** 12 tests but all are integration or radix test_gate. Untested directly:
  - `cmd_format` (line 20) — the full format command loop
  - `resolve_format_paths`
  - `format_session`
  - The `--check` exit-code path (tested via subprocess)
  - The `--stdout` path (tested via subprocess)
  - `formatted_source_for_write`
- **Missing negatives (Medium):** No test for: unreadable `.fab` file, file that becomes empty after formatting, paths with special characters, `format_session` with missing reader pack.
- **Redundancy (Medium):** Repeated `run_faber_format_stdout` / `run_faber_format_stdout_with_args` calls. The Thai localization test (line 136-192) has 57 lines — 30+ are string-contains assertions. Could be table-driven.
- **Setup complexity (Low):** Tests are relatively clean with helpers.

### `src/package/binding_probe.rs` / `src/package/binding_probe_test.rs`
- **Coverage gaps (High):** `binding_probe_test.rs` tests `probe_manifest` only. Untested:
  - `run_rust_binding_probe` (line 58) — spawns `cargo check`, manages cache, handles timeout
  - Probe timeout path (60-second deadline at line 190)
  - Probe spawn failure path
  - Probe cache-hit shortcut (lines 63-69)
  - Probe gate poisoned error (line 70)
  - Cleanup failure with successful probe (lines 91-94)
  - Cleanup failure with failed probe (lines 95-99)
  - `probe_source` (shim + probes assembly)
  - `read_output`
  - `truncate_output` (8000-char limit)
  - `probe_root` (unique temp directory naming)
- **Missing negatives (High):** No test for: timeout, killed probe, non-zero cargo exit, cargo stdout/stderr with diagnostic output, edge case where temp directory creation fails.
- **Redundancy:** N/A (tests too few).
- **Setup complexity:** N/A (not tested).

### `src/package/frontmatter.rs`
- **Coverage gaps (High):** Zero tests. Untested:
  - `manifest_path_for_spec` (line 8) — finds manifest from PackageSpec
  - `validate_frontmatter_against_manifest` (line 46) — 6 conflict dimensions
  - `merge_entry_test_selection` (line 122) — CLI vs frontmatter precedence, mutual exclusion with `cli_overrides`
- **Missing negatives (High):** No test for: each of the 6 conflict dimensions in `validate_frontmatter_against_manifest`, CLI selection with overlapping frontmatter selection, empty frontmatter with CLI selection.
- **Redundancy:** N/A (no tests).
- **Setup complexity:** N/A.

### `src/package/discovery.rs`
- **Coverage gaps (High):** Zero dedicated tests. Untested at unit level:
  - `discover_package` — `PackageSpec` resolution from path
  - `discover_build_layout` — `BuildLayout` construction
  - `is_manifest_backed_or_directory_package_input`
  - `sanitize_crate_name`
- **Missing negatives (High):** No test for: non-existent path, path without source directory, manifest with missing `[paths]` section, package at filesystem root.
- **Redundancy:** N/A.
- **Setup complexity (High in integration):** Every `package_test.rs` test constructs packages through these functions. Extracting unit tests would reduce all downstream setup.

### `src/package/binding.rs` / `src/package/binding_test.rs`
- **Coverage gaps (Medium):** `binding_test.rs` has 9 tests covering analyzed layout, nested methods, missing symbols, signature mismatches, async mismatch, error channel mismatch, duplicate rows, parent escape, and absolute paths. Good coverage. Missing:
  - `verify_library_bindings` with manifest validation failure
  - Binding manifest with `shim` section but missing file on disk
  - Binding with 0 functions (empty bindings)
  - Binding with a function that has `has_body = false` (delegata)
- **Missing negatives (Medium):** No test for: manifest with `build.kind` not equal to "lib", target not declared in `build.targets`, missing `[target.rust]` section, `PROBE_GATE` poisoned.
- **Redundancy (Low):** `test_package` helper reduces duplication. Tests are clean.
- **Setup complexity (Medium):** `test_package` builds a full directory tree per test. Each test writes 4 files. Could use an in-memory manifest builder.

### `src/package/lockfile.rs` / `src/package/lockfile_test.rs`
- **Coverage gaps (Medium):** 3 tests: duplicate lock rejection, duplicate index rejection, duplicate+dependency validation. Missing:
  - `read_lock` with missing file (returns `Ok(None)`)
  - `read_lock` with valid lock file
  - `validate_dependencies_against_lock` with empty dependencies (returns empty `Vec`)
  - `validate_dependencies_against_lock` with missing lock
  - `validate_dependencies_against_lock` with version mismatch
  - `validate_dependencies_against_lock` with missing package_root/interface_root on disk
  - `validate_dependencies_against_lock` with prebuilt artifact (non-source/lib kind)
  - `validate_locked_paths` for artifact/target_manifest paths
  - `LockedPackage::resolve_path`, `package_root_path`, `interface_root_path_for`
- **Missing negatives (Medium):** No test for: invalid TOML in lock file, lock file with `deny_unknown_fields` violation, malformed lock with missing required fields.
- **Redundancy (Low):** Tests are lean.
- **Setup complexity (Low):** `package()` and `duplicate_lock_source()` helpers reduce boilerplate.

### `src/package/artifact_plan.rs` / `src/package/artifact_plan_test.rs`
- **Coverage gaps (Medium):** 5 tests. Good coverage of the plan_package surface. Missing:
  - `plan_package` with actual `AnalyzedPackage` units (only tested with empty package)
  - `linked_library_crate_map` with actual lock data
  - `native_library_deps` happy path (only error paths tested)
  - `plan_rust_artifacts` with tokio detection (async entry/function)
  - `plan_go_artifacts` / `plan_ts_artifacts` with actual units
  - `ArtifactPlan::has_runtime_dependency`
- **Missing negatives (Medium):** No test for: plan with `Target::Faber` (returns unsupported), plan with unknown target, `native_library_deps` with version mismatch, dependency not in lock index.
- **Redundancy (Low):** Tests are clean and focused.
- **Setup complexity (Medium):** `empty_package` helper is good. Adding tests with actual `AnalyzedPackageUnit` data would increase setup.

### `src/package/member_path.rs` / `src/package/member_path_test.rs`
- **Coverage gaps (Medium):** 2 tests (`parent_escape`, `symlink_escape`). Missing:
  - `normalize_member_path` with: empty string, whitespace-only, absolute path, `.` only input, `..` only input, path with only `/` separators, path with `./` prefix, Windows-style `\` (if applicable)
  - `resolve_package_member` with: valid relative path, path that resolves to existing file, path where `package_root` canonicalization fails
  - `nearest_existing_ancestor` directly
- **Missing negatives (High):** No test for: empty package_root, non-existent package_root, path that normalizes to empty string, path with `..` after valid segments (e.g., `src/../escape.fab`), path with `.` components.
- **Redundancy (Low):** Tests are clean.
- **Setup complexity (Low):** `temp_dir` helper is sufficient.

### `src/package/cargo.rs` / `src/package/cargo_test.rs`
- **Coverage gaps (Medium):** 5 tests. Good coverage of Cargo.toml generation. Missing:
  - `package_host_selection_diagnostic` with various plan states
  - `RustRuntimePlan::requires_generated_crate` with tokio-only, host-only, library-path-deps-only
  - `render_generated_cargo_toml` with provider-only plan (no faber-runtime)
  - `invoke_cargo_build` and `invoke_cargo_test` (spawn `cargo` subprocess)
- **Missing negatives (Medium):** No test for: plan with `provider_error` set, plan with `host_required_routes` but no host selection, `render_generated_cargo_toml_with_support` where support returns error.
- **Redundancy (Low):** Tests are clean and focused.
- **Setup complexity (Low-Medium):** Tests are clean with reasonable setup.

### `src/package/runtime_dependency.rs` / `src/package/runtime_dependency_test.rs`
- **Coverage gaps (Medium):** 3 tests. Good coverage of runtime path detection. Missing:
  - `parse_dependency_requirement` with bare version string
  - `parse_dependency_requirement` with malformed inline table (falls back to String)
  - `normalize_dependency_value` with non-table value (passes through)
  - `runtime_path_from_crate_roots` with multiple roots
  - `runtime_path_for_target_dependencies` error path when materialization fails
- **Missing negatives (Medium):** No test for: dependency with path that doesn't exist, Cargo.toml with `faber` but no `faber-runtime` package, manifest with malformed TOML.
- **Redundancy (Low):** Tests are clean.
- **Setup complexity (Medium):** Each test creates a temp directory tree with Cargo manifests.

### `src/package/library_link.rs` / `src/package/library_link_test.rs`
- **Coverage gaps (Medium):** 5 tests. Good coverage of visibility promotion, binding lookup, and Cargo.toml rendering. Missing:
  - `emit_linked_library_crates` (tested only indirectly)
  - `emit_one_library_crate`
  - `binding_for_function` with actual match
  - `promote_binding_function_visibility` with complex source (generics, lifetimes)
- **Missing negatives (Medium):** No test for: `verify_library_bindings` failure in `emit_linked_library_crates`, library without `[target.rust]`, emit failure for one crate in multi-crate deps.
- **Redundancy (Low):** Tests are clean.
- **Setup complexity (Low-Medium):** Clean helpers.

### `src/reference.rs` / `src/reference_test.rs`
- **Coverage gaps (Low):** 14 tests, excellent coverage of the reference pack lifecycle. Missing:
  - `ReferencePack::load` when `FABER_REFERENCE_ROOT` points to missing directory
  - `detect_layout` with neither `exempla/` nor bare index
  - `read_metadata` when PACK.toml has invalid TOML
  - `install_sibling_root` discovery (hard to test without installed binary)
  - `dev_repo_root` when no repo root is found (returns `None`)
- **Missing negatives (Medium):** No test for: index.toml with `registry_terms` count mismatch (line 191), empty file list with `fab_count` > 0, PACK.toml with invalid TOML, missing `legacy-redirects.toml` file, legacy redirect pointing to itself.
- **Redundancy (Medium):** 14 tests, 5 of which repeat the `env_lock` + env-var save/restore pattern. Could extract into an `EnvGuard` helper.
- **Setup complexity (Medium):** Tests depend on the real exempla corpus on disk. This coupling means tests only work inside the repo checkout.

### `src/reference_parse.rs` / `src/reference_parse_test.rs`
- **Coverage gaps (Low):** 4 tests. Good coverage of entry parsing. Missing:
  - `legacy_entry_from_redirect`
  - `read_exempla_file`
  - `build_entry_body` with empty comment header, with empty fab source, with fab source that doesn't end with newline
  - `comment_header_to_prose` with mixed comment and blank lines
  - `first_fab_block` with multiple code blocks
  - `normalize_summary` with already-punctuated summary
  - `parse_exempla_kind` with all known kind values, with unknown kind
  - `validate_exempla_entry` with empty syntax, empty summary, empty body, missing fab block
- **Missing negatives (Medium):** No test for: frontmatter missing required fields (category, summary, syntax), unreadable exempla file, malformed exempla body, `legacy_entry_from_redirect` with missing canonical entry.
- **Redundancy (Low):** Tests are clean and targeted.
- **Setup complexity (Low):** Tests use inline string literals.

### `src/explain.rs` / `src/explain_test.rs`
- **Coverage gaps (Low):** 9 tests. Strong coverage. Missing:
  - `Registry::by_category`
  - `Registry::categories`
  - `Registry::version_warning`
  - `Registry::reference_version`
  - `render_category`
  - `render_list` comprehensively (tested only briefly)
  - `render_json` for Legacy variant
  - `search_score` edge cases (empty query, partial match in related terms)
  - `normalize_query` with leading/trailing whitespace
- **Missing negatives (Low):** No test for: `from_entries` with duplicate term, `lookup` with non-existent term, `search` with empty string, `render_json` with Legacy variant.
- **Redundancy (Low):** Tests are clean.
- **Setup complexity (Medium):** Tests load the real exempla corpus from disk.

### `src/diagnostic_explain.rs` / `src/diagnostic_explain_test.rs`
- **Coverage gaps (Low):** 8 tests. Strong coverage of query parsing, lookup, and rendering. Missing:
  - `lookup_installed_diagnostic` with empty reader locale
  - `lookup_installed_diagnostic` where pack id doesn't match locale
  - `is_diagnostic_query` with lowercase-only code
  - `DiagnosticLookupKey::parse` with issue that has uppercase (rejected)
  - `render_plain` with no help text
- **Missing negatives (Low):** No test for: empty reader locale, reader pack with wrong id, missing pack file.
- **Redundancy (Low):** Tests are clean with a `synthetic_pack()` helper.
- **Setup complexity (Low):** Good use of synthetic pack data.

### `src/cli_test.rs` (via `src/main.rs`)
- **Coverage gaps (Low):** 38 tests. Excellent CLI coverage. Missing:
  - `--reader-locale` on `archive` subcommand (if applicable)
  - `--out-dir` flag combinations with `--release`
  - `format --config` flag (warning path)
  - `install --store`, `install --registry`, `install --project` flags
- **Missing negatives (Low):** No test for: `--reader-locale` with empty value, very long CLI args (overflow), Unicode in arguments.
- **Redundancy (High):** 38 tests, many following identical parse-then-assert patterns. Candidate for table-driven tests per subcommand. The explain subcommand alone has 8 tests that could be combined.
- **Setup complexity (Low):** Direct `try_parse_from` calls, minimal setup.

### `src/package_test.rs`
- **Coverage gaps (Medium):** ~950 lines of tests covering the main package pipeline. Good integration coverage. Missing unit tests for functions tested indirectly: `config_with_reader_locale`, `load_package`, `emit_generated_crate`, `library_cached_file_interface`, `library_resolver_from_config`.
- **Missing negatives (Low):** Tests focus on success paths.
- **Redundancy (High):** Repeated patterns: `test_temp_dir` + `fs::create_dir_all` + `fs::write("faber.toml", ...)` + `fs::write("main.fab", ...)` + `compile_package` + assert. The `coreutils_like_package` helper exists but is only used by some tests. The MIR artifact harness pattern (`artifact_harness_hello_case`, etc.) is a good model that could be extended.
- **Setup complexity (High):** Average 15-40 lines of filesystem setup per test. The `compile_emit_build_run` helper consolidates 4 steps but doesn't reduce setup for package creation. Consider a `PackageFixture` builder.

### `src/input_shape.rs` / `src/input_shape_test.rs`
- **Coverage gaps (Low):** 5 tests covering all public functions. Good coverage.
- **Missing negatives (Low):** No test for: empty input array, single-element array with empty string, very long path, path with Unicode, path that triggers OS-level path validation.
- **Redundancy (Low):** Tests are clean.
- **Setup complexity (Low):** Tests are simple boolean assertions.

### `src/script/mod.rs` / `src/script_test.rs`
- **Coverage gaps (Low):** 8 tests. Good coverage of script execution. Missing tests for: `run_source` with the `faber:processus` kernel imports beyond env/cwd, `run_named` error case, `run_with_session` directly.
- **Missing negatives (Low):** No test for: `interpret_source_or_exit` exit behavior (hard to test without process boundary).
- **Redundancy (Low):** Tests are clean.
- **Setup complexity (Low):** Inline source strings in tests.

### `src/commands/script.rs` / `src/commands/script_test.rs`
- **Coverage gaps (Low):** 8 tests. Good coverage. Missing:
  - `interpret_path` with zip archive (tested indirectly?)
  - `manifestless_file_declares_non_kernel_import` with parse failure
  - `manifestless_file_declares_non_kernel_import` with program without statements
- **Missing negatives (Low):** No test for: `interpret_path` with unreadable file, corrupt zip archive, archive with remapped diagnostics.
- **Redundancy (Low):** Tests are clean.
- **Setup complexity (Low):** Clean.

### `src/package/modules.rs` / `src/package/modules_test.rs`
- **Coverage gaps (Low):** 2 tests. Good coverage of `sanitize_rust_module_ident`. Missing:
  - `module_segments_for_file` with frontmatter group (line 13-18)
  - `module_segments_for_file` where `strip_prefix` fails (absolute path outside source root)
  - `module_segments` with `main.fab` filename
- **Missing negatives (Low):** No test for: `module_segments` with path at filesystem root, segments with all-special characters, empty frontmatter group string.
- **Redundancy (Low):** Clean.
- **Setup complexity (Low):** Simple string assertions.

### `src/library.rs` (inline tests)
- **Coverage gaps (Low):** 3 inline tests. Good for the env-var-driven `default_library_home` function. Missing:
  - `LibraryResolver::resolve` with valid specifier
  - `LibraryResolver::resolve` with non-provider-shaped specifier
  - `ResolvedLibraryModule::new`
  - `default_library_home` when `FABER_LIBRARY_HOME` points to a valid directory
- **Missing negatives (Low):** No test for: `resolve` with missing provider, `resolve` with missing module, malformed import specifier.
- **Redundancy (Low):** Clean.
- **Setup complexity (Low):** Clean with env guard helpers.

### `src/core_support_test.rs` / `src/core_support/materialize_test.rs`
- **Coverage gaps (Low):** `core_support_test.rs` has 4 tests, `materialize_test.rs` has 8 tests. Good coverage. Missing:
  - `materialize` (the default entry point using platform cache dir)
  - Materialization when cache root doesn't exist (auto-create)
  - `MaterializedCoreSupport::provider` with each valid provider name
- **Missing negatives (Low):** Already covered: hash mismatch, unexpected files, duplicates, unsafe paths, oversized entries, symlink escape, group-writable root, symlinked cache entry, corruption recovery. Good.
- **Redundancy (Low):** Clean helper functions.
- **Setup complexity (Low):** `payload()` helper abstracts tar construction.

### `src/package/go_build.rs` / `src/package/go_build_test.rs`
- **Coverage gaps (Low):** 4 tests. Good coverage. Missing:
  - `parse_go_func_sigs` with complex signatures, methods, empty code
  - `go_capitalize` with empty string
  - `inject_after_imports` with grouped imports, comment-only lines
  - `invoke_go_build` and `run_go_binary` (require Go toolchain)
- **Missing negatives (Low):** No test for: `emit_go_module` with existing module files that fail to delete, `go.mod` write failure.
- **Redundancy (Low):** Clean.
- **Setup complexity (Low):** Clean with `temp_dir` helper.

### `src/commands/install.rs` / `src/commands/install_test.rs`
- **Coverage gaps (Medium):** 4 tests. Good coverage of `install_store_source`. Missing:
  - `cmd_install` entry point
  - `install_store_path`
  - `install_store_cista_path`
  - `install_store_registry_package`
  - `install_store_git_package`
- **Missing negatives (Medium):** No test for: `cmd_install` with neither `--path` nor library name, `install_store_source` with unsupported target language, `install_store_path` where canonicalize fails.
- **Redundancy (Low):** Tests are clean with `write_cista_repo`, `write_library_repo`, `write_project_with_dependency` helpers.
- **Setup complexity (Medium):** Each test creates a full temp directory tree with git repos and Faber manifests.

### Untested source files (no companion, no inline tests)
The following files have **zero dedicated tests**:

| File | Lines (est.) | Risk | Tested indirectly via |
|---|---|---|---|
| `src/cli/emit.rs` | ~60 | Low | `tests/emit_integration_test.rs` (subprocess) |
| `src/commands/archive.rs` | ~80 | Medium | `script_test.rs` (zip path) |
| `src/commands/explain.rs` | ~100 | Medium | None |
| `src/commands/host.rs` | ~70 | Low | None |
| `src/commands/init.rs` | ~50 | Low | None |
| `src/commands/targets.rs` | ~30 | Low | None |
| `src/commands/test.rs` | ~100 | Medium | `tests/run_integration_test.rs` |
| `src/explain_render.rs` | ~150 | Medium | `explain_test.rs` (rendering output) |
| `src/io_buf.rs` | 18 | Low | None (trivial) |
| `src/package/cmd.rs` | ~200 | Medium | `package_test.rs` (indirect) |
| `src/package/codegen.rs` | ~80 | Low | `package_test.rs` (indirect) |
| `src/package/discovery.rs` | ~200 | High | `package_test.rs` (indirect) |
| `src/package/dispatch.rs` | ~50 | Low | `package_test.rs` (indirect) |
| `src/package/file_interface.rs` | ~100 | Medium | `package_test.rs` (indirect) |
| `src/package/import_graph.rs` | ~100 | Medium | `package_test.rs` (indirect) |
| `src/package/library.rs` | ~80 | Medium | `package_test.rs` (indirect) |
| `src/package/manifest.rs` | ~150 | Medium | `package_test.rs` (indirect) |
| `src/package/paths.rs` | 50 | Low | 2 inline test functions |
| `src/package/product.rs` | ~100 | Medium | None |
| `src/package/reader.rs` | ~60 | Low | `package_test.rs` (indirect) |
| `src/package/source_files.rs` | ~80 | Low | `package_test.rs` (indirect) |
| `src/script/trap.rs` | 118 | Low | `script_test.rs` (indirect) |

### Integration tests (`tests/`)
- **`tests/emit_integration_test.rs`:** 18 tests. Good subprocess coverage of emit.
- **`tests/clean_install_integration_test.rs`:** 2 tests. Sparse.
- **`tests/format_integration_test.rs`:** Present (not fully analyzed).
- **`tests/run_integration_test.rs`:** Present (not fully analyzed).
- **`tests/web2_build_integration_test.rs`:** Present — tests real web2 application builds.
- **`tests/hygiene.rs`:** 1 test. Ratchets code hygiene budgets (`unwrap`, `expect`, `panic`, `todo`, etc.).
- **`tests/install_path_integration_test.rs`:** Present (not fully analyzed).

---

## Methodology notes

- Analysis performed by reading every `*_test.rs` file and its corresponding source file.
- Files without test companions were identified by matching source module names against `#[path = "..."]` directives and `#[cfg(test)] mod tests` blocks.
- "Indirectly tested" means the code path is exercised by package_test.rs or integration tests but has no dedicated unit test targeting its specific contract.
- Line counts are approximate and based on reading observed file lengths.
- This is a read-only analysis; no files were modified.
