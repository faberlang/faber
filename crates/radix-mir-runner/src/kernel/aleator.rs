use super::super::{RunnerError, Value};
use handroll::entropy::{self, Rng};
use handroll::uuid_v4;
use radix_lexer::Span;
use std::sync::{Mutex, MutexGuard};

struct RngState {
    seeded: Option<Rng>,
}

static RNG_STATE: Mutex<RngState> = Mutex::new(RngState { seeded: None });

fn rng_state() -> MutexGuard<'static, RngState> {
    match RNG_STATE.lock() {
        Ok(guard) => guard,
        Err(poisoned) => poisoned.into_inner(),
    }
}

fn with_rng<R>(f: impl FnOnce(&mut Rng) -> R) -> R {
    let mut guard = rng_state();
    if let Some(ref mut rng) = guard.seeded {
        f(rng)
    } else {
        let mut rng = Rng::from_entropy();
        f(&mut rng)
    }
}

/// Dispatch for the `faber:aleator` kernel module.
///
/// `fractum`/`sortire` draw from the seeded (or entropy-initialized) RNG;
/// `octetos`/`uuid` read fresh OS entropy from `handroll::entropy::os_entropy`
/// and are never influenced by `semina` (handroll U5b replaces the former
/// rand/getrandom paths; sequence values changed, semantics preserved).
pub(crate) fn dispatch(verb: &str, args: &[Value], span: Span) -> Result<Value, RunnerError> {
    match verb {
        "fractum" => {
            if !args.is_empty() {
                return Err(RunnerError::internal(
                    span,
                    "aleator.fractum provider arity",
                ));
            }
            Ok(Value::Float(with_rng(Rng::gen_f64)))
        }
        "sortire" => {
            let [min, max] = args else {
                return Err(RunnerError::internal(
                    span,
                    "aleator.sortire provider arity",
                ));
            };
            let min = match min {
                Value::Int(v) => *v,
                _ => {
                    return Err(RunnerError::internal(
                        span,
                        "aleator.sortire min expects numerus",
                    ))
                }
            };
            let max = match max {
                Value::Int(v) => *v,
                _ => {
                    return Err(RunnerError::internal(
                        span,
                        "aleator.sortire max expects numerus",
                    ))
                }
            };
            let (lo, hi) = if min <= max { (min, max) } else { (max, min) };
            Ok(Value::Int(with_rng(|rng| rng.gen_i64_inclusive(lo, hi))))
        }
        "octetos" => {
            let [n] = args else {
                return Err(RunnerError::internal(
                    span,
                    "aleator.octetos provider arity",
                ));
            };
            let n = match n {
                Value::Int(v) => *v,
                _ => {
                    return Err(RunnerError::internal(
                        span,
                        "aleator.octetos expects numerus",
                    ))
                }
            };
            let len = usize::try_from(n.max(0))
                .map_err(|_| RunnerError::internal(span, "aleator.octetos length out of range"))?;
            let mut bytes = vec![0_u8; len];
            if len > 0 {
                entropy::os_entropy(&mut bytes).map_err(|err| {
                    RunnerError::internal(span, format!("aleator.octetos failed: {err}"))
                })?;
            }
            Ok(Value::Octeti(bytes))
        }
        "uuid" => {
            if !args.is_empty() {
                return Err(RunnerError::internal(span, "aleator.uuid provider arity"));
            }
            uuid_v4()
                .map(Value::Str)
                .map_err(|err| RunnerError::internal(span, format!("aleator.uuid failed: {err}")))
        }
        "semina" => {
            let [n] = args else {
                return Err(RunnerError::internal(span, "aleator.semina provider arity"));
            };
            let n = match n {
                Value::Int(v) => *v,
                _ => {
                    return Err(RunnerError::internal(
                        span,
                        "aleator.semina expects numerus",
                    ))
                }
            };
            let mut guard = rng_state();
            guard.seeded = u64::try_from(n).ok().map(Rng::seed_from_u64);
            Ok(Value::Unit)
        }
        other => Err(RunnerError::unsupported(
            span,
            &format!("kernel aleator verb `{other}`"),
        )),
    }
}

#[cfg(test)]
mod tests {
    use handroll::uuid_v4;
    use regex::Regex;

    #[test]
    fn uuid_v4_is_canonical_lowercase_v4_shape() {
        let shape =
            Regex::new(r"^[0-9a-f]{8}-[0-9a-f]{4}-4[0-9a-f]{3}-[89ab][0-9a-f]{3}-[0-9a-f]{12}$")
                .expect("valid v4 shape regex");
        for _ in 0..64 {
            let uuid = uuid_v4().expect("entropy source available");
            assert!(
                shape.is_match(&uuid),
                "uuid `{uuid}` does not match the canonical v4 shape"
            );
        }
    }

    #[test]
    fn uuid_v4_pins_version_and_variant_bits() {
        let uuid = uuid_v4().expect("entropy source available");
        let hex = uuid.replace('-', "");
        let mut bytes = [0_u8; 16];
        for (i, pair) in hex.as_bytes().chunks_exact(2).enumerate() {
            bytes[i] = u8::from_str_radix(std::str::from_utf8(pair).expect("hex pairs"), 16)
                .expect("valid hex");
        }
        assert_eq!(bytes[6] >> 4, 0x4, "version nibble must be 4");
        assert_eq!(bytes[8] >> 6, 0b10, "variant bits must be 10xx");
    }

    #[test]
    fn uuid_v4_calls_differ() {
        assert_ne!(
            uuid_v4().expect("entropy source available"),
            uuid_v4().expect("entropy source available")
        );
    }
}
