//! Blocking entropy leaf. Serves Latin `aleator:*` routes.
//!
//! Seedable draws (`fractum`, `sortire`) use xoshiro256** (same split as the
//! retired runner kernel / handroll). `octetos` and `uuid` read OS entropy
//! and ignore `semina`. `n ≤ 0` on `semina` returns seedable draws to unseeded
//! OS/thread entropy.

#[cfg(test)]
#[path = "entropy_test.rs"]
mod tests;

use crate::error::LeafError;
use crate::value::{Arg, Reply};
use std::io::{self, Read};
use std::sync::{Mutex, MutexGuard};

struct RngState {
    seeded: Option<Rng>,
}

static RNG_STATE: Mutex<RngState> = Mutex::new(RngState { seeded: None });

/// Clear the deterministic seed at an execution boundary.
pub fn reset_seed() {
    rng_state().seeded = None;
}

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

/// Whether `route` belongs to this leaf.
#[must_use]
pub fn is_route(route: &str) -> bool {
    serve(route, &[]).is_some()
}

/// Answer an `aleator:*` route. `None` means the route is not this leaf.
pub fn serve(route: &str, args: &[Arg]) -> Option<Result<Reply, LeafError>> {
    let result = match route {
        "aleator:fractum" => random_float(args),
        "aleator:sortire" => random_int(args),
        "aleator:octetos" => random_bytes(args),
        "aleator:uuid" => uuid(args),
        "aleator:semina" => seed(args),
        _ => return None,
    };
    Some(result)
}

fn empty_ok(route: &'static str, args: &[Arg]) -> Result<(), LeafError> {
    if args.is_empty() {
        Ok(())
    } else {
        Err(LeafError::invalid_argument(route, "unexpected arguments"))
    }
}

fn expect_int(
    route: &'static str,
    args: &[Arg],
    index: usize,
    name: &str,
) -> Result<i64, LeafError> {
    match args.get(index) {
        Some(Arg::Int(value)) => Ok(*value),
        Some(_) => Err(LeafError::invalid_argument(
            route,
            format!("{name} must be numerus"),
        )),
        None => Err(LeafError::invalid_argument(
            route,
            format!("missing {name}"),
        )),
    }
}

fn random_float(args: &[Arg]) -> Result<Reply, LeafError> {
    const ROUTE: &str = "aleator:fractum";
    empty_ok(ROUTE, args)?;
    Ok(Reply::Float(with_rng(Rng::gen_f64)))
}

fn random_int(args: &[Arg]) -> Result<Reply, LeafError> {
    const ROUTE: &str = "aleator:sortire";
    let min = expect_int(ROUTE, args, 0, "min")?;
    let max = expect_int(ROUTE, args, 1, "max")?;
    if min > max {
        return Err(LeafError::invalid_argument(ROUTE, "min must be ≤ max"));
    }
    Ok(Reply::Int(with_rng(|rng| rng.gen_i64_inclusive(min, max))))
}

fn random_bytes(args: &[Arg]) -> Result<Reply, LeafError> {
    const ROUTE: &str = "aleator:octetos";
    let n = expect_int(ROUTE, args, 0, "n")?;
    if n < 0 {
        return Err(LeafError::invalid_argument(ROUTE, "n must be non-negative"));
    }
    let len =
        usize::try_from(n).map_err(|_| LeafError::invalid_argument(ROUTE, "n is too large"))?;
    let mut bytes = vec![0_u8; len];
    if len > 0 {
        os_entropy(&mut bytes).map_err(|err| LeafError::unavailable(ROUTE, err.to_string()))?;
    }
    Ok(Reply::Bytes(bytes))
}

fn uuid(args: &[Arg]) -> Result<Reply, LeafError> {
    const ROUTE: &str = "aleator:uuid";
    empty_ok(ROUTE, args)?;
    uuid_v4()
        .map(Reply::Str)
        .map_err(|err| LeafError::unavailable(ROUTE, err.to_string()))
}

fn seed(args: &[Arg]) -> Result<Reply, LeafError> {
    const ROUTE: &str = "aleator:semina";
    let n = expect_int(ROUTE, args, 0, "n")?;
    let mut guard = rng_state();
    guard.seeded = u64::try_from(n).ok().map(Rng::seed_from_u64);
    Ok(Reply::Unit)
}

/// xoshiro256** — same generator the retired aleator kernel used via handroll.
struct Rng {
    state: [u64; 4],
}

impl Rng {
    fn seed_from_u64(seed: u64) -> Self {
        let mut sm = seed;
        let mut state = [0_u64; 4];
        for word in &mut state {
            *word = splitmix64(&mut sm);
        }
        Self { state }
    }

