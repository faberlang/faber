//! Identifier mangling for C++-family device source (MSL, CUDA C++).
//!
//! A user-chosen name (a parameter, a local, a kernel entry) reaches device
//! source verbatim after [`crate::sanitize_ident`], so a name that is a C++
//! keyword, an MSL type or address space, a builtin function the emitters
//! call, or one of the names the emitters generate themselves (`t3`, `l5`,
//! `id`, `u9_fault`, ...) would break the compile or capture a generated
//! value. [`mangle_device_ident`] is the one policy: such a name is prefixed
//! with [`MANGLE_PREFIX`], and so is any name that already starts with the
//! prefix. The map is deterministic and injective: every name that is left
//! alone does not start with the prefix, and every mangled name does (the
//! original is the text after it).

/// The prefix of a mangled identifier.
pub const MANGLE_PREFIX: &str = "kw_";

/// C++14 keywords plus the MSL type, address-space, attribute and namespace
/// words (sorted; searched by binary search).
const RESERVED_WORDS: &[&str] = &[
    "alignas",
    "alignof",
    "and",
    "and_eq",
    "array",
    "asm",
    "atomic",
    "atomic_bool",
    "atomic_int",
    "atomic_uint",
    "auto",
    "bfloat",
    "bitand",
    "bitor",
    "bool",
    "break",
    "buffer",
    "case",
    "catch",
    "char",
    "char16_t",
    "char32_t",
    "class",
    "compl",
    "const",
    "const_cast",
    "constant",
    "constexpr",
    "continue",
    "decltype",
    "default",
    "delete",
    "depth2d",
    "device",
    "discard_fragment",
    "do",
    "double",
    "dynamic_cast",
    "else",
    "enum",
    "explicit",
    "export",
    "extern",
    "false",
    "float",
    "for",
    "fragment",
    "friend",
    "goto",
    "half",
    "if",
    "inline",
    "int",
    "int16_t",
    "int32_t",
    "int64_t",
    "int8_t",
    "kernel",
    "long",
    "main",
    "mesh",
    "metal",
    "mutable",
    "namespace",
    "new",
    "noexcept",
    "not",
    "not_eq",
    "nullptr",
    "object",
    "operator",
    "or",
    "or_eq",
    "packed_float3",
    "private",
    "protected",
    "ptrdiff_t",
    "public",
    "ray_data",
    "register",
    "reinterpret_cast",
    "return",
    "sampler",
    "short",
    "signed",
    "simdgroup",
    "size_t",
    "sizeof",
    "static",
    "static_assert",
    "static_cast",
    "struct",
    "switch",
    "template",
    "texture",
    "texture1d",
    "texture2d",
    "texture3d",
    "texturecube",
    "this",
    "thread",
    "thread_local",
    "threadgroup",
    "threadgroup_imageblock",
    "throw",
    "true",
    "try",
    "typedef",
    "typeid",
    "typename",
    "uchar",
    "uint",
    "uint16_t",
    "uint32_t",
    "uint64_t",
    "uint8_t",
    "ulong",
    "union",
    "unsigned",
    "ushort",
    "using",
    "vertex",
    "virtual",
    "void",
    "volatile",
    "wchar_t",
    "while",
    "xor",
    "xor_eq",
];

/// Builtin math and select functions the device emitters call by bare name
/// (`using namespace metal`): a local of the same name would hide the function
/// at the call (sorted).
const BUILTIN_FUNCTIONS: &[&str] = &[
    "abs", "ceil", "clamp", "cos", "exp", "fabs", "floor", "fma", "fmod", "log", "max", "min",
    "pow", "rsqrt", "select", "sign", "sin", "sqrt", "tanh", "trunc",
];

/// Names the emitters generate for their own builtins and bookkeeping (sorted).
const GENERATED_WORDS: &[&str] = &[
    "gid",
    "id",
    "lane",
    "lid",
    "local_id",
    "tid",
    "workgroup_id",
    "workgroup_size",
];

/// The scalar type words that take a vector (`float4`) or, for `half` and
/// `float`, a matrix (`float4x4`) suffix.
const VECTOR_BASES: &[&str] = &[
    "bfloat", "bool", "char", "float", "half", "int", "long", "short", "uchar", "uint", "ulong",
    "ushort",
];

fn is_dim(byte: u8) -> bool {
    (b'2'..=b'4').contains(&byte)
}

/// `float4`, `uint2`, `half3x3`, ... (a scalar type word plus a 2-4 vector
/// width or a 2-4 by 2-4 matrix shape).
fn is_vector_or_matrix_type(name: &str) -> bool {
    VECTOR_BASES.iter().any(|base| {
        name.strip_prefix(base)
            .is_some_and(|rest| match rest.as_bytes() {
                [w] => is_dim(*w),
                [c, b'x', r] => is_dim(*c) && is_dim(*r),
                _ => false,
            })
    })
}

/// `t3`, `l5`, `p0`: the generated temp, local and fallback-parameter names.
fn is_generated_slot(name: &str) -> bool {
    let Some(rest) = name
        .strip_prefix('t')
        .or_else(|| name.strip_prefix('l'))
        .or_else(|| name.strip_prefix('p'))
    else {
        return false;
    };
    !rest.is_empty() && rest.bytes().all(|byte| byte.is_ascii_digit())
}

/// Whether `name` must be mangled before it reaches device source.
#[must_use]
pub fn is_device_reserved(name: &str) -> bool {
    name.starts_with(MANGLE_PREFIX)
        || name.starts_with("faber_")
        || name.starts_with("u9_")
        || RESERVED_WORDS.binary_search(&name).is_ok()
        || BUILTIN_FUNCTIONS.binary_search(&name).is_ok()
        || GENERATED_WORDS.binary_search(&name).is_ok()
        || is_vector_or_matrix_type(name)
        || is_generated_slot(name)
}

/// Mangle one already-sanitized identifier for device source: unchanged unless
/// [`is_device_reserved`], else prefixed with [`MANGLE_PREFIX`].
#[must_use]
pub fn mangle_device_ident(name: &str) -> String {
    if is_device_reserved(name) {
        format!("{MANGLE_PREFIX}{name}")
    } else {
        name.to_owned()
    }
}

#[cfg(test)]
#[path = "device_ident_test.rs"]
mod tests;
