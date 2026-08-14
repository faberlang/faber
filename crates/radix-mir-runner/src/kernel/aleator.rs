use super::super::{RunnerError, Value};
use radix_lexer::Span;
use rand::rngs::StdRng;
use rand::{Rng, SeedableRng};
use std::sync::{Mutex, MutexGuard};

struct RngState {
    seeded: Option<StdRng>,
}

static RNG_STATE: Mutex<RngState> = Mutex::new(RngState { seeded: None });

fn rng_state() -> MutexGuard<'static, RngState> {
    match RNG_STATE.lock() {
        Ok(guard) => guard,
        Err(poisoned) => poisoned.into_inner(),
    }
}

fn with_rng<R>(f: impl FnOnce(&mut StdRng) -> R) -> R {
    let mut guard = rng_state();
    if let Some(ref mut rng) = guard.seeded {
        f(rng)
    } else {
        let mut rng = StdRng::from_entropy();
        f(&mut rng)
    }
}

/// Generate a canonical RFC 9562 (v4) UUID string, `8-4-4-4-12` lowercase hex.
///
/// Entropy is 16 bytes from `getrandom` (the same dependency the `octetos`
/// provider uses — no new entropy path). The version nibble is pinned to 4
/// (byte 6, `0x40`) and the RFC 4122 variant bits to `10xx` (byte 8, `0x80`).
pub(crate) fn uuid_v4() -> Result<String, getrandom::Error> {
    let mut bytes = [0_u8; 16];
    getrandom::getrandom(&mut bytes)?;
    bytes[6] = (bytes[6] & 0x0f) | 0x40;
    bytes[8] = (bytes[8] & 0x3f) | 0x80;
    Ok(format!(
        "{:02x}{:02x}{:02x}{:02x}-{:02x}{:02x}-{:02x}{:02x}-{:02x}{:02x}-{:02x}{:02x}{:02x}{:02x}{:02x}{:02x}",
        bytes[0], bytes[1], bytes[2], bytes[3], bytes[4], bytes[5], bytes[6], bytes[7],
        bytes[8], bytes[9], bytes[10], bytes[11], bytes[12], bytes[13], bytes[14], bytes[15],
    ))
}

pub(crate) fn dispatch(verb: &str, args: &[Value], span: Span) -> Result<Value, RunnerError> {
    match verb {
        "fractum" => {
            if !args.is_empty() {
                return Err(RunnerError::internal(
                    span,
                    "aleator.fractum provider arity",
                ));
            }
            Ok(Value::Float(with_rng(|rng| rng.gen_range(0.0..1.0))))
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
            Ok(Value::Int(with_rng(|rng| rng.gen_range(lo..=hi))))
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
                getrandom::getrandom(&mut bytes).map_err(|err| {
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
            guard.seeded = u64::try_from(n).ok().map(StdRng::seed_from_u64);
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
    use super::uuid_v4;
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
