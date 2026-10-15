//! Tests for the intrinsic registration table.
//! Proves: rows are unique per (receiver, name, arity); tabula, intervallum,
//! lista, sparsa, textus, and atomic receivers resolve correctly; higher-order
//! and collection-op classifications match.

use super::{INTRINSICS, IntrinsicMirKind, IntrinsicReceiver};
use crate::driver::{AnalyzeRequest, Config, Session, analyze_source};
use crate::intrinsics::{
    is_atomic_operator_method, lookup_method, lookup_method_for_type, mir_collection_op, receivers_for_type,
};
use crate::semantic::{NumericWidth, Primitive, Type, TypeTable};
use radix_types::MirAtomicOp;
use radix_types::MirCollectionOp;

#[test]
fn registry_rows_are_unique_per_receiver_name_and_arity() {
    let mut seen = std::collections::HashSet::new();
    for entry in INTRINSICS {
        let key = (entry.receiver, entry.faber_name, entry.arg_count);
        assert!(seen.insert(key), "duplicate intrinsic row: {key:?}");
    }
}

#[test]
fn promoted_tabula_methods_share_mir_ops_with_registry() {
    let mut types = TypeTable::new();
    let map_ty = types.map(types.primitive(Primitive::Textus), types.primitive(Primitive::Numerus));
    let receiver = types.get(map_ty);

    let accipe = lookup_method_for_type(receiver, "accipe", 1).expect("tabula accipe");
    assert_eq!(mir_collection_op(accipe), Some(MirCollectionOp::Index));

    let pone = lookup_method_for_type(receiver, "pone", 2).expect("tabula pone");
    assert_eq!(mir_collection_op(pone), Some(MirCollectionOp::Put));
}

#[test]
fn regex_consentit_resolves_on_regex_receiver() {
    let types = TypeTable::new();
    let regex = types.primitive(Primitive::Regex);
    let receiver = types.get(regex);

    let consentit = lookup_method_for_type(receiver, "consentit", 1).expect("regex consentit");
    assert_eq!(consentit.receiver, IntrinsicReceiver::Regex);
    assert_eq!(consentit.return_kind, super::IntrinsicReturnKind::Bool);
    assert_eq!(mir_collection_op(consentit), Some(MirCollectionOp::RegexMatches));

    assert_eq!(receivers_for_type(receiver), &[IntrinsicReceiver::Regex]);
}

#[test]
fn regex_find_and_match_accessors_resolve() {
    let types = TypeTable::new();
    let regex = types.get(types.primitive(Primitive::Regex));
    let found = types.get(types.primitive(Primitive::Match));

    let find = lookup_method_for_type(regex, "inventa", 1).expect("regex inventa");
    assert_eq!(find.return_kind, super::IntrinsicReturnKind::OptionMatch);
    assert_eq!(mir_collection_op(find), Some(MirCollectionOp::RegexFind));

    let find_all = lookup_method_for_type(regex, "collecta", 1).expect("regex collecta");
    assert_eq!(find_all.return_kind, super::IntrinsicReturnKind::MatchArray);
    assert_eq!(mir_collection_op(find_all), Some(MirCollectionOp::RegexFindAll));

    let split = lookup_method_for_type(regex, "divide", 1).expect("regex divide");
    assert_eq!(split.receiver, IntrinsicReceiver::Regex);
    assert_eq!(split.return_kind, super::IntrinsicReturnKind::TextusArray);
    assert_eq!(mir_collection_op(split), Some(MirCollectionOp::RegexSplit));

    assert_eq!(receivers_for_type(found), &[IntrinsicReceiver::Match]);
    let accessors = [
        ("inventum", 0, super::IntrinsicReturnKind::Textus, MirCollectionOp::MatchText),
        (
            "principium",
            0,
            super::IntrinsicReturnKind::Numerus,
            MirCollectionOp::MatchStart,
        ),
        ("terminus", 0, super::IntrinsicReturnKind::Numerus, MirCollectionOp::MatchEnd),
        (
            "coetus",
            1,
            super::IntrinsicReturnKind::OptionTextus,
            MirCollectionOp::MatchGroup,
        ),
        (
            "nominatus",
            1,
            super::IntrinsicReturnKind::OptionTextus,
            MirCollectionOp::MatchNamed,
        ),
    ];
    for (name, arity, return_kind, op) in accessors {
        let row = lookup_method_for_type(found, name, arity).expect("match accessor");
        assert_eq!(row.receiver, IntrinsicReceiver::Match);
        assert_eq!(row.return_kind, return_kind);
        assert_eq!(mir_collection_op(row), Some(op));
    }
}

