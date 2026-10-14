//! SCRFRAME (F-70): a received `scrinium` carries every declared field, on
//! both the `accipe` and the `cursor` path.

use super::*;
use crate::cursor::GeneratorCursor;
use crate::host::BufferHost;
use crate::sermo::SermoValue;
use radix_hir::DefId;
use radix_lexer::{Interner, Span, Symbol};
use radix_mir::{MirProgram, MirType, MirValidationContext};
use radix_types::{Type, TypeTable};
use rustc_hash::FxHashMap;

const FRAME: DefId = DefId(1);
const STATUS: DefId = DefId(2);
const FIELDS: [&str; 8] = [
    "id",
    "parent_id",
    "call",
    "status",
    "data",
    "created_ms",
    "from",
    "trace",
];

struct Fixture {
    types: TypeTable,
    interner: Interner,
    program: MirProgram,
    context_fields: FxHashMap<Symbol, MirType>,
    variants: Vec<DefId>,
}

fn fixture() -> Fixture {
    let mut types = TypeTable::default();
    let mut interner = Interner::new();
    let status_ty = types.intern(Type::Enum(STATUS));
    let mut context_fields = FxHashMap::default();
    for name in FIELDS {
        context_fields.insert(interner.intern(name), MirType::semantic(status_ty));
    }
    Fixture {
        types,
        interner,
        program: MirProgram::new(),
        context_fields,
        variants: (10..17).map(DefId).collect(),
    }
}

fn field(frame: &Value, interner: &Interner, name: &str) -> Value {
    let Value::Struct(def, fields) = frame else {
        panic!("received frame is not a struct: {frame:?}");
    };
    assert_eq!(*def, FRAME);
    let fields = fields.borrow();
    assert_eq!(fields.len(), FIELDS.len(), "frame is missing fields");
    let symbol = fields
        .keys()
        .find(|symbol| interner.resolve(**symbol) == name)
        .unwrap_or_else(|| panic!("frame has no `{name}` field"));
    fields[symbol].clone()
}

fn assert_full_echo_frame(frame: &Value, fixture: &Fixture) {
    let get = |name| field(frame, &fixture.interner, name);
    assert_eq!(get("data"), Value::Str("salve".to_owned()));
    assert_eq!(get("call"), Value::Str("runtime:echo".to_owned()));
    assert_eq!(get("parent_id"), Value::Str("1".to_owned()));
    let Value::EnumVariant(item, _) = get("status") else {
        panic!("status is not an enum variant");
    };
    assert_eq!(item, fixture.variants[1]);
    for absent in ["id", "created_ms", "from", "trace"] {
        assert_eq!(get(absent), Value::Nil, "{absent}");
    }
}

fn open_echo<H: Host + ?Sized>(runner: &mut Runner<'_, H>) -> Rc<RefCell<SermoValue>> {
    let Value::Sermo(sermo) = runner
        .sermo_open("runtime:echo".to_owned(), Span::default())
        .expect("open echo")
    else {
        panic!("sermo_open did not return a sermo");
    };
    runner
        .sermo_set_opener(&sermo, Value::Str("salve".to_owned()), Span::default())
        .expect("set opener");
    sermo
}

macro_rules! runner_over {
    ($fixture:ident, $runner:ident) => {
        let mut context = MirValidationContext::new(&$fixture.types);
        context
            .struct_fields
            .insert(FRAME, $fixture.context_fields.clone());
        context
            .enum_variants
            .insert(STATUS, $fixture.variants.clone());
        let mut host = BufferHost::default();
        let mut $runner = Runner::new(
            &$fixture.program,
            &context,
            &$fixture.types,
            &$fixture.interner,
            &mut host,
        );
    };
}

#[test]
fn accipe_frame_carries_every_scrinium_field() {
    let fixture = fixture();
    runner_over!(fixture, runner);
    let sermo = open_echo(&mut runner);
    let frame = runner
        .sermo_receive(FRAME, &sermo, Span::default())
        .expect("accipe");
    assert_full_echo_frame(&frame, &fixture);
}

#[test]
fn cursor_frames_carry_every_scrinium_field() {
    let fixture = fixture();
    runner_over!(fixture, runner);
    let sermo = open_echo(&mut runner);
    let cursor = GeneratorCursor::sermo(sermo, FRAME);
    let frames = runner
        .cursor_collect(&cursor, Span::default())
        .expect("cursor");
    assert_eq!(frames.len(), 1);
    assert_full_echo_frame(&frames[0], &fixture);
}

#[test]
fn sermo_materialization_failure_carries_fault_args() {
    const ROUTE: &str = "solum:lege";
    const PATH: &str = "/nonexistent/radix-error-channel/x";

    let fixture = fixture();
    runner_over!(fixture, runner);
    let Value::Sermo(sermo) = runner
        .sermo_open(ROUTE.to_owned(), Span::default())
        .expect("open leaf route")
    else {
        panic!("sermo_open did not return a sermo");
    };
    runner
        .sermo_set_opener(&sermo, Value::Str(PATH.to_owned()), Span::default())
        .expect("set opener");
    let error = runner
        .sermo_first_item(&sermo, Span::default())
        .expect_err("a failed leaf reply must raise");

    // WHY: the leaf failure crosses as a kind/path record, so the conversion
    // diagnostic joins those facts as its message and carries them as
    // structured args. No platform text is asserted because none crosses;
    // comparing the whole error against the same constructor pins code,
    // issue, args, message and span together.
    let want = RunnerError {
        message: format!("NotFound {PATH}"),
        args: vec![
            (
                "detail".to_owned(),
                "sermo reply conversion failed".to_owned(),
            ),
            ("fault_kind".to_owned(), "NotFound".to_owned()),
            ("fault_path".to_owned(), PATH.to_owned()),
        ],
        ..RunnerError::conversion(Span::default(), "sermo reply conversion failed")
    };
    assert_eq!(error, want);
    assert_eq!(error.issue, "runner_sermo_reply_conversion_failed");
    assert!(
        error
            .args
            .contains(&("fault_kind".to_owned(), "NotFound".to_owned()))
    );
    assert!(
        error
            .args
            .contains(&("fault_path".to_owned(), PATH.to_owned()))
    );
}

#[test]
fn failed_leaf_reply_carries_kind_and_path_facts() {
    const ROUTE: &str = "solum:lege";
    const PATH: &str = "/nonexistent/radix-error-channel/x";

    let fixture = fixture();
    runner_over!(fixture, runner);
    let Value::Sermo(sermo) = runner
        .sermo_open(ROUTE.to_owned(), Span::default())
        .expect("open leaf route")
    else {
        panic!("sermo_open did not return a sermo");
    };
    runner
        .sermo_set_opener(&sermo, Value::Str(PATH.to_owned()), Span::default())
        .expect("set opener");
    let error = runner
        .sermo_first_item(&sermo, Span::default())
        .expect_err("a failed leaf reply must raise");

    // The crossing message is the record's kind and path, never platform text.
    assert_eq!(error.message, format!("NotFound {PATH}"));
    // Identity facts survive as structured args; prose is never asserted.
    assert!(
        error
            .args
            .contains(&("fault_kind".to_owned(), "NotFound".to_owned()))
    );
    assert!(
        error
            .args
            .contains(&("fault_path".to_owned(), PATH.to_owned()))
    );
}
