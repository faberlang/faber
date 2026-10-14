//! Radix-owned host ABI contract table.
//!
//! This crate is the single source of truth for the host ABI surface Radix
//! emits: runtime symbol names, wire enum codes, LLVM layout spellings, status
//! codes, and diagnostic symbol rows. A published compiler binary reports this
//! table through `radix abi` without importing the runtime at compile time.
//!
//! Authority model
//! ---------------
//! The authority is the Radix binary itself (`radix abi`). Constant values
//! here match the runtime table so emitters and generated artifacts keep the
//! `__faber_rt_v1_*` spellings. Drift is caught by dev-only coherence tests,
//! not by the compiler reading the runtime back as an input.
//!
//! Individual `pub const` items are the import surface for the MIR LLVM and
//! Wasm text emitters. The paired `*_ROWS` tables are the structured view
//! consumed by the `radix abi` renderers. Both come from one declaration list
//! per family so a constant and its row cannot drift apart.

/// Host ABI version emitted by this Radix build.
pub const ABI_VERSION: u32 = 1;

/// Compiler-facing name of the published ABI surface.
pub const ABI_SURFACE_NAME: &str = "faber-rt-v1";

macro_rules! abi_string_family {
    (
        $rows:ident {
            $( $name:ident = $value:literal ),* $(,)?
        }
    ) => {
        $(
            pub const $name: &str = $value;
        )*
        /// Structured `(const_name, value)` rows, derived from the same
        /// declaration list as the individual constants above.
        pub const $rows: &[(&'static str, &'static str)] = &[
            $((stringify!($name), $value)),*
        ];
    };
}

macro_rules! abi_enum_family {
    (
        $rows:ident : type $alias:ident = $repr:ty {
            $( $name:ident = $value:literal ),* $(,)?
        }
    ) => {
        pub type $alias = $repr;
        $(
            pub const $name: $alias = $value;
        )*
        /// Structured `(const_name, code)` rows, derived from the same
        /// declaration list as the individual constants above.
        pub const $rows: &[(&'static str, u32)] = &[
            $((stringify!($name), $value)),*
        ];
    };
}

macro_rules! abi_diagnostics {
    (
        $(( $kind:literal, $carrier:literal, $symbol:literal )),* $(,)?
    ) => {
        /// Diagnostic carrier symbol rows: `(kind, carrier, runtime_symbol)`.
        pub const DIAGNOSTIC_SYMBOLS_V1: &[(&str, &str, &str)] = &[
            $(($kind, $carrier, $symbol)),*
        ];

        /// Resolve the runtime diagnostic symbol for a `(kind, carrier)` pair.
        pub fn diagnostic_symbol_v1(kind: &str, carrier: &str) -> Option<&'static str> {
            DIAGNOSTIC_SYMBOLS_V1
                .iter()
                .find(|(candidate_kind, candidate_carrier, _)| {
                    *candidate_kind == kind && *candidate_carrier == carrier
                })
                .map(|(_, _, symbol)| *symbol)
        }
    };
}

/// Reserved gradient handle type discriminator for multi-handle ABI.
///
/// Currently unused; reserved to distinguish gradient handles from other
/// opaque handle families (tensor, array, etc.) in a future unified handle
/// table.
pub const GRADIENT_HANDLE_TYPE: u32 = 0;

/// WGSL gradient handle type discriminator.
///
/// WGSL gradient handles are opaque u32 indices into a host-side
/// gradient buffer registry. Unlike LLVM gradient handles (which are
/// runtime-managed pointers), WGSL handles are mapped to `GPUBuffer`
/// instances by the WebGPU host runtime.
pub const GRADIENT_WGSL_HANDLE_TYPE: u32 = 1;

/// Opaque WGSL gradient handle.
///
/// An index into the WebGPU host's gradient buffer registry. The host
/// maps this index to a `GPUBuffer` used for host-side gradient
/// accumulation. This is a JavaScript-host concept; no `extern "C"`
/// symbols are associated with it.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
#[repr(transparent)]
pub struct GradientWgslHandle(pub u32);

abi_string_family! {
    GRADIENT_SYMBOL_ROWS {
        SYMBOL_GRADIENT_CREATE = "__faber_rt_v1_gradient_create",
        SYMBOL_GRADIENT_ACCUMULATE = "__faber_rt_v1_gradient_accumulate",
        SYMBOL_GRADIENT_READ = "__faber_rt_v1_gradient_read",
        SYMBOL_GRADIENT_ZERO = "__faber_rt_v1_gradient_zero",
    }
}