#[test]
fn tabula_claves_is_registered_as_keys() {
    let mut types = TypeTable::new();
    let map = types.map(types.primitive(Primitive::Textus), types.primitive(Primitive::Numerus));
    let tabula = types.get(map);

    let claves = lookup_method_for_type(tabula, "claves", 0).expect("tabula claves");
    assert_eq!(claves.receiver, IntrinsicReceiver::Tabula);
    assert_eq!(mir_collection_op(claves), Some(MirCollectionOp::Keys));
}

#[test]
fn tabula_claves_typechecks_as_key_lista() {
    let source = r#"
incipit {
  sit tabula<textus, numerus> scores ← { "aelia": 95 }
  sit lista<textus> keys ← scores.claves()
  nota keys
}
"#;
    let session = Session::new(Config::default().with_latin_locale());
    let result = analyze_source(&session, "tabula-claves.fab", source, AnalyzeRequest::default());
    assert!(result.is_ok(), "tabula claves must typecheck: {:?}", result.err());
}

#[test]
fn tabula_valores_is_registered_as_values() {
    let mut types = TypeTable::new();
    let map = types.map(types.primitive(Primitive::Textus), types.primitive(Primitive::Numerus));
    let tabula = types.get(map);

    let valores = lookup_method_for_type(tabula, "valores", 0).expect("tabula valores");
    assert_eq!(valores.receiver, IntrinsicReceiver::Tabula);
    assert_eq!(mir_collection_op(valores), Some(MirCollectionOp::Values));
}

#[test]
fn tabula_valores_typechecks_as_value_lista() {
    let source = r#"
incipit {
  sit tabula<textus, numerus> scores ← { "aelia": 95 }
  sit lista<numerus> values ← scores.valores()
  nota values
}
"#;
    let session = Session::new(Config::default().with_latin_locale());
    let result = analyze_source(&session, "tabula-valores.fab", source, AnalyzeRequest::default());
    assert!(result.is_ok(), "tabula valores must typecheck: {:?}", result.err());
}

#[test]
fn intervallum_coercere_is_registered_as_interval_clamp() {
    let mut types = TypeTable::new();
    let interval = types.intervallum(types.primitive(Primitive::Numerus));
    let receiver = types.get(interval);

    let coercere = lookup_method_for_type(receiver, "coercere", 1).expect("intervallum coercere");
    assert_eq!(coercere.receiver, IntrinsicReceiver::Intervallum);
    assert_eq!(mir_collection_op(coercere), Some(MirCollectionOp::IntervalClamp));
}

#[test]
fn intervallum_coercere_typechecks_as_intervallum() {
    let source = r"
incipit {
  sit intervallum<numerus> wide ← 0‥10
  sit intervallum<numerus> narrow ← wide.coercere(2…8)
  nota narrow
}
";
    let session = Session::new(Config::default().with_latin_locale());
    let result = analyze_source(&session, "intervallum-coercere.fab", source, AnalyzeRequest::default());
    assert!(result.is_ok(), "intervallum coercere must typecheck: {:?}", result.err());
}

#[test]
fn fractus_receiver_prefers_fractus_only_rounding_rows() {
    let types = TypeTable::new();
    let receiver = types.get(types.primitive(Primitive::Fractus));

    let rotunda = lookup_method_for_type(receiver, "rotunda", 0).expect("fractus rotunda");
    assert_eq!(rotunda.receiver, IntrinsicReceiver::Fractus);
    assert_eq!(mir_collection_op(rotunda), Some(MirCollectionOp::Round));

    let absolutum = lookup_method_for_type(receiver, "absolutum", 0).expect("shared numeric absolutum");
    assert_eq!(absolutum.receiver, IntrinsicReceiver::Numeric);
}

#[test]
fn numerus_receiver_uses_numeric_rows_only() {
    let types = TypeTable::new();
    let receiver = types.get(types.primitive(Primitive::Numerus));

    assert!(lookup_method_for_type(receiver, "rotunda", 0).is_none());
    assert!(lookup_method(IntrinsicReceiver::Numeric, "absolutum", 0).is_some());
    assert_eq!(receivers_for_type(receiver), &[IntrinsicReceiver::Numeric]);
}

