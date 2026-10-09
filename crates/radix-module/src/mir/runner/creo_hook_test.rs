//! The genus `creo` hook runs once after field initialization on every typed
//! construction (`Genus { field = value }`), before the value is bound.

use super::{BufferHost, run_source};
use crate::codegen::Target;
use crate::driver::{Config, Session};

fn run_lines(name: &str, source: &str) -> Vec<String> {
    let session = Session::new(Config::default().with_target(Target::HirRust));
    let mut host = BufferHost::default();
    run_source(&session, name, source, &mut host)
        .unwrap_or_else(|error| panic!("{name} should run on the MIR runner: {error:?}"));
    host.stdout_lines
}

#[test]
fn typed_construction_runs_the_creo_hook_after_field_initialization() {
    let lines = run_lines(
        "creo-hook-runs.fab",
        r#"+++
locale = "en"
+++
class Terminus {
    var int valor = 0

    fn creo() → void {
        if self.valor ≻ 100 {
            self.valor ← 100
        }
    }
}

class Circulus {
    var int radius = 1
    var int diameter = 0

    fn creo() → void {
        self.diameter ← self.radius * 2
    }
}

main {
    const Terminus inside ← Terminus { valor = 50 }
    print inside.valor
    const Terminus clamped ← Terminus { valor = 200 }
    print clamped.valor
    const Circulus c ← Circulus { radius = 5 }
    print c.diameter
}
"#,
    );
    assert_eq!(lines, ["50", "100", "10"]);
}

#[test]
fn a_genus_without_a_creo_hook_constructs_unchanged() {
    let lines = run_lines(
        "creo-hook-absent.fab",
        r#"+++
locale = "en"
+++
class Plain {
    var int valor = 0
}

main {
    const Plain p ← Plain { valor = 200 }
    print p.valor
}
"#,
    );
    assert_eq!(lines, ["200"]);
}