    fn from_entropy() -> Self {
        let mut seed = [0_u8; 32];
        let mut state = [0_u64; 4];
        if os_entropy(&mut seed).is_ok() {
            let (words, _) = seed.as_chunks::<8>();
            for (word, bytes) in state.iter_mut().zip(words) {
                *word = u64::from_le_bytes(*bytes);
            }
        }
        if state == [0_u64; 4] {
            Self::seed_from_u64(degraded_seed())
        } else {
            Self { state }
        }
    }

    fn next_u64(&mut self) -> u64 {
        let state = &mut self.state;
        let result = state[1].wrapping_mul(5).rotate_left(7).wrapping_mul(9);
        let t = state[1] << 17;
        state[2] ^= state[0];
        state[3] ^= state[1];
        state[1] ^= state[2];
        state[0] ^= state[3];
        state[2] ^= t;
        state[3] = state[3].rotate_left(45);
        result
    }

    fn gen_f64(&mut self) -> f64 {
        const ONE_HALF_OPEN: u64 = 0x3FF0_0000_0000_0000;
        f64::from_bits(ONE_HALF_OPEN | (self.next_u64() >> 12)) - 1.0
    }

    fn gen_i64_inclusive(&mut self, lo: i64, hi: i64) -> i64 {
        let span = u64::from_le_bytes(hi.to_le_bytes())
            .wrapping_sub(u64::from_le_bytes(lo.to_le_bytes()))
            .wrapping_add(1);
        if span == 0 {
            lo.wrapping_add(i64::from_le_bytes(self.next_u64().to_le_bytes()))
        } else {
            let sample = self.next_u64_below(span);
            lo.wrapping_add(i64::from_le_bytes(sample.to_le_bytes()))
        }
    }

    #[allow(clippy::cast_possible_truncation)]
    fn next_u64_below(&mut self, range: u64) -> u64 {
        let mut x = self.next_u64();
        let mut product = u128::from(x) * u128::from(range);
        let mut low = product as u64;
        if low < range {
            let threshold = range.wrapping_neg() % range;
            while low < threshold {
                x = self.next_u64();
                product = u128::from(x) * u128::from(range);
                low = product as u64;
            }
        }
        (product >> 64) as u64
    }
}

fn splitmix64(seed: &mut u64) -> u64 {
    *seed = seed.wrapping_add(0x9E37_79B9_7F4A_7C15);
    let mut z = *seed;
    z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
    z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
    z ^ (z >> 31)
}

#[cfg(unix)]
fn os_entropy(buf: &mut [u8]) -> io::Result<()> {
    std::fs::File::open("/dev/urandom")?.read_exact(buf)
}

#[cfg(not(unix))]
fn os_entropy(buf: &mut [u8]) -> io::Result<()> {
    let mut sm = degraded_seed();
    for chunk in buf.chunks_mut(8) {
        let bytes = splitmix64(&mut sm).to_le_bytes();
        chunk.copy_from_slice(&bytes[..chunk.len()]);
    }
    Ok(())
}

fn degraded_seed() -> u64 {
    use std::time::{SystemTime, UNIX_EPOCH};
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |elapsed| elapsed.as_nanos());
    let bytes = nanos.to_le_bytes();
    let (halves, _) = bytes.as_chunks::<8>();
    let nanos_lo = u64::from_le_bytes(halves[0]);
    #[allow(clippy::cast_possible_truncation)]
    let nanos_hi = (nanos >> 64) as u64;
    let local = 0_u64;
    let stack = std::ptr::addr_of!(local) as usize as u64;
    nanos_lo ^ nanos_hi ^ stack ^ u64::from(std::process::id()).rotate_left(17)
}

fn uuid_v4() -> io::Result<String> {
    let mut bytes = [0_u8; 16];
    os_entropy(&mut bytes)?;
    bytes[6] = (bytes[6] & 0x0f) | 0x40;
    bytes[8] = (bytes[8] & 0x3f) | 0x80;
    Ok(format!(
        "{:02x}{:02x}{:02x}{:02x}-{:02x}{:02x}-{:02x}{:02x}-{:02x}{:02x}-{:02x}{:02x}{:02x}{:02x}{:02x}{:02x}",
        bytes[0],
        bytes[1],
        bytes[2],
        bytes[3],
        bytes[4],
        bytes[5],
        bytes[6],
        bytes[7],
        bytes[8],
        bytes[9],
        bytes[10],
        bytes[11],
        bytes[12],
        bytes[13],
        bytes[14],
        bytes[15],
    ))
}