#[test]
fn lista_higher_order_methods_are_classified_for_mir() {
    let omnia = lookup_method(IntrinsicReceiver::Lista, "omnia", 1).expect("omnia");
    assert_eq!(omnia.params, &[crate::intrinsics::IntrinsicParamKind::Predicate]);
    assert_eq!(omnia.return_kind, crate::intrinsics::IntrinsicReturnKind::Bool);
    assert!(matches!(omnia.mir_kind, IntrinsicMirKind::HigherOrderAll));
    assert!(crate::intrinsics::mir_is_higher_order_lista(omnia));

    let quilibet = lookup_method(IntrinsicReceiver::Lista, "quilibet", 1).expect("quilibet");
    assert_eq!(quilibet.params, &[crate::intrinsics::IntrinsicParamKind::Predicate]);
    assert_eq!(quilibet.return_kind, crate::intrinsics::IntrinsicReturnKind::Bool);
    assert!(matches!(quilibet.mir_kind, IntrinsicMirKind::HigherOrderAny));
    assert!(crate::intrinsics::mir_is_higher_order_lista(quilibet));

    let filtrata = lookup_method(IntrinsicReceiver::Lista, "filtrata", 1).expect("filtrata");
    assert!(matches!(filtrata.mir_kind, IntrinsicMirKind::HigherOrderFiltrata));

    assert!(lookup_method(IntrinsicReceiver::Lista, "map", 1).is_none());

    let mappata = lookup_method(IntrinsicReceiver::Lista, "mappata", 1).expect("mappata");
    assert!(matches!(mappata.mir_kind, IntrinsicMirKind::HigherOrderMap));

    let reducta = lookup_method(IntrinsicReceiver::Lista, "reducta", 2).expect("reducta");
    assert!(matches!(reducta.mir_kind, IntrinsicMirKind::HigherOrderReduce));
}

#[test]
fn lista_summa_is_promoted_to_mir_collection_sum() {
    let summa = lookup_method(IntrinsicReceiver::Lista, "summa", 0).expect("summa");
    assert_eq!(mir_collection_op(summa), Some(MirCollectionOp::Sum));
}

#[test]
fn sparsa_nonnihil_is_promoted_to_distinct_mir_operation() {
    let nonnihil = lookup_method(IntrinsicReceiver::Sparsa, "nonnihil", 0).expect("sparsa nonnihil");
    assert_eq!(mir_collection_op(nonnihil), Some(MirCollectionOp::SparseNonzeroCount));
}

#[test]
fn tensor_activatio_relu_is_registered_as_mir_tensor_relu() {
    let mut types = TypeTable::new();
    let f32_ty = types.sized_numeric(Primitive::Fractus, NumericWidth::F32);
    let shape = types.unspecified_shape();
    let tensor_id = types.intern(Type::Tensor(f32_ty, shape));
    let tensor_ty = types.get(tensor_id);

    let relu = lookup_method_for_type(tensor_ty, "activatio_relu", 0).expect("tensor activatio_relu");
    assert_eq!(relu.receiver, IntrinsicReceiver::Tensor);
    assert_eq!(relu.arg_count, 0);
    assert_eq!(mir_collection_op(relu), Some(MirCollectionOp::TensorRelu));

    let gelu = lookup_method_for_type(tensor_ty, "gelu", 0).expect("tensor gelu");
    assert_eq!(mir_collection_op(gelu), Some(MirCollectionOp::TensorGelu));

    let silu = lookup_method_for_type(tensor_ty, "silu", 0).expect("tensor silu");
    assert_eq!(silu.receiver, IntrinsicReceiver::Tensor);
    assert_eq!(silu.return_kind, super::IntrinsicReturnKind::SameAsReceiver);
    assert_eq!(silu.mir_kind, IntrinsicMirKind::CodegenOnly);
    assert_eq!(mir_collection_op(silu), None, "silu must stay a composed lowering");
}

#[test]
fn sparse_representation_densata_is_promoted_to_distinct_mir_operation() {
    let densata = lookup_method(IntrinsicReceiver::Sparsa, "densata", 0).expect("sparsa densata");
    assert_eq!(mir_collection_op(densata), Some(MirCollectionOp::SparseDensify));

    let accipe = lookup_method(IntrinsicReceiver::Sparsa, "accipe", 1).expect("sparsa accipe");
    assert_eq!(mir_collection_op(accipe), Some(MirCollectionOp::SparseGet));

    let ponde = lookup_method(IntrinsicReceiver::Sparsa, "ponde", 2).expect("sparsa ponde");
    assert_eq!(mir_collection_op(ponde), Some(MirCollectionOp::SparseSet));
}

