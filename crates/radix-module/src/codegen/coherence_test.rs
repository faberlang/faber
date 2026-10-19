//! Host method capability coherence: every ABI symbol in HOST_METHOD_CAPABILITIES
//! maps to a canonical intrinsic owner, and Go/TypeScript frame protocol emission
//! matches runtime FrameStatus semantics (terminal vs non-terminal status values).

use crate::abi::host_abi;
#[cfg(any(feature = "hir-go", feature = "hir-ts"))]
use crate::codegen::Target;
#[cfg(any(feature = "hir-go", feature = "hir-ts"))]
use crate::driver::Config;
use crate::intrinsics::{INTRINSICS, IntrinsicReceiver, mir_collection_op};
#[cfg(any(feature = "hir-go", feature = "hir-ts"))]
use crate::runtime_contract::frame::FrameStatus;
#[cfg(any(feature = "hir-go", feature = "hir-ts"))]
use crate::{Compiler, Output};
use radix_types::MirCollectionOp;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
struct HostMethodCapability {
    receiver: IntrinsicReceiver,
    op: MirCollectionOp,
    symbol: &'static str,
}

// This is the explicit crossing boundary, rather than a second lowering table:
// receiver + MIR operation identify the compiler capability; the ABI constant is
// the single runtime spelling. Everything else is either target-local or belongs
// to a non-method runtime surface.
const HOST_METHOD_CAPABILITIES: &[HostMethodCapability] = &[
    HostMethodCapability {
        receiver: IntrinsicReceiver::Textus,
        op: MirCollectionOp::IsEmpty,
        symbol: host_abi::SYMBOL_TEXT_IS_EMPTY,
    },
    HostMethodCapability {
        receiver: IntrinsicReceiver::Textus,
        op: MirCollectionOp::Contains,
        symbol: host_abi::SYMBOL_TEXT_CONTAINS,
    },
    HostMethodCapability {
        receiver: IntrinsicReceiver::Textus,
        op: MirCollectionOp::StartsWith,
        symbol: host_abi::SYMBOL_TEXT_STARTS_WITH,
    },
    HostMethodCapability {
        receiver: IntrinsicReceiver::Textus,
        op: MirCollectionOp::EndsWith,
        symbol: host_abi::SYMBOL_TEXT_ENDS_WITH,
    },
    HostMethodCapability {
        receiver: IntrinsicReceiver::Textus,
        op: MirCollectionOp::Uppercase,
        symbol: host_abi::SYMBOL_TEXT_UPPERCASE,
    },
    HostMethodCapability {
        receiver: IntrinsicReceiver::Textus,
        op: MirCollectionOp::Lowercase,
        symbol: host_abi::SYMBOL_TEXT_LOWERCASE,
    },
    HostMethodCapability {
        receiver: IntrinsicReceiver::Textus,
        op: MirCollectionOp::Trim,
        symbol: host_abi::SYMBOL_TEXT_TRIM,
    },
    HostMethodCapability {
        receiver: IntrinsicReceiver::Textus,
        op: MirCollectionOp::Split,
        symbol: host_abi::SYMBOL_TEXT_SPLIT,
    },
    HostMethodCapability {
        receiver: IntrinsicReceiver::Textus,
        op: MirCollectionOp::Replace,
        symbol: host_abi::SYMBOL_TEXT_REPLACE,
    },
    HostMethodCapability {
        receiver: IntrinsicReceiver::Lista,
        op: MirCollectionOp::Append,
        symbol: host_abi::SYMBOL_ARRAY_PUSH,
    },
    HostMethodCapability {
        receiver: IntrinsicReceiver::Lista,
        op: MirCollectionOp::Contains,
        symbol: host_abi::SYMBOL_ARRAY_CONTAINS,
    },
    HostMethodCapability {
        receiver: IntrinsicReceiver::Lista,
        op: MirCollectionOp::Length,
        symbol: host_abi::SYMBOL_ARRAY_LENGTH,
    },
    HostMethodCapability {
        receiver: IntrinsicReceiver::Lista,
        op: MirCollectionOp::Reverse,
        symbol: host_abi::SYMBOL_ARRAY_REVERSE,
    },
    HostMethodCapability {
        receiver: IntrinsicReceiver::Lista,
        op: MirCollectionOp::Sort,
        symbol: host_abi::SYMBOL_ARRAY_SORT,
    },
    HostMethodCapability {
        receiver: IntrinsicReceiver::Lista,
        op: MirCollectionOp::Sum,
        symbol: host_abi::SYMBOL_ARRAY_SUM,
    },
    HostMethodCapability {
        receiver: IntrinsicReceiver::Tabula,
        op: MirCollectionOp::Put,
        symbol: host_abi::SYMBOL_MAP_PUT,
    },
    HostMethodCapability {
        receiver: IntrinsicReceiver::Tabula,
        op: MirCollectionOp::Contains,
        symbol: host_abi::SYMBOL_MAP_CONTAINS,
    },
    HostMethodCapability {
        receiver: IntrinsicReceiver::Tabula,
        op: MirCollectionOp::Delete,
        symbol: host_abi::SYMBOL_MAP_DELETE,
    },
    HostMethodCapability {
        receiver: IntrinsicReceiver::Tabula,
        op: MirCollectionOp::Length,
        symbol: host_abi::SYMBOL_MAP_LENGTH,
    },
    HostMethodCapability {
        receiver: IntrinsicReceiver::Tabula,
        op: MirCollectionOp::IsEmpty,
        symbol: host_abi::SYMBOL_MAP_IS_EMPTY,
    },
    HostMethodCapability {
        receiver: IntrinsicReceiver::Tabula,
        op: MirCollectionOp::Keys,
        symbol: host_abi::SYMBOL_MAP_KEYS,
    },
    HostMethodCapability {
        receiver: IntrinsicReceiver::Tabula,
        op: MirCollectionOp::Values,
        symbol: host_abi::SYMBOL_MAP_VALUES,
    },
    HostMethodCapability {
        receiver: IntrinsicReceiver::Copia,
        op: MirCollectionOp::Append,
        symbol: host_abi::SYMBOL_SET_ADD,
    },
    HostMethodCapability {
        receiver: IntrinsicReceiver::Copia,
        op: MirCollectionOp::Contains,
        symbol: host_abi::SYMBOL_SET_CONTAINS,
    },
    HostMethodCapability {
        receiver: IntrinsicReceiver::Copia,
        op: MirCollectionOp::Delete,
        symbol: host_abi::SYMBOL_SET_DELETE,
    },
    HostMethodCapability {
        receiver: IntrinsicReceiver::Copia,
        op: MirCollectionOp::Length,
        symbol: host_abi::SYMBOL_SET_LENGTH,
    },
    HostMethodCapability {
        receiver: IntrinsicReceiver::Copia,
        op: MirCollectionOp::IsEmpty,
        symbol: host_abi::SYMBOL_SET_IS_EMPTY,
    },
    HostMethodCapability {
        receiver: IntrinsicReceiver::Copia,
        op: MirCollectionOp::Union,
        symbol: host_abi::SYMBOL_SET_UNION,
    },
    HostMethodCapability {
        receiver: IntrinsicReceiver::Copia,
        op: MirCollectionOp::Intersection,
        symbol: host_abi::SYMBOL_SET_INTERSECTION,
    },
    HostMethodCapability {
        receiver: IntrinsicReceiver::Copia,
        op: MirCollectionOp::Difference,
        symbol: host_abi::SYMBOL_SET_DIFFERENCE,
    },
    HostMethodCapability {
        receiver: IntrinsicReceiver::Intervallum,
        op: MirCollectionOp::IntervalIntersect,
        symbol: host_abi::SYMBOL_INTERVAL_INTERSECT,
    },
    HostMethodCapability {
        receiver: IntrinsicReceiver::Intervallum,
        op: MirCollectionOp::IntervalUnion,
        symbol: host_abi::SYMBOL_INTERVAL_UNION,
    },
    HostMethodCapability {
        receiver: IntrinsicReceiver::Intervallum,
        op: MirCollectionOp::IntervalLength,
        symbol: host_abi::SYMBOL_INTERVAL_LENGTH,
    },
    HostMethodCapability {
        receiver: IntrinsicReceiver::Intervallum,
        op: MirCollectionOp::IntervalContains,
        symbol: host_abi::SYMBOL_INTERVAL_CONTAINS,
    },
    HostMethodCapability {
        receiver: IntrinsicReceiver::Intervallum,
        op: MirCollectionOp::IntervalClamp,
        symbol: host_abi::SYMBOL_INTERVAL_CLAMP,
    },
];

