//! A method call on a value of an instantiated generic class validates and
//! runs: the receiver is `Applied(class, args)` while the method's `self`
//! parameter is the bare class.

use crate::codegen::Target;
use crate::driver::{Config, Session};
use crate::mir::{BufferHost, RunSourceError, run_source};

fn run(name: &str, source: &str) -> Vec<String> {
    let session = Session::new(Config::default().with_target(Target::HirTypeScript));
    let mut host = BufferHost::default();
    match run_source(&session, name, source, &mut host) {
        Ok(()) => host.stdout_lines,
        Err(RunSourceError::Frontend(errors)) => panic!("fixture frontend errors: {errors:?}"),
        Err(RunSourceError::Mir(errors)) => panic!("fixture MIR errors: {errors:?}"),
        Err(RunSourceError::Stepper(errors)) => panic!("fixture runner errors: {errors:?}"),
    }
}

const SIZE_PARAMETER: &str = r#"+++
locale = "en"
+++
class Box<size N> {
    mut vector<f32, size N> minima

    fn scalar() → f32 {
        return 1.5
    }
}

main {
    let vector<f32, size 3> v ← [1.0, 2.0, 3.0] ↦ vector<f32, size 3>
    let Box<3> b ← Box { minima = v }
    print b.scalar()
}
"#;

#[test]
fn method_call_on_size_parameter_class_runs() {
    assert_eq!(run("size-box.fab", SIZE_PARAMETER), vec!["1.5"]);
}

const TYPE_PARAMETER: &str = r#"+++
locale = "en"
+++
class Cista<T> {
    mut T item

    fn put(T value) {
        self.item ← value
    }

    fn get() → T {
        return self.item
    }
}

main {
    mut Cista<int> b ← Cista { item = 7 }
    b.put(41)
    let int x ← b.get()
    print x
}
"#;

#[test]
fn method_calls_on_type_parameter_class_run_with_result_substitution() {
    assert_eq!(run("cista-put-get.fab", TYPE_PARAMETER), vec!["41"]);
}