#[test]
fn ascii_cursor_is_registered_as_cord_cursor() {
    let types = TypeTable::new();
    let ascii = types.get(types.primitive(Primitive::Ascii));

    assert_eq!(receivers_for_type(ascii), &[IntrinsicReceiver::Ascii]);
    let cursor = lookup_method_for_type(ascii, "cursor", 0).expect("ascii cursor");
    assert_eq!(cursor.receiver, IntrinsicReceiver::Ascii);
    assert_eq!(cursor.return_kind, super::IntrinsicReturnKind::CursorLittera);
    assert_eq!(mir_collection_op(cursor), Some(MirCollectionOp::CordCursor));
}

#[test]
fn textus_cursor_is_registered_as_cord_cursor() {
    let types = TypeTable::new();
    let textus = types.get(types.primitive(Primitive::Textus));

    assert_eq!(receivers_for_type(textus), &[IntrinsicReceiver::Textus]);
    let cursor = lookup_method_for_type(textus, "cursor", 0).expect("textus cursor");
    assert_eq!(cursor.receiver, IntrinsicReceiver::Textus);
    assert_eq!(cursor.return_kind, super::IntrinsicReturnKind::CursorLittera);
    assert_eq!(mir_collection_op(cursor), Some(MirCollectionOp::CordCursor));
}

#[test]
fn textus_vacua_is_registered_as_is_empty() {
    let types = TypeTable::new();
    let textus = types.get(types.primitive(Primitive::Textus));

    let vacua = lookup_method_for_type(textus, "vacua", 0).expect("textus vacua");
    assert_eq!(vacua.receiver, IntrinsicReceiver::Textus);
    assert_eq!(mir_collection_op(vacua), Some(MirCollectionOp::IsEmpty));
}

#[test]
fn textus_vacua_typechecks_as_bivalens() {
    let source = r#"
incipit {
  sit textus s ← ""
  sit bivalens empty_flag ← s.vacua()
  nota empty_flag
}
"#;
    let session = Session::new(Config::default().with_latin_locale());
    let result = analyze_source(&session, "textus-vacua.fab", source, AnalyzeRequest::default());
    assert!(result.is_ok(), "textus vacua must typecheck: {:?}", result.err());
}

#[test]
fn atomic_receiver_methods_are_registered_as_mir_atomic_ops() {
    let mut types = TypeTable::new();
    let u32_ty = types.sized_numeric(Primitive::Numerus, NumericWidth::U32);
    let atomic_ty = types.atomic(u32_ty);
    let receiver = types.get(atomic_ty);

    assert_eq!(receivers_for_type(receiver), &[IntrinsicReceiver::Atomic]);
    assert!(is_atomic_operator_method("load"));
    assert!(is_atomic_operator_method("compare_exchange"));
    assert!(lookup_method_for_type(receiver, "load", 0).is_some());
    assert!(lookup_method_for_type(receiver, "store", 1).is_some());
    assert!(lookup_method_for_type(receiver, "exchange", 1).is_some());
    let compare_exchange = lookup_method_for_type(receiver, "compare_exchange", 2).expect("compare_exchange");
    assert_eq!(compare_exchange.receiver, IntrinsicReceiver::Atomic);
    assert!(matches!(
        compare_exchange.mir_kind,
        IntrinsicMirKind::Atomic(MirAtomicOp::CompareExchange)
    ));
}

// ── CCI-1: the three common-case intrinsic rows ─────────────────────────