const HOST_ONLY_SYMBOLS: &[&str] = &[
    host_abi::SYMBOL_INIT,
    host_abi::SYMBOL_SHUTDOWN,
    host_abi::SYMBOL_WRITE_NOTA_TEXT,
    host_abi::SYMBOL_ASSERT,
    host_abi::SYMBOL_ASSERT_MESSAGE,
    host_abi::SYMBOL_FATAL,
    host_abi::SYMBOL_FATAL_OPAQUE,
    host_abi::SYMBOL_FORMAT_I64,
    host_abi::SYMBOL_FORMAT_I1,
    host_abi::SYMBOL_FORMAT_I64_I64,
    host_abi::SYMBOL_FORMAT_I64_I64_I64,
    host_abi::SYMBOL_FORMAT_F64,
    host_abi::SYMBOL_FORMAT_TEXT,
    host_abi::SYMBOL_FORMAT_TEXT_TEXT,
    host_abi::SYMBOL_FORMAT_TEXT_I64,
    host_abi::SYMBOL_FORMAT_I64_TEXT,
    host_abi::SYMBOL_FORMAT_TEXT_TEXT_TEXT,
    host_abi::SYMBOL_FORMAT_TEXT_I64_I1,
];

#[cfg(any(feature = "hir-go", feature = "hir-ts"))]
fn compile(target: Target) -> String {
    let compiler = Compiler::new(Config::default().with_target(target).with_latin_locale());
    let result = compiler.compile_str(
        "frame-coherence.fab",
        r#"incipit {
    sit sermo s ← ad 'runtime:echo' ("salve")
    sit meus<textus> m ← s.meus<textus>()
    m.sink("ping")
    m.fini()
    sit tuus<textus> t ← s.tuus<textus>()
    t.fini()
}"#,
    );
    assert!(
        result
            .diagnostics
            .iter()
            .all(|diagnostic| !diagnostic.is_error()),
        "{:#?}",
        result.diagnostics
    );
    match result.output.expect("target output") {
        Output::Go(output) => output.code,
        Output::TypeScript(output) => output.code,
        _ => panic!("unexpected output variant"),
    }
}