abi_string_family! {
    SYMBOL_ROWS {
        SYMBOL_INIT = "__faber_rt_v1_init",
        SYMBOL_SHUTDOWN = "__faber_rt_v1_shutdown",
        SYMBOL_WRITE_NOTA_TEXT = "__faber_rt_v1_write_nota_text",
        SYMBOL_ASSERT = "__faber_rt_v1_assert",
        SYMBOL_ASSERT_MESSAGE = "__faber_rt_v1_assert_message",
        SYMBOL_FATAL = "__faber_rt_v1_fatal",
        SYMBOL_FATAL_OPAQUE = "__faber_rt_v1_fatal_opaque",
        SYMBOL_FORMAT_I64 = "__faber_rt_v1_format_i64",
        SYMBOL_FORMAT_I1 = "__faber_rt_v1_format_i1",
        SYMBOL_FORMAT_I64_I64 = "__faber_rt_v1_format_i64_i64",
        SYMBOL_FORMAT_I64_I64_I64 = "__faber_rt_v1_format_i64_i64_i64",
        SYMBOL_FORMAT_F64 = "__faber_rt_v1_format_f64",
        // `display_fractus` keeps f32 precision (`0.1f32` renders `0.1`, not
        // the widened `0.10000000149011612` an f64 carrier would produce).
        SYMBOL_FORMAT_F32 = "__faber_rt_v1_format_f32",
        SYMBOL_FORMAT_TEXT = "__faber_rt_v1_format_text",
        SYMBOL_FORMAT_TEXT_TEXT = "__faber_rt_v1_format_text_text",
        SYMBOL_FORMAT_TEXT_I64 = "__faber_rt_v1_format_text_i64",
        SYMBOL_FORMAT_I64_TEXT = "__faber_rt_v1_format_i64_text",
        SYMBOL_FORMAT_TEXT_TEXT_TEXT = "__faber_rt_v1_format_text_text_text",
        SYMBOL_FORMAT_TEXT_I64_I1 = "__faber_rt_v1_format_text_i64_i1",
        SYMBOL_FORMAT_1_PTR_TO_PTR = "__faber_rt_v1_format_1_ptr_to_ptr",
        // One option (`T ∪ none`) argument plus its payload value kind: the
        // null-encoded option carries no kind, so the call site supplies it
        // (the format twin of `__faber_rt_v1_diagnostic_nota_option`).
        SYMBOL_FORMAT_OPTION = "__faber_rt_v1_format_option",
        SYMBOL_TEXT_LENGTH = "__faber_rt_v1_text_length",
        SYMBOL_TEXT_CONCAT = "__faber_rt_v1_text_concat",
        SYMBOL_TEXT_EQ = "__faber_rt_v1_text_eq",
        SYMBOL_TEXT_NE = "__faber_rt_v1_text_ne",
        SYMBOL_ASCII_EQ = "__faber_rt_v1_ascii_eq",
        SYMBOL_ASCII_NE = "__faber_rt_v1_ascii_ne",
        SYMBOL_TEXT_IS_EMPTY = "__faber_rt_v1_text_is_empty",
        SYMBOL_TEXT_CONTAINS = "__faber_rt_v1_text_contains",
        SYMBOL_TEXT_STARTS_WITH = "__faber_rt_v1_text_starts_with",
        SYMBOL_TEXT_ENDS_WITH = "__faber_rt_v1_text_ends_with",
        SYMBOL_TEXT_UPPERCASE = "__faber_rt_v1_text_uppercase",
        SYMBOL_TEXT_LOWERCASE = "__faber_rt_v1_text_lowercase",
        SYMBOL_TEXT_TRIM = "__faber_rt_v1_text_trim",
        SYMBOL_TEXT_SLICE = "__faber_rt_v1_text_slice",
        SYMBOL_TEXT_SPLIT = "__faber_rt_v1_text_split",
        SYMBOL_TEXT_REPLACE = "__faber_rt_v1_text_replace",
        SYMBOL_TEXT_PARSE_INTEGER = "__faber_rt_v1_text_parse_integer",
        SYMBOL_TEXT_PARSE_FLOAT = "__faber_rt_v1_text_parse_float",
        SYMBOL_TEXT_PARSE_INTEGER_OR = "__faber_rt_v1_text_parse_integer_or",
        SYMBOL_TEXT_PARSE_FLOAT_OR = "__faber_rt_v1_text_parse_float_or",
        SYMBOL_TEXT_TRUTHY = "__faber_rt_v1_text_truthy",
        SYMBOL_TEXT_I64 = "__faber_rt_v1_text_i64",
        SYMBOL_TEXT_F64 = "__faber_rt_v1_text_f64",
        SYMBOL_TEXT_I1 = "__faber_rt_v1_text_i1",
        SYMBOL_ASCII_TRUTHY = "__faber_rt_v1_ascii_truthy",
        SYMBOL_SOLUM_READ_TEXT = "__faber_rt_v1_solum_read_text",
        SYMBOL_SOLUM_READ_LINES = "__faber_rt_v1_solum_read_lines",
        SYMBOL_SOLUM_READ_BYTES = "__faber_rt_v1_solum_read_bytes",
        SYMBOL_SOLUM_WRITE_TEXT = "__faber_rt_v1_solum_write_text",
        SYMBOL_VALOR_I64 = "__faber_rt_v1_valor_i64",
        SYMBOL_VALOR_F64 = "__faber_rt_v1_valor_f64",
        SYMBOL_VALOR_I1 = "__faber_rt_v1_valor_i1",
        SYMBOL_VALOR_TEXT = "__faber_rt_v1_valor_text",
        SYMBOL_VALOR_ASCII = "__faber_rt_v1_valor_ascii",
        SYMBOL_VALOR_NIHIL = "__faber_rt_v1_valor_nihil",
        SYMBOL_VALOR_GET_I64 = "__faber_rt_v1_valor_get_i64",
        SYMBOL_VALOR_GET_F64 = "__faber_rt_v1_valor_get_f64",
        SYMBOL_VALOR_GET_I1 = "__faber_rt_v1_valor_get_i1",
        SYMBOL_VALOR_GET_TEXT = "__faber_rt_v1_valor_get_text",
        SYMBOL_VALOR_GET_ASCII = "__faber_rt_v1_valor_get_ascii",
        SYMBOL_VALOR_GET_NIHIL = "__faber_rt_v1_valor_get_nihil",
        SYMBOL_OCTETI_NEW = "__faber_rt_v1_octeti_new",
        SYMBOL_OCTETI_APPEND = "__faber_rt_v1_octeti_append",
        SYMBOL_OCTETI_GET = "__faber_rt_v1_octeti_get",
        SYMBOL_OCTETI_LENGTH = "__faber_rt_v1_octeti_length",
        SYMBOL_OCTETI_FROM_TEXT = "__faber_rt_v1_octeti_from_text",
        SYMBOL_OCTETI_FROM_ASCII = "__faber_rt_v1_octeti_from_ascii",
        SYMBOL_OCTETI_GET_TEXT = "__faber_rt_v1_octeti_get_text",
        SYMBOL_OCTETI_GET_ASCII = "__faber_rt_v1_octeti_get_ascii",
        SYMBOL_INSTANS_FROM_TEXT = "__faber_rt_v1_instans_from_text",
        SYMBOL_INSTANS_FROM_VALOR = "__faber_rt_v1_instans_from_valor",
        SYMBOL_INSTANS_RETAG = "__faber_rt_v1_instans_retag",
        SYMBOL_INSTANS_GET_TEXT = "__faber_rt_v1_instans_get_text",
        // `⊥` default for aggregate/opaque payload extraction.
        // `expr ↦ T ⊥ recovery` lowers to a fixed-signature
        // `(payload + fallback → payload)` row per closed-set conversion, so
        // the host substitutes the fallback on a missing or wrong-typed
        // payload instead of aborting. Parse `_or` rows and `option_get_or`
        // are the same closed-set mechanism; `valor_cape` is the fac/cape
        // channel, not this inline `⊥` fallback.
        SYMBOL_VALOR_GET_I64_OR = "__faber_rt_v1_valor_get_i64_or",
        SYMBOL_VALOR_GET_F64_OR = "__faber_rt_v1_valor_get_f64_or",
        SYMBOL_VALOR_GET_I1_OR = "__faber_rt_v1_valor_get_i1_or",
        SYMBOL_VALOR_GET_TEXT_OR = "__faber_rt_v1_valor_get_text_or",
        SYMBOL_VALOR_GET_ASCII_OR = "__faber_rt_v1_valor_get_ascii_or",
        SYMBOL_VALOR_GET_OCTETI_OR = "__faber_rt_v1_valor_get_octeti_or",
        SYMBOL_VALOR_GET_ARRAY_OR = "__faber_rt_v1_valor_get_array_or",
        SYMBOL_VALOR_GET_MAP_OR = "__faber_rt_v1_valor_get_map_or",
        SYMBOL_VALOR_GET_GENUS_OR = "__faber_rt_v1_valor_get_genus_or",
        SYMBOL_OCTETI_GET_TEXT_OR = "__faber_rt_v1_octeti_get_text_or",
        SYMBOL_OCTETI_GET_ASCII_OR = "__faber_rt_v1_octeti_get_ascii_or",
        SYMBOL_INSTANS_FROM_TEXT_OR = "__faber_rt_v1_instans_from_text_or",
        SYMBOL_INSTANS_FROM_VALOR_OR = "__faber_rt_v1_instans_from_valor_or",
        SYMBOL_INSTANS_DISPLAY = "__faber_rt_v1_instans_display",
        SYMBOL_NUMERUS_OVERFLOW = "__faber_rt_v1_numerus_overflow",
        SYMBOL_COMPARE_LT = "__faber_rt_v1_compare_lt_2_ptr_ptr_to_i1",
        SYMBOL_COMPARE_GT = "__faber_rt_v1_compare_gt_2_ptr_ptr_to_i1",
        SYMBOL_COMPARE_LTE = "__faber_rt_v1_compare_lte_2_ptr_ptr_to_i1",
        SYMBOL_COMPARE_GTE = "__faber_rt_v1_compare_gte_2_ptr_ptr_to_i1",
        SYMBOL_COMPARE_EQ = "__faber_rt_v1_compare_eq_2_ptr_ptr_to_i1",
        SYMBOL_COMPARE_NE = "__faber_rt_v1_compare_ne_2_ptr_ptr_to_i1",
        SYMBOL_CONVERT_RUNTIME_PTR = "__faber_rt_v1_convert_runtime_1_ptr_to_ptr",
        SYMBOL_TEMPUS_NUNC = "__faber_rt_v1_tempus_nunc",
        // Async tempus wait (`dormiet`/`expectet`). The LLVM lane inlines a
        // `poll` sleep; the wasm module cannot syscall, so the wait is a
        // closed-set host row taking the millisecond duration (`numerus` i64)
        // and returning the vacuum payload. The wasm host sleeps; the stub
        // host default-values the import (instant wait).
        SYMBOL_TEMPUS_WAIT = "__faber_rt_v1_tempus_wait",
        SYMBOL_TOML_SOLVE = "__faber_rt_v1_toml_solve",
        SYMBOL_VALOR_CAPE = "__faber_rt_v1_valor_cape",
        SYMBOL_JSON_PANGE = "__faber_rt_v1_json_pange",
        SYMBOL_JSON_SOLVE = "__faber_rt_v1_json_solve",
        SYMBOL_JSON_TEMPTA = "__faber_rt_v1_json_tempta",
        SYMBOL_VALOR_OCTETI = "__faber_rt_v1_valor_octeti",
        SYMBOL_VALOR_ARRAY = "__faber_rt_v1_valor_array",
        SYMBOL_VALOR_MAP = "__faber_rt_v1_valor_map",
        SYMBOL_VALOR_GET_OCTETI = "__faber_rt_v1_valor_get_octeti",
        SYMBOL_VALOR_GET_ARRAY = "__faber_rt_v1_valor_get_array",
        SYMBOL_VALOR_GET_MAP = "__faber_rt_v1_valor_get_map",
        // Closed genus field-layout contract; see `GENUS_FIELD_POLICY_*`.
        SYMBOL_VALOR_GENUS = "__faber_rt_v1_valor_genus",
        SYMBOL_VALOR_GET_GENUS = "__faber_rt_v1_valor_get_genus",
        SYMBOL_ARRAY_NEW = "__faber_rt_v1_array_new",
        SYMBOL_ARRAY_PUSH = "__faber_rt_v1_array_push",
        SYMBOL_ARRAY_EXTEND = "__faber_rt_v1_array_extend",
        SYMBOL_ARRAY_LENGTH = "__faber_rt_v1_array_length",
        SYMBOL_ARRAY_GET = "__faber_rt_v1_array_get",
        SYMBOL_ARRAY_SET = "__faber_rt_v1_array_set",
        SYMBOL_ARRAY_CLONE = "__faber_rt_v1_array_clone",
        SYMBOL_ARRAY_CONTAINS = "__faber_rt_v1_array_contains",
        SYMBOL_ARRAY_IS_EMPTY = "__faber_rt_v1_array_is_empty",
        SYMBOL_ARRAY_REVERSE = "__faber_rt_v1_array_reverse",
        SYMBOL_ARRAY_RANGE = "__faber_rt_v1_array_range",
        SYMBOL_ARRAY_OPTION = "__faber_rt_v1_array_option",
        SYMBOL_ARRAY_SORT = "__faber_rt_v1_array_sort",
        SYMBOL_ARRAY_SORT_TEXT = "__faber_rt_v1_array_sort_text",
        SYMBOL_ARRAY_SORT_BY_KEYS = "__faber_rt_v1_array_sort_by_keys",
        SYMBOL_ARRAY_SUM = "__faber_rt_v1_array_sum",
        // Process `argumenta`: captured argv excluding host argv[0], returned
        // as a `lista<textus>` handle for `incipit argumenta`.
        SYMBOL_ARGUMENTS = "__faber_rt_v1_arguments",
        SYMBOL_OPTION_NONE = "__faber_rt_v1_option_none",
        SYMBOL_OPTION_SOME = "__faber_rt_v1_option_some",
        SYMBOL_OPTION_IS_PRESENT = "__faber_rt_v1_option_is_present",
        SYMBOL_OPTION_GET = "__faber_rt_v1_option_get",
        SYMBOL_OPTION_GET_OR = "__faber_rt_v1_option_get_or",
        SYMBOL_OPTION_UNWRAP_PTR = "__faber_rt_v1_option_unwrap_ptr",
        SYMBOL_MAP_NEW = "__faber_rt_v1_map_new",
        SYMBOL_MAP_PUT = "__faber_rt_v1_map_put",
        SYMBOL_MAP_OPTION = "__faber_rt_v1_map_option",
        SYMBOL_MAP_GET = "__faber_rt_v1_map_get",
        SYMBOL_MAP_CONTAINS = "__faber_rt_v1_map_contains",
        SYMBOL_MAP_DELETE = "__faber_rt_v1_map_delete",
        SYMBOL_MAP_LENGTH = "__faber_rt_v1_map_length",
        SYMBOL_MAP_IS_EMPTY = "__faber_rt_v1_map_is_empty",
        SYMBOL_MAP_KEYS = "__faber_rt_v1_map_keys",
        SYMBOL_MAP_VALUES = "__faber_rt_v1_map_values",
        SYMBOL_AGGREGATE_SET_INDEX_PTR_I64 = "__faber_rt_v1_aggregate_set_index_ptr_i64",
        SYMBOL_VALOR_TENSOR = "__faber_rt_v1_valor_tensor",
        SYMBOL_SET_NEW = "__faber_rt_v1_set_new",
        SYMBOL_SET_ADD = "__faber_rt_v1_set_add",
        SYMBOL_SET_CONTAINS = "__faber_rt_v1_set_contains",
        SYMBOL_SET_DELETE = "__faber_rt_v1_set_delete",
        SYMBOL_SET_LENGTH = "__faber_rt_v1_set_length",
        SYMBOL_SET_IS_EMPTY = "__faber_rt_v1_set_is_empty",
        SYMBOL_SET_FROM_ARRAY = "__faber_rt_v1_set_from_array",
        SYMBOL_ARRAY_FROM_SET = "__faber_rt_v1_array_from_set",
        SYMBOL_SET_UNION = "__faber_rt_v1_set_union",
        SYMBOL_SET_INTERSECTION = "__faber_rt_v1_set_intersection",
        SYMBOL_SET_DIFFERENCE = "__faber_rt_v1_set_difference",
        SYMBOL_SET_SYMMETRIC_DIFFERENCE = "__faber_rt_v1_set_symmetric_difference",
        SYMBOL_SET_IS_SUBSET = "__faber_rt_v1_set_is_subset",
        SYMBOL_SET_IS_SUPERSET = "__faber_rt_v1_set_is_superset",
        SYMBOL_TENSOR_NEW = "__faber_rt_v1_tensor_new",
        SYMBOL_TENSOR_CREATE = "__faber_rt_v1_tensor_create",
        SYMBOL_TENSOR_FROM_FLAT = "__faber_rt_v1_tensor_from_flat",
        SYMBOL_TENSOR_RANK = "__faber_rt_v1_tensor_rank",
        SYMBOL_TENSOR_SHAPE = "__faber_rt_v1_tensor_shape",
        SYMBOL_TENSOR_RESHAPE = "__faber_rt_v1_tensor_reshape",
        SYMBOL_TENSOR_GET = "__faber_rt_v1_tensor_get",
        SYMBOL_TENSOR_SET = "__faber_rt_v1_tensor_set",
        SYMBOL_TENSOR_FILL = "__faber_rt_v1_tensor_fill",
        SYMBOL_TENSOR_FLATTEN = "__faber_rt_v1_tensor_flatten",
        SYMBOL_TENSOR_MATERIALIZE = "__faber_rt_v1_tensor_materialize",
        SYMBOL_TENSOR_SLICE = "__faber_rt_v1_tensor_slice",
        SYMBOL_TENSOR_ADD = "__faber_rt_v1_tensor_add",
        SYMBOL_TENSOR_SUB = "__faber_rt_v1_tensor_sub",
        SYMBOL_TENSOR_MUL = "__faber_rt_v1_tensor_mul",
        SYMBOL_TENSOR_MATMUL = "__faber_rt_v1_tensor_matmul",
        SYMBOL_TENSOR_SUM = "__faber_rt_v1_tensor_sum",
        SYMBOL_TENSOR_MEAN = "__faber_rt_v1_tensor_mean",
        SYMBOL_TENSOR_MAX = "__faber_rt_v1_tensor_max",
        SYMBOL_TENSOR_MIN = "__faber_rt_v1_tensor_min",
        SYMBOL_TENSOR_CONVERT = "__faber_rt_v1_tensor_convert",
        SYMBOL_SPARSE_NEW = "__faber_rt_v1_sparse_new",
        SYMBOL_SPARSE_GET = "__faber_rt_v1_sparse_get",
        SYMBOL_SPARSE_SET = "__faber_rt_v1_sparse_set",
        SYMBOL_SPARSE_NONZERO = "__faber_rt_v1_sparse_nonzero",
        SYMBOL_SPARSE_RANK = "__faber_rt_v1_sparse_rank",
        SYMBOL_SPARSE_DENSIFY = "__faber_rt_v1_sparse_densify",
        SYMBOL_SPARSE_FROM_TENSOR = "__faber_rt_v1_sparse_from_tensor",
        SYMBOL_REGEX_FROM_TEXT = "__faber_rt_v1_regex_from_text",
        SYMBOL_REGEX_FROM_ASCII = "__faber_rt_v1_regex_from_ascii",
        SYMBOL_REGEX_GET_TEXT = "__faber_rt_v1_regex_get_text",
        SYMBOL_REGEX_LITERAL_1_PTR_TO_PTR = "__faber_rt_v1_regex_literal_1_ptr_to_ptr",
        // Regex operations (RX-10d). Every row takes the runtime context
        // first. `matches` and the `match_start`/`match_end` accessors write
        // their scalar through a trailing out pointer and return a status; the
        // rest return a `%FaberRtPtrResultV1` whose value is a text handle, a
        // match handle or an array handle, or null for `none` (a miss, a group
        // that did not take part, an unknown name). Offsets are code points.
        // The Wasm host (RX-10e) binds the same operations through its own
        // import signatures: the regex carrier and the match value are opaque
        // handles, a find or group result is an `option` handle, a bulk
        // result is a `lista` handle. Every operation is one symbol shared by
        // both targets, named for its receiver family: `regex_*` for the
        // regex value, `text_escape` (a method on `textus`) and `match_*` for
        // the match value.
        SYMBOL_REGEX_MATCHES = "__faber_rt_v1_regex_matches",
        SYMBOL_REGEX_FIND = "__faber_rt_v1_regex_find",
        SYMBOL_REGEX_FIND_ALL = "__faber_rt_v1_regex_find_all",
        SYMBOL_REGEX_SPLIT = "__faber_rt_v1_regex_split",
        SYMBOL_REGEX_REPLACE = "__faber_rt_v1_regex_replace",
        SYMBOL_REGEX_REPLACE_EACH = "__faber_rt_v1_regex_replace_each",
        SYMBOL_TEXT_ESCAPE = "__faber_rt_v1_text_escape",
        SYMBOL_MATCH_TEXT = "__faber_rt_v1_match_text",
        SYMBOL_MATCH_START = "__faber_rt_v1_match_start",
        SYMBOL_MATCH_END = "__faber_rt_v1_match_end",
        SYMBOL_MATCH_GROUP = "__faber_rt_v1_match_group",
        SYMBOL_MATCH_NAMED = "__faber_rt_v1_match_named",
        SYMBOL_READ_LINE_0_TO_PTR = "__faber_rt_v1_read_line_0_to_ptr",
        // Shared cursor-stream materialization. One fixed symbol; the
        // callee-encoded `cursor_stream_{callee}` LLVM dialect is retired.
        // The host runs the generator to completion and collects `cede`
        // yields into a `lista<T>` (`eval_cursor_stream` is the reference
        // semantics).
        SYMBOL_CURSOR_STREAM = "__faber_rt_v1_cursor_stream",
        SYMBOL_INTERVAL_NEW = "__faber_rt_v1_interval_new",
        SYMBOL_INTERVAL_INTERSECT = "__faber_rt_v1_interval_intersect",
        SYMBOL_INTERVAL_UNION = "__faber_rt_v1_interval_union",
        SYMBOL_INTERVAL_LENGTH = "__faber_rt_v1_interval_length",
        SYMBOL_INTERVAL_CONTAINS = "__faber_rt_v1_interval_contains",
        SYMBOL_INTERVAL_CLAMP_I64 = "__faber_rt_v1_interval_clamp_i64",
        SYMBOL_INTERVAL_CLAMP = "__faber_rt_v1_interval_clamp",
        SYMBOL_INTERVAL_MATERIALIZE_ARRAY = "__faber_rt_v1_interval_materialize_array",
        SYMBOL_INTERVAL_MATERIALIZE_TENSOR = "__faber_rt_v1_interval_materialize_tensor",
        SYMBOL_CLI_PARSE = "__faber_rt_v1_cli_parse",
        SYMBOL_CLI_TABLE = "__faber_rt_v1_cli_table",
        SYMBOL_CLI_FIELD_PTR = "__faber_rt_v1_cli_field_ptr",
        SYMBOL_CLI_FIELD_I64 = "__faber_rt_v1_cli_field_i64",
        SYMBOL_CLI_FIELD_F64 = "__faber_rt_v1_cli_field_f64",
        SYMBOL_CLI_FIELD_I1 = "__faber_rt_v1_cli_field_i1",
        SYMBOL_CLI_SELECTED_COMMAND = "__faber_rt_v1_cli_selected_command",
        SYMBOL_CLI_EXIT_CODE = "__faber_rt_v1_cli_exit_code",
        // Shared failable error-carrier row. One fixed symbol for the
        // `ReturnError` payload channel — the host reads the typed error
        // payload from a failable `(status, payload)` result (`cape err`).
        // The LLVM per-type `__faber_runtime_return_error_{ty}` dialect is
        // retired.
        SYMBOL_FALLIBLE_ERROR = "__faber_rt_v1_fallible_error",
        SYMBOL_PROGRAM_ENTRY = "__faber_program_entry_v1",
        SYMBOL_GPU_COPY_IN = "__faber_gpu_v1_copy_in",
        SYMBOL_GPU_READBACK = "__faber_gpu_v1_readback",
        SYMBOL_GPU_SYNC = "__faber_gpu_v1_sync",
        // Shared sermo materialization. `ad 'route'(payload)` opens a stream
        // (route + valor → opaque handle); `SermoSetOpener` replaces the
        // first request frame's payload before consume; `↦ textus` /
        // `↦ valor` materialize inbound frames; the `_or` row is the `⊥`
        // recovery for stream extraction.
        SYMBOL_SERMO_OPEN = "__faber_rt_v1_sermo_open",
        SYMBOL_SERMO_SET_OPENER = "__faber_rt_v1_sermo_set_opener",
        SYMBOL_SERMO_MATERIALIZE_TEXT = "__faber_rt_v1_sermo_materialize_text",
        SYMBOL_SERMO_MATERIALIZE_VALOR = "__faber_rt_v1_sermo_materialize_valor",
        SYMBOL_SERMO_MATERIALIZE_I64_OR = "__faber_rt_v1_sermo_materialize_i64_or",
    }
}