/// CCI-1: the three rows resolve on the tensor receiver with the frozen
/// spellings, arity, param/return kinds, and MIR collection ops.
#[test]
fn cci1_common_case_intrinsic_rows_resolve_on_the_tensor_receiver() {
    let mut types = TypeTable::new();
    let f32_ty = types.sized_numeric(Primitive::Fractus, NumericWidth::F32);
    let shape = types.unspecified_shape();
    let tensor_id = types.intern(Type::Tensor(f32_ty, shape));
    let tensor_ty = types.get(tensor_id);

    let rms_norm = lookup_method_for_type(tensor_ty, "rms_norm", 3).expect("tensor rms_norm");
    assert_eq!(rms_norm.receiver, IntrinsicReceiver::Tensor);
    assert_eq!(rms_norm.arg_count, 3);
    assert_eq!(
        rms_norm.params,
        &[
            crate::intrinsics::IntrinsicParamKind::NumerusIndex,
            crate::intrinsics::IntrinsicParamKind::Inner,
            crate::intrinsics::IntrinsicParamKind::TensorPeer,
        ]
    );
    assert_eq!(rms_norm.return_kind, crate::intrinsics::IntrinsicReturnKind::SameAsReceiver);
    assert_eq!(mir_collection_op(rms_norm), Some(MirCollectionOp::TensorRmsNorm));

    let rope_norm = lookup_method_for_type(tensor_ty, "rope_norm", 2).expect("tensor rope_norm");
    assert_eq!(rope_norm.receiver, IntrinsicReceiver::Tensor);
    assert_eq!(rope_norm.arg_count, 2);
    assert_eq!(
        rope_norm.params,
        &[
            crate::intrinsics::IntrinsicParamKind::NumerusIndex,
            crate::intrinsics::IntrinsicParamKind::TensorPeer,
        ]
    );
    assert_eq!(rope_norm.generic.len(), 1);
    assert_eq!(rope_norm.generic[0].sort, radix_types::GenericParamSort::Magnitudo);
    assert_eq!(rope_norm.return_kind, crate::intrinsics::IntrinsicReturnKind::SameAsReceiver);
    assert_eq!(mir_collection_op(rope_norm), Some(MirCollectionOp::TensorRope));

    let gather = lookup_method_for_type(tensor_ty, "gather", 1).expect("tensor gather");
    assert_eq!(gather.receiver, IntrinsicReceiver::Tensor);
    assert_eq!(gather.arg_count, 1);
    assert_eq!(gather.params, &[crate::intrinsics::IntrinsicParamKind::TensorIndexIds]);
    assert_eq!(gather.return_kind, crate::intrinsics::IntrinsicReturnKind::GatherShape);
    assert_eq!(mir_collection_op(gather), Some(MirCollectionOp::TensorGather));

    // No other arity exists for these spellings (bounded rows).
    assert!(lookup_method_for_type(tensor_ty, "rms_norm", 2).is_none());
    assert!(lookup_method_for_type(tensor_ty, "rope_norm", 1).is_none());
    assert!(lookup_method_for_type(tensor_ty, "gather", 2).is_none());
}

/// CCI-1: the existing tensor rows are untouched (additive proof — the new
/// rows must not disturb the `addita_bias`/`laminatio`/`matmul` catalog).
#[test]
fn cci1_additive_rows_preserve_existing_tensor_rows() {
    let mut types = TypeTable::new();
    let f32_ty = types.sized_numeric(Primitive::Fractus, NumericWidth::F32);
    let shape = types.unspecified_shape();
    let tensor_id = types.intern(Type::Tensor(f32_ty, shape));
    let tensor_ty = types.get(tensor_id);

    let addita_bias = lookup_method_for_type(tensor_ty, "addita_bias", 1).expect("tensor addita_bias");
    assert_eq!(mir_collection_op(addita_bias), Some(MirCollectionOp::TensorAdd));

    let laminatio = lookup_method_for_type(tensor_ty, "laminatio", 4).expect("tensor laminatio 4-arg");
    assert_eq!(mir_collection_op(laminatio), Some(MirCollectionOp::TensorLayerNorm));

    let matmul = lookup_method_for_type(tensor_ty, "matmul", 1).expect("tensor matmul");
    assert_eq!(mir_collection_op(matmul), Some(MirCollectionOp::TensorMatMul));

    let activatio_softmax = lookup_method_for_type(tensor_ty, "activatio_softmax", 0).expect("tensor softmax");
    assert_eq!(mir_collection_op(activatio_softmax), Some(MirCollectionOp::TensorSoftmax));
}

/// e2133f3f: `softmax_causal` is the source spelling for the causal-masked
/// softmax collection op — a method row, not a compiler-recognized
/// `softmax(x + mask)` composition.
#[test]
fn softmax_causal_row_maps_to_tensor_causal_masked_softmax() {
    let mut types = TypeTable::new();
    let f32_ty = types.sized_numeric(Primitive::Fractus, NumericWidth::F32);
    let shape = types.unspecified_shape();
    let tensor_id = types.intern(Type::Tensor(f32_ty, shape));
    let tensor_ty = types.get(tensor_id);

    let row = lookup_method_for_type(tensor_ty, "softmax_causal", 0).expect("tensor softmax_causal");
    assert_eq!(mir_collection_op(row), Some(MirCollectionOp::TensorCausalMaskedSoftmax));
}

