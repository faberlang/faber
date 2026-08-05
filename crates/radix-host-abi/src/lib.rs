//! Radix-owned host ABI contract table.
//!
//! This module is the single source of truth for the host ABI surface Radix
//! emits: runtime symbol names, wire enum codes, LLVM layout spellings, status
//! codes, and diagnostic symbol rows. The historical home for these constants
//! was `faber-runtime/src/host_abi.rs`; Radix now owns them so a published
//! compiler binary can report its own ABI through `radix abi` without importing
//! the runtime at compile time.
//!
//! Authority model
//! ---------------
//! The authority is the Radix binary itself (`radix abi`). The constant values
//! here intentionally mirror the runtime table so that emitters and generated
//! artifacts keep producing the same `__faber_rt_v1_*` spellings; drift between
//! this table and the runtime is caught by dev-only coherence tests, not by the
//! compiler reading the runtime back as an input.
//!
//! The individual `pub const` items remain the import surface for the MIR LLVM
//! and Wasm text emitters (see Phase 2 of the `radix-abi-contract` goal). The
//! paired `*_ROWS` tables are the structured view consumed by the `radix abi`
//! renderers. Both are produced from one declaration list per family so the
//! constant and its row entry can never drift apart.

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
        SYMBOL_FORMAT_TEXT = "__faber_rt_v1_format_text",
        SYMBOL_FORMAT_TEXT_TEXT = "__faber_rt_v1_format_text_text",
        SYMBOL_FORMAT_TEXT_I64 = "__faber_rt_v1_format_text_i64",
        SYMBOL_FORMAT_I64_TEXT = "__faber_rt_v1_format_i64_text",
        SYMBOL_FORMAT_TEXT_TEXT_TEXT = "__faber_rt_v1_format_text_text_text",
        SYMBOL_FORMAT_TEXT_I64_I1 = "__faber_rt_v1_format_text_i64_i1",
        SYMBOL_TEXT_LENGTH = "__faber_rt_v1_text_length",
        SYMBOL_TEXT_CONCAT = "__faber_rt_v1_text_concat",
        SYMBOL_TEXT_EQ = "__faber_rt_v1_text_eq",
        SYMBOL_TEXT_NE = "__faber_rt_v1_text_ne",
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
        SYMBOL_VALOR_OCTETI = "__faber_rt_v1_valor_octeti",
        SYMBOL_VALOR_ARRAY = "__faber_rt_v1_valor_array",
        SYMBOL_VALOR_MAP = "__faber_rt_v1_valor_map",
        SYMBOL_VALOR_GET_OCTETI = "__faber_rt_v1_valor_get_octeti",
        SYMBOL_VALOR_GET_ARRAY = "__faber_rt_v1_valor_get_array",
        SYMBOL_VALOR_GET_MAP = "__faber_rt_v1_valor_get_map",
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
        SYMBOL_ARRAY_SUM = "__faber_rt_v1_array_sum",
        SYMBOL_OPTION_NONE = "__faber_rt_v1_option_none",
        SYMBOL_OPTION_SOME = "__faber_rt_v1_option_some",
        SYMBOL_OPTION_IS_PRESENT = "__faber_rt_v1_option_is_present",
        SYMBOL_OPTION_GET = "__faber_rt_v1_option_get",
        SYMBOL_OPTION_GET_OR = "__faber_rt_v1_option_get_or",
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
        SYMBOL_INTERVAL_NEW = "__faber_rt_v1_interval_new",
        SYMBOL_INTERVAL_INTERSECT = "__faber_rt_v1_interval_intersect",
        SYMBOL_INTERVAL_UNION = "__faber_rt_v1_interval_union",
        SYMBOL_INTERVAL_LENGTH = "__faber_rt_v1_interval_length",
        SYMBOL_INTERVAL_CONTAINS = "__faber_rt_v1_interval_contains",
        SYMBOL_INTERVAL_CLAMP_I64 = "__faber_rt_v1_interval_clamp_i64",
        SYMBOL_INTERVAL_CLAMP = "__faber_rt_v1_interval_clamp",
        SYMBOL_INTERVAL_MATERIALIZE_ARRAY = "__faber_rt_v1_interval_materialize_array",
        SYMBOL_INTERVAL_MATERIALIZE_TENSOR = "__faber_rt_v1_interval_materialize_tensor",
        SYMBOL_PROGRAM_ENTRY = "__faber_program_entry_v1",
        SYMBOL_GPU_COPY_IN = "__faber_gpu_v1_copy_in",
        SYMBOL_GPU_READBACK = "__faber_gpu_v1_readback",
        SYMBOL_GPU_SYNC = "__faber_gpu_v1_sync",
    }
}

abi_string_family! {
    LLVM_TYPE_ROWS {
        LLVM_SLICE_TYPE = "%FaberRtSliceV1",
        LLVM_SLICE_TYPE_DEFINITION = "%FaberRtSliceV1 = type { ptr, i64 }",
        LLVM_EXIT_TYPE = "%FaberRtExitV1",
        LLVM_EXIT_TYPE_DEFINITION = "%FaberRtExitV1 = type { i32, i32 }",
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
];

abi_diagnostics! {
    ("nota", "ptr", "__faber_rt_v1_diagnostic_nota_ptr"),
    ("nota", "text", "__faber_rt_v1_diagnostic_nota_text"),
    ("nota", "ascii", "__faber_rt_v1_diagnostic_nota_ascii"),
    ("nota", "i64", "__faber_rt_v1_diagnostic_nota_i64"),
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