// Genus field-layout contract for `valor_get_genus`.
//
// `valor ↦ genus` crosses the host through one named-field descriptor table
// with parallel arrays in declaration order: UTF-8 names, `VALUE_KIND_*`
// codes, these policy flags, and pre-seeded typed output slots.
// `GENUS_FIELD_POLICY_MANDATORY` (`0`): the key must be present in the
// `Valor::Tabula` payload — a missing key fails the whole extraction
// (non-OK status, latched for `⊥` default).
// `GENUS_FIELD_POLICY_DEFAULTABLE` (`1`): the host retains the pre-seeded
// slot when the key is missing. The seed is the emitter-supplied
// construction default (declared default, or the sponte zero/nihil seed
// for optional fields). Extra JSON keys are ignored.
// `__faber_rt_v1_valor_genus` boxes the same table shape.
abi_enum_family! {
    GENUS_FIELD_POLICY_ROWS: type FaberRtGenusFieldPolicyV1 = u8 {
        GENUS_FIELD_POLICY_MANDATORY = 0,
        GENUS_FIELD_POLICY_DEFAULTABLE = 1,
    }
}

abi_string_family! {
    LLVM_TYPE_ROWS {
        LLVM_SLICE_TYPE = "%FaberRtSliceV1",
        LLVM_SLICE_TYPE_DEFINITION = "%FaberRtSliceV1 = type { ptr, i64 }",
        LLVM_EXIT_TYPE = "%FaberRtExitV1",
        // The exit struct crosses the C boundary as a single register.
        // LLVM's C-convention lowering of `{ i32, i32 }` splits the fields
        // across w0/w1 on AArch64, while the rustc/clang caller packs both
        // into x0. The named type is an `i64` carrying
        // `process_code | (status.code << 32)`.
        LLVM_EXIT_TYPE_DEFINITION = "%FaberRtExitV1 = type i64",
        LLVM_PTR_RESULT_TYPE = "%FaberRtPtrResultV1",
        LLVM_PTR_RESULT_TYPE_DEFINITION = "%FaberRtPtrResultV1 = type { i32, ptr }",
        LLVM_STATUS_TYPE = "%FaberRtStatusV1",
        LLVM_STATUS_TYPE_DEFINITION = "%FaberRtStatusV1 = type { i32 }",
    }
}