#[test]
fn regex_replace_and_text_escape_resolve_by_receiver() {
    let types = TypeTable::new();
    let regex = types.get(types.primitive(Primitive::Regex));
    let text = types.get(types.primitive(Primitive::Textus));

    // `muta` is one Latin name on two receivers: the regex row replaces
    // matches, the textus row replaces a literal substring. Receiver type picks
    // the operation; the two never share a row.
    let regex_replace = lookup_method_for_type(regex, "muta", 2).expect("regex muta");
    assert_eq!(regex_replace.receiver, IntrinsicReceiver::Regex);
    assert_eq!(regex_replace.return_kind, super::IntrinsicReturnKind::Textus);
    assert_eq!(mir_collection_op(regex_replace), Some(MirCollectionOp::RegexReplace));
    let text_replace = lookup_method_for_type(text, "muta", 2).expect("textus muta");
    assert_eq!(text_replace.receiver, IntrinsicReceiver::Textus);
    assert_eq!(mir_collection_op(text_replace), Some(MirCollectionOp::Replace));

    // RX-5 (RD-5): the closure form is the same row, not a second one: the
    // replacement slot accepts a `textus` or a `(match) → textus` closure, so
    // the registry holds exactly one regex `muta` of arity two.
    assert_eq!(
        regex_replace.params,
        &[
            super::IntrinsicParamKind::Textus,
            super::IntrinsicParamKind::TextusOrReplacer
        ]
    );
    let regex_muta_rows = INTRINSICS
        .iter()
        .filter(|row| row.receiver == IntrinsicReceiver::Regex && row.faber_name == "muta" && row.arg_count == 2)
        .count();
    assert_eq!(regex_muta_rows, 1);

    let escape = lookup_method_for_type(text, "munita", 0).expect("textus munita");
    assert_eq!(escape.receiver, IntrinsicReceiver::Textus);
    assert_eq!(escape.return_kind, super::IntrinsicReturnKind::Textus);
    assert_eq!(mir_collection_op(escape), Some(MirCollectionOp::TextEscape));
    assert!(lookup_method_for_type(regex, "munita", 0).is_none());
}

fn analyze_regex_replace(source: &str) -> Result<crate::driver::AnalyzedModule, Vec<radix_diagnostics::Diagnostic>> {
    let session = Session::new(Config::default().with_latin_locale());
    analyze_source(&session, "regex-muta.fab", source, AnalyzeRequest::default())
}

#[test]
fn regex_muta_closure_typechecks_from_the_match_signature() {
    // The closure parameter is a hole: the match type has no source name (RD-3),
    // so the intrinsic's signature types it. The literal form is unchanged.
    let closure = r#"
incipit {
  sit regex r ← "a+" ↦ regex
  sit textus out ← r.muta("caab", _ m ∴ m.inventum())
  nota out
}
"#;
    let result = analyze_regex_replace(closure);
    assert!(result.is_ok(), "closure muta must typecheck: {:?}", result.err());
    let literal = r#"
incipit {
  sit regex r ← "a+" ↦ regex
  sit textus out ← r.muta("caab", "x")
  nota out
}
"#;
    let result = analyze_regex_replace(literal);
    assert!(result.is_ok(), "literal muta must typecheck: {:?}", result.err());
}

#[test]
fn regex_muta_rejects_a_closure_that_does_not_return_textus() {
    let source = r#"
incipit {
  sit regex r ← "a+" ↦ regex
  nota r.muta("caab", _ m ∴ m.principium())
}
"#;
    let Err(diagnostics) = analyze_regex_replace(source) else {
        panic!("a numerus result is not a replacement");
    };
    crate::test_support::assert_has_error_issue(&diagnostics, "SEM010", "expression_type_mismatch");
}

#[test]
fn regex_muta_rejects_a_closure_parameter_that_is_not_the_match() {
    let source = r#"
incipit {
  sit regex r ← "a+" ↦ regex
  nota r.muta("caab", numerus m ∴ "x")
}
"#;
    let Err(diagnostics) = analyze_regex_replace(source) else {
        panic!("the parameter is the match");
    };
    crate::test_support::assert_has_error_issue(&diagnostics, "SEM010", "closure_parameter_type_mismatch");
}