#[cfg(any(feature = "hir-go", feature = "hir-ts"))]
fn frame_statuses() -> [(&'static str, FrameStatus); 7] {
    [
        ("request", FrameStatus::Request),
        ("item", FrameStatus::Item),
        ("byte", FrameStatus::Byte),
        ("bulk", FrameStatus::Bulk),
        ("done", FrameStatus::Done),
        ("error", FrameStatus::Error),
        ("cancel", FrameStatus::Cancel),
    ]
}

/// A file of the in-tree Go runtime package (`runtime/go`).
#[cfg(all(feature = "hir-go", feature = "hir-ts"))]
fn go_runtime_source(file: &str) -> String {
    let path = crate::test_support::runtime_dir("go").join(file);
    std::fs::read_to_string(&path).unwrap_or_else(|error| panic!("{}: {error}", path.display()))
}

#[cfg(any(feature = "hir-go", feature = "hir-ts"))]
fn pascal(tag: &str) -> String {
    let mut value = tag.to_owned();
    value[..1].make_ascii_uppercase();
    value
}

#[test]
fn intrinsic_host_abi_capabilities_have_registered_method_owners() {
    let mut symbols = std::collections::HashSet::new();
    let mut capabilities = std::collections::HashSet::new();
    for capability in HOST_METHOD_CAPABILITIES {
        assert!(
            capability.symbol.starts_with("__faber_rt_v1_"),
            "unversioned symbol: {}",
            capability.symbol
        );
        assert!(
            symbols.insert(capability.symbol),
            "duplicate host method symbol: {}",
            capability.symbol
        );
        assert!(
            capabilities.insert((capability.receiver, capability.op)),
            "duplicate host capability: {capability:?}"
        );
    }

    let missing_owners = HOST_METHOD_CAPABILITIES
        .iter()
        .filter(|capability| {
            !INTRINSICS
                .iter()
                .any(|entry| entry.receiver == capability.receiver && mir_collection_op(entry) == Some(capability.op))
        })
        .map(|capability| format!("{:?}/{:?}: {}", capability.receiver, capability.op, capability.symbol))
        .collect::<Vec<_>>();
    eprintln!(
        "host method capability residuals:\n{}",
        if missing_owners.is_empty() {
            "<none>".to_owned()
        } else {
            missing_owners.join("\n")
        }
    );
    assert!(
        missing_owners.is_empty(),
        "host method capabilities lack canonical crate::intrinsics owners:\n{}",
        missing_owners.join("\n")
    );

    let host_only = HOST_ONLY_SYMBOLS
        .iter()
        .copied()
        .collect::<std::collections::HashSet<_>>();
    assert_eq!(
        host_only.len(),
        HOST_ONLY_SYMBOLS.len(),
        "host-only carve-out contains duplicates"
    );
    assert!(
        host_only
            .iter()
            .all(|symbol| symbol.starts_with("__faber_rt_v1_"))
    );
    assert!(
        host_only.is_disjoint(&symbols),
        "host-only symbols must not be method capabilities"
    );
}

#[cfg(all(feature = "hir-go", feature = "hir-ts"))]
#[test]
fn go_and_typescript_frame_protocols_follow_runtime_status_semantics() {
    // The Go frame protocol lives in the Go runtime package (T1-G9); generated
    // Go only names its types.
    let go = compile(Target::HirGo);
    let go_frame = go_runtime_source("frame.go");
    let go_sermo = go_runtime_source("sermo.go");
    let typescript = compile(Target::HirTypeScript);
    let statuses = frame_statuses();
    let terminal = statuses
        .iter()
        .filter_map(|(tag, status)| status.is_terminal().then_some(*tag))
        .collect::<Vec<_>>();
    let content = statuses
        .iter()
        .filter_map(|(tag, status)| status.is_content().then_some(*tag))
        .collect::<Vec<_>>();

    assert_eq!(terminal, vec!["done", "error", "cancel"]);
    assert_eq!(content, vec!["item", "byte", "bulk"]);
    // TypeScript frame transport now lives behind the runtime import; this
    // compiler-side proof checks that generated modules use that runtime seam.
    assert!(
        typescript.contains("from \"@faber/runtime\"") && typescript.contains("sermo.Sermo.open(\"runtime:echo\""),
        "TypeScript delegates frame handling through the runtime sermo API"
    );

    for (tag, _) in statuses {
        assert!(
            go_frame.contains(&format!("code{}", pascal(tag)))
                && go_frame.contains(&format!("Sermo{} struct{{}}", pascal(tag))),
            "Go omits {tag} frame status"
        );
    }

    assert!(
        !go.contains("radixFrame") && go.contains("rt.SermoOpen"),
        "generated Go carries a frame protocol of its own"
    );

    let go_cursor = go_sermo
        .split("func sermoCursor")
        .nth(1)
        .and_then(|body| body.split("func sermoAccipe").next())
        .expect("Go cursor helper");
    // Terminal statuses finish Go's inbound cursor; non-terminal statuses
    // must not trigger its drained path.
    for tag in &terminal {
        assert!(
            go_cursor.contains(&format!("code{}", pascal(tag))),
            "Go cursor omits terminal {tag}"
        );
    }
    for (tag, status) in frame_statuses() {
        if !status.is_terminal() {
            assert!(
                !go_cursor.contains(&format!("code{}:\n\t\t\t\tsermo.incomingDrained", pascal(tag))),
                "Go marks non-terminal {tag} terminal"
            );
        }
    }
}