abi_enum_family! {
    VALUE_KIND_ROWS: type FaberRtValueKindV1 = u32 {
        VALUE_KIND_I1 = 1,
        VALUE_KIND_I8 = 2,
        VALUE_KIND_I32 = 3,
        VALUE_KIND_I64 = 4,
        VALUE_KIND_F32 = 5,
        VALUE_KIND_F64 = 6,
        VALUE_KIND_PTR = 7,
        VALUE_KIND_I16 = 8,
        VALUE_KIND_U8 = 9,
        VALUE_KIND_U16 = 10,
        VALUE_KIND_U32 = 11,
        VALUE_KIND_U64 = 12,
        VALUE_KIND_F16 = 13,
        VALUE_KIND_TEXT = 14,
        VALUE_KIND_VALOR = 15,
        VALUE_KIND_OPTION_I64 = 16,
        VALUE_KIND_INSTANS = 17,
        VALUE_KIND_ASCII = 18,
    }
}

abi_enum_family! {
    INSTANS_PRECISION_ROWS: type FaberRtInstansPrecisionV1 = u32 {
        INSTANS_PRECISION_SECONDS = 0,
        INSTANS_PRECISION_MILLIS = 1,
        INSTANS_PRECISION_MICROS = 2,
        INSTANS_PRECISION_NANOS = 3,
    }
}

