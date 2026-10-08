use super::*;

#[test]
fn word_tables_are_sorted_for_binary_search() {
    for table in [RESERVED_WORDS, BUILTIN_FUNCTIONS, GENERATED_WORDS] {
        assert!(table.windows(2).all(|pair| pair[0] < pair[1]));
    }
}

#[test]
fn keywords_types_and_address_spaces_are_mangled() {
    for word in [
        "half",
        "float",
        "kernel",
        "device",
        "constant",
        "thread",
        "threadgroup",
        "int",
        "uint",
        "bool",
        "struct",
        "namespace",
        "using",
        "template",
        "vertex",
        "fragment",
        "texture",
        "sampler",
        "float4",
        "half3x3",
        "uint2",
        "long",
    ] {
        assert_eq!(mangle_device_ident(word), format!("kw_{word}"), "{word}");
    }
}

#[test]
fn builtin_functions_and_generated_names_are_mangled() {
    for word in [
        "max",
        "sqrt",
        "exp",
        "t0",
        "l12",
        "p3",
        "id",
        "lane",
        "u9_fault",
        "faber_scalar_4",
    ] {
        assert_eq!(mangle_device_ident(word), format!("kw_{word}"), "{word}");
    }
}

#[test]
fn ordinary_names_are_unchanged() {
    for word in [
        "x",
        "src",
        "dst",
        "rms_stats",
        "half_dim",
        "float_",
        "t",
        "l_a",
        "p1x",
        "float5",
    ] {
        assert_eq!(mangle_device_ident(word), word, "{word}");
    }
}

#[test]
fn mangling_is_injective_and_never_lands_on_a_reserved_word() {
    // A name that already carries the prefix is mangled again, so the image of
    // the mangled names and the untouched names never overlap.
    assert_eq!(mangle_device_ident("kw_half"), "kw_kw_half");
    assert_ne!(mangle_device_ident("half"), mangle_device_ident("kw_half"));
    assert_eq!(mangle_device_ident("kw_x"), "kw_kw_x");
    for word in ["half", "kernel", "t0", "kw_half"] {
        let mangled = mangle_device_ident(word);
        assert_eq!(mangled.strip_prefix(MANGLE_PREFIX), Some(word));
    }
}