abi_enum_family! {
    ARRAY_RANGE_MODE_ROWS: type FaberRtArrayRangeModeV1 = u32 {
        ARRAY_RANGE_SLICE = 1,
        ARRAY_RANGE_TAKE = 2,
        ARRAY_RANGE_TAKE_LAST = 3,
        ARRAY_RANGE_DROP_FIRST = 4,
    }
}

abi_enum_family! {
    ARRAY_OPTION_MODE_ROWS: type FaberRtArrayOptionModeV1 = u32 {
        ARRAY_OPTION_INDEX = 1,
        ARRAY_OPTION_FIRST = 2,
        ARRAY_OPTION_LAST = 3,
        ARRAY_OPTION_REMOVE_FIRST = 4,
        ARRAY_OPTION_REMOVE_LAST = 5,
    }
}

/// Status code rows: `(const_name, code)`. The runtime wraps these codes in a
/// `FaberRtStatusV1 { code }` struct; Radix owns the code values, the runtime
/// owns the struct layout.
pub const STATUS_CODES: &[(&str, i32)] = &[
    ("STATUS_OK", 0),
    ("STATUS_INVALID_ARGUMENT", 1),
    ("STATUS_IO_ERROR", 2),
    ("STATUS_PANIC", 3),
    ("STATUS_UNSUPPORTED", 4),
    // Failable error-channel discriminator. Status-first multi-value
    // `(i32 status, payload…)`: `STATUS_OK` + success payload on the happy
    // path, `STATUS_FALLIBLE` + typed `ReturnError` payload when the
    // failable channel fires (`iace` throw / `⇥` propagation / `fac`/`cape`
    // recovery).
    ("STATUS_FALLIBLE", 5),
];

abi_diagnostics! {
    ("nota", "ptr", "__faber_rt_v1_diagnostic_nota_ptr"),
    ("nota", "text", "__faber_rt_v1_diagnostic_nota_text"),
    ("nota", "ascii", "__faber_rt_v1_diagnostic_nota_ascii"),
    ("nota", "i64", "__faber_rt_v1_diagnostic_nota_i64"),
    ("nota", "u64", "__faber_rt_v1_diagnostic_nota_u64"),
    ("nota", "i1", "__faber_rt_v1_diagnostic_nota_i1"),
    ("nota", "float", "__faber_rt_v1_diagnostic_nota_f32"),
    ("nota", "double", "__faber_rt_v1_diagnostic_nota_f64"),
    ("nota", "i8", "__faber_rt_v1_diagnostic_nota_i8"),
    ("nota", "i32", "__faber_rt_v1_diagnostic_nota_i32"),
    ("mone", "ptr", "__faber_rt_v1_diagnostic_mone_ptr"),
    ("mone", "text", "__faber_rt_v1_diagnostic_mone_text"),
    ("mone", "ascii", "__faber_rt_v1_diagnostic_mone_ascii"),
    ("mone", "i64", "__faber_rt_v1_diagnostic_mone_i64"),
    ("scribe", "ptr", "__faber_rt_v1_diagnostic_scribe_ptr"),
    ("scribe", "text", "__faber_rt_v1_diagnostic_scribe_text"),
    ("scribe", "i64", "__faber_rt_v1_diagnostic_scribe_i64"),
    ("scribe", "i32", "__faber_rt_v1_diagnostic_scribe_i32"),
    ("scribe", "double", "__faber_rt_v1_diagnostic_scribe_f64"),
    ("vide", "ptr", "__faber_rt_v1_diagnostic_vide_ptr"),
    ("vide", "text", "__faber_rt_v1_diagnostic_vide_text"),
    ("vide", "ascii", "__faber_rt_v1_diagnostic_vide_ascii"),
    ("vide", "i64", "__faber_rt_v1_diagnostic_vide_i64"),
}

#[cfg(test)]
#[path = "lib_test.rs"]
mod lib_test;
