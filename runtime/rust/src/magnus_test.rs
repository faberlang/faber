// WHY: the float assertions are exact-bit claims (ties, overflow boundary), not tolerances;
// the single-letter names are the arithmetic's own (a, b, q, r, u, v).
#![allow(clippy::float_cmp, clippy::many_single_char_names)]

use super::{Magnus, MagnusError, Radix};
use std::collections::hash_map::DefaultHasher;
use std::hash::{Hash, Hasher};

// ---- deterministic generator (SplitMix64; no external crate) -----------------------------

struct Gen(u64);

impl Gen {
    fn next(&mut self) -> u64 {
        self.0 = self.0.wrapping_add(0x9E37_79B9_7F4A_7C15);
        let mut z = self.0;
        z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
        z ^ (z >> 31)
    }

    fn u128(&mut self) -> u128 {
        (u128::from(self.next()) << 64) | u128::from(self.next())
    }

    /// Uniform bit length in `0..=max_bits`, then random bits and a random sign: hits every
    /// limb count and plenty of small and boundary-ish values.
    fn signed(&mut self, max_bits: u32) -> i128 {
        let bits = u32::try_from(self.next() % (u64::from(max_bits) + 1)).unwrap();
        let mask = if bits == 0 {
            0
        } else {
            u128::MAX >> (128 - bits)
        };
        let magnitude = i128::try_from(self.u128() & mask).unwrap();
        if self.next() & 1 == 1 {
            -magnitude
        } else {
            magnitude
        }
    }
}

fn m(x: i128) -> Magnus {
    Magnus::from_i128(x)
}

fn floor_divmod(a: i128, b: i128) -> (i128, i128) {
    let (mut q, mut r) = (a / b, a % b);
    if r != 0 && ((r < 0) != (b < 0)) {
        q -= 1;
        r += b;
    }
    (q, r)
}

fn dec(text: &str) -> Magnus {
    Magnus::parse_decimal(text).unwrap()
}

fn hash_of(x: &Magnus) -> u64 {
    let mut h = DefaultHasher::new();
    x.hash(&mut h);
    h.finish()
}

const ITER: usize = 3000;
const B120: u32 = 120;

// ---- property tests against i128 ---------------------------------------------------------

#[test]
fn prop_add_sub_neg_match_i128() {
    let mut g = Gen(1);
    for _ in 0..ITER {
        let (a, b) = (g.signed(B120), g.signed(B120));
        assert_eq!((&m(a) + &m(b)).to_i128(), Some(a + b), "{a} + {b}");
        assert_eq!((&m(a) - &m(b)).to_i128(), Some(a - b), "{a} - {b}");
        assert_eq!((-m(a)).to_i128(), Some(-a), "-{a}");
        assert_eq!((m(a) + m(b)).to_i128(), Some(a + b), "owned operands");
    }
}

#[test]
fn prop_mul_matches_i128() {
    let mut g = Gen(2);
    for _ in 0..ITER {
        let (a, b) = (g.signed(62), g.signed(62));
        assert_eq!((&m(a) * &m(b)).to_i128(), Some(a * b), "{a} * {b}");
        let (c, d) = (g.signed(B120), g.signed(6));
        assert_eq!((&m(c) * &m(d)).to_i128(), Some(c * d), "{c} * {d}");
    }
}

#[test]
fn prop_mul_large_then_div_recovers_operand() {
    // Products far beyond i128: (a*b)/b == a and (a*b)%b == 0 checks mul and Knuth D together.
    let mut g = Gen(22);
    for _ in 0..500 {
        let big = |g: &mut Gen| {
            (0..6).fold(Magnus::one(), |acc, _| {
                &(&acc * &Magnus::from_u64(g.next() | 1)) + &m(g.signed(40))
            })
        };
        let (a, b) = (big(&mut g), big(&mut g));
        if b.is_zero() {
            continue;
        }
        let product = &a * &b;
        let (q, r) = product.div_mod_floor(&b).unwrap();
        assert_eq!(q, a);
        assert!(r.is_zero());
    }
}

#[test]
fn prop_div_mod_floor_match_i128() {
    let mut g = Gen(3);
    for _ in 0..ITER {
        let a = g.signed(B120);
        let mut b = g.signed(B120);
        if b == 0 {
            b = 7;
        }
        let (q, r) = floor_divmod(a, b);
        let (mq, mr) = m(a).div_mod_floor(&m(b)).unwrap();
        assert_eq!(mq.to_i128(), Some(q), "{a} / {b}");
        assert_eq!(mr.to_i128(), Some(r), "{a} % {b}");
        assert_eq!(m(a).div_floor(&m(b)).unwrap(), mq);
        assert_eq!(m(a).mod_floor(&m(b)).unwrap(), mr);
        // Floor identity and remainder range hold on the Magnus side too.
        assert_eq!(&(&m(b) * &mq) + &mr, m(a));
        assert!(mr.is_zero() || mr.is_negative() == m(b).is_negative());
    }
}

#[test]
fn prop_bitwise_and_not_match_i128_including_negatives() {
    let mut g = Gen(4);
    for _ in 0..ITER {
        let (a, b) = (g.signed(B120), g.signed(B120));
        assert_eq!(m(a).bit_and(&m(b)).to_i128(), Some(a & b), "{a} & {b}");
        assert_eq!(m(a).bit_or(&m(b)).to_i128(), Some(a | b), "{a} | {b}");
        assert_eq!(m(a).bit_xor(&m(b)).to_i128(), Some(a ^ b), "{a} ^ {b}");
        assert_eq!(m(a).bit_not().to_i128(), Some(!a), "!{a}");
    }
}

#[test]
fn prop_shifts_match_i128() {
    let mut g = Gen(5);
    for _ in 0..ITER {
        let a = g.signed(B120);
        let n = g.next() % 140;
        let right = if n >= 127 {
            if a < 0 { -1 } else { 0 }
        } else {
            a >> n
        };
        assert_eq!(m(a).shr(n).to_i128(), Some(right), "{a} >> {n}");
        let small = g.signed(60);
        let k = g.next() % 60;
        assert_eq!(
            m(small).shl(k).unwrap().to_i128(),
            Some(small << k),
            "{small} << {k}"
        );
    }
}

#[test]
fn prop_potentia_matches_i128() {
    let mut g = Gen(6);
    for _ in 0..ITER {
        let base = g.signed(14);
        let exp = g.next() % 10;
        let want = base.checked_pow(u32::try_from(exp).unwrap());
        let got = m(base).potentia(&Magnus::from_u64(exp)).unwrap();
        match want {
            Some(v) => assert_eq!(got.to_i128(), Some(v), "{base}^{exp}"),
            None => assert!(got.to_i128().is_none(), "{base}^{exp} overflows i128"),
        }
    }
}

#[test]
fn prop_cmp_eq_hash_ord_consistent_with_i128() {
    let mut g = Gen(7);
    let mut values: Vec<i128> = (0..400).map(|_| g.signed(B120)).collect();
    for &a in values.iter().take(150) {
        let b = g.signed(B120);
        assert_eq!(m(a).cmp(&m(b)), a.cmp(&b), "{a} vs {b}");
        assert_eq!(m(a) == m(b), a == b);
        assert_eq!(m(a).partial_cmp(&m(b)), Some(a.cmp(&b)));
        // The same value reached by a different route is equal and hashes equal.
        let rebuilt = &(&m(a) + &m(b)) - &m(b);
        assert_eq!(rebuilt, m(a));
        assert_eq!(hash_of(&rebuilt), hash_of(&m(a)));
    }
    let mut magnus: Vec<Magnus> = values.iter().map(|&v| m(v)).collect();
    values.sort_unstable();
    magnus.sort();
    let sorted: Vec<Option<i128>> = magnus.iter().map(Magnus::to_i128).collect();
    assert_eq!(sorted, values.iter().map(|&v| Some(v)).collect::<Vec<_>>());
}

#[test]
fn zero_is_canonical_and_never_negative() {
    let zero = Magnus::zero();
    let negated = -Magnus::zero();
    let cancelled = &m(5) - &m(5);
    let shifted = m(-1).shr(10).abs() - Magnus::one();
    for z in [&negated, &cancelled, &shifted] {
        assert_eq!(*z, zero);
        assert_eq!(hash_of(z), hash_of(&zero));
        assert!(!z.is_negative());
        assert_eq!(z.signum(), 0);
        assert_eq!(z.bit_len(), 0);
    }
    assert_eq!(dec("-0"), zero);
    assert_eq!(m(-3).signum(), -1);
    assert_eq!(m(3).signum(), 1);
    assert!(Magnus::zero().is_zero() && !m(1).is_zero());
    assert!(m(-3).is_odd() && !m(-4).is_odd());
}

// ---- known vectors ------------------------------------------------------------------------

#[test]
fn two_to_the_200_and_fifty_factorial() {
    let two_200 = Magnus::from(2_u64)
        .potentia(&Magnus::from(200_u64))
        .unwrap();
    assert_eq!(
        two_200.to_string(),
        "1606938044258990275541962092341162602522202993782792835301376"
    );
    assert_eq!(Magnus::one().shl(200).unwrap(), two_200);
    assert_eq!(two_200.bit_len(), 201);

    let fact = (1..=50_u64).fold(Magnus::one(), |acc, k| &acc * &Magnus::from_u64(k));
    assert_eq!(
        fact.to_string(),
        "30414093201713378043612608166064768844377641568960512000000000000"
    );
    // 50! / 49! == 50 and 50! % 49! == 0.
    let fact49 = (1..=49_u64).fold(Magnus::one(), |acc, k| &acc * &Magnus::from_u64(k));
    assert_eq!(fact.div_floor(&fact49).unwrap(), m(50));
    assert!(fact.mod_floor(&fact49).unwrap().is_zero());
    assert_eq!(
        Magnus::from(3_u64)
            .potentia(&Magnus::from(100_u64))
            .unwrap()
            .to_string(),
        "515377520732011331036461129765621272702107522001"
    );
}

#[test]
fn ten_pow_40_over_ten_pow_20() {
    let ten = Magnus::from(10_u64);
    let p40 = ten.potentia(&Magnus::from(40_u64)).unwrap();
    let p20 = ten.potentia(&Magnus::from(20_u64)).unwrap();
    assert_eq!(p40.to_string(), format!("1{}", "0".repeat(40)));
    let (q, r) = p40.div_mod_floor(&p20).unwrap();
    assert_eq!(q, p20);
    assert!(r.is_zero());
    // One more than the dividend leaves remainder 1; negative dividend floors away from zero.
    let (q1, r1) = (&p40 + &Magnus::one()).div_mod_floor(&p20).unwrap();
    assert_eq!((q1, r1), (p20.clone(), Magnus::one()));
    let (qn, rn) = (-(&p40 + &Magnus::one())).div_mod_floor(&p20).unwrap();
    assert_eq!(qn, -(&p20 + &Magnus::one()));
    assert_eq!(rn, &p20 - &Magnus::one());
}

fn limbs_to_magnus(limbs: &[u32]) -> Magnus {
    Magnus::from_parts(false, limbs.to_vec())
}

/// Independent slow reference: binary long division on magnitudes.
fn slow_divmod(u: &Magnus, v: &Magnus) -> (Magnus, Magnus) {
    let mut q = Magnus::zero();
    let mut r = Magnus::zero();
    for bit in (0..u.bit_len()).rev() {
        r = r.shl(1).unwrap();
        if super::get_bit(&u.mag, usize::try_from(bit).unwrap()) {
            r = &r + &Magnus::one();
        }
        q = q.shl(1).unwrap();
        if r >= *v {
            r = &r - v;
            q = &q + &Magnus::one();
        }
    }
    (q, r)
}

#[test]
fn knuth_d_add_back_vector() {
    // The Hacker's Delight vector that forces step D6 (qhat one too large, add back):
    // u = 0x8000_0000_0000_0000_0000_0003, v = 0x2000_0000_0000_0001.
    // The true quotient is 3; qhat overshoots to 4 before the add-back repairs it.
    let u = limbs_to_magnus(&[3, 0, 0x8000_0000]);
    let v = limbs_to_magnus(&[1, 0, 0x2000_0000]);
    let (q, r) = u.div_mod_floor(&v).unwrap();
    assert_eq!(q, m(3));
    assert_eq!(r, limbs_to_magnus(&[0, 0, 0x2000_0000]));
    assert_eq!(&(&v * &q) + &r, u);
}

#[test]
fn knuth_d_qhat_correction_vectors() {
    // qhat correction (step D3), third-limb test: the two-limb estimate is 2 but the true
    // quotient digit is 1 (u = 2^64, v = 0x8000_0000_0000_0001).
    let u = limbs_to_magnus(&[0, 0, 1]);
    let v = limbs_to_magnus(&[1, 0x8000_0000]);
    assert_eq!(u.div_mod_floor(&v).unwrap(), (m(1), &u - &v));
    // qhat == BASE branch: the window's top limb equals the divisor's top limb, so the first
    // estimate is 2^32 and must be clamped before the multiply.
    let u = limbs_to_magnus(&[0, 0, 0x8000_0000]);
    let v = limbs_to_magnus(&[0xFFFF_FFFF, 0x8000_0000]);
    let (q, r) = u.div_mod_floor(&v).unwrap();
    assert_eq!(&(&v * &q) + &r, u);
    assert!(r < v);
    assert_eq!((q, r), slow_divmod(&u, &v));

    // All-ones dividend by all-ones-ish divisors (the classic worst cases for qhat).
    let u = limbs_to_magnus(&[0xFFFF_FFFF; 6]);
    for v in [
        limbs_to_magnus(&[0xFFFF_FFFF, 0xFFFF_FFFF]),
        limbs_to_magnus(&[0, 0x8000_0000]),
        limbs_to_magnus(&[0xFFFF_FFFF, 0x8000_0000, 0xFFFF_FFFF]),
    ] {
        let (q, r) = u.div_mod_floor(&v).unwrap();
        assert_eq!((q, r), slow_divmod(&u, &v));
    }
}

#[test]
fn knuth_d_structured_sweep_against_slow_reference() {
    // Every combination of edge limbs: dividends of 3 limbs (and 4 over a narrower set),
    // divisors of 2 and 3 limbs with a non-zero top limb.
    let edge = [0_u32, 1, 0x7FFF_FFFF, 0x8000_0000, 0xFFFF_FFFF];
    let mut checked = 0_usize;
    for u0 in edge {
        for u1 in edge {
            for u2 in edge {
                let u = limbs_to_magnus(&[u0, u1, u2]);
                for v0 in edge {
                    for v1 in edge {
                        if v1 == 0 {
                            continue;
                        }
                        let v = limbs_to_magnus(&[v0, v1]);
                        let (q, r) = u.div_mod_floor(&v).unwrap();
                        assert_eq!((q, r), slow_divmod(&u, &v), "{u:?} / {v:?}");
                        checked += 1;
                        for v2 in edge {
                            if v2 == 0 {
                                continue;
                            }
                            let w = limbs_to_magnus(&[v0, v1, v2]);
                            assert_eq!(u.div_mod_floor(&w).unwrap(), slow_divmod(&u, &w));
                            checked += 1;
                        }
                    }
                }
            }
        }
    }
    assert!(checked > 2000, "sweep ran {checked} divisions");
}

#[test]
fn knuth_d_random_wide_operands_against_slow_reference() {
    let mut g = Gen(8);
    for _ in 0..200 {
        let limbs = |g: &mut Gen, n: usize| {
            let mut v: Vec<u32> = (0..n)
                .map(|_| u32::try_from(g.next() & 0xFFFF_FFFF).unwrap())
                .collect();
            // Bias toward the boundary limbs Algorithm D is sensitive to.
            if g.next().is_multiple_of(3) {
                v[0] = 0xFFFF_FFFF;
            }
            limbs_to_magnus(&v)
        };
        let (un, vn) = (2 + g.next() % 6, 2 + g.next() % 3);
        let u = limbs(&mut g, usize::try_from(un).unwrap());
        let v = limbs(&mut g, usize::try_from(vn).unwrap());
        if v.is_zero() {
            continue;
        }
        assert_eq!(u.div_mod_floor(&v).unwrap(), slow_divmod(&u, &v));
    }
}

#[test]
fn floor_div_mod_sign_matrix() {
    // (a, b, floor quotient, floor remainder)
    let table: [(i128, i128, i128, i128); 12] = [
        (7, 2, 3, 1),
        (-7, 2, -4, 1),
        (7, -2, -4, -1),
        (-7, -2, 3, -1),
        (8, 2, 4, 0),
        (-8, 2, -4, 0),
        (8, -2, -4, 0),
        (-8, -2, 4, 0),
        (0, 5, 0, 0),
        (0, -5, 0, 0),
        (1, 5, 0, 1),
        (-1, 5, -1, 4),
    ];
    for (a, b, q, r) in table {
        let (mq, mr) = m(a).div_mod_floor(&m(b)).unwrap();
        assert_eq!(
            (mq.to_i128(), mr.to_i128()),
            (Some(q), Some(r)),
            "{a} div {b}"
        );
    }
    // The same matrix on multi-limb values (scale everything by 2^100 and 3).
    let big = Magnus::one().shl(100).unwrap();
    let three = m(3);
    for (a, b) in [(7, 2), (-7, 2), (7, -2), (-7, -2), (-1, 5)] {
        let ma = &(&m(a) * &big) + &three;
        let mb = &m(b) * &big;
        let (q, r) = ma.div_mod_floor(&mb).unwrap();
        assert_eq!(&(&mb * &q) + &r, ma);
        assert!(r.is_zero() || r.is_negative() == mb.is_negative());
        assert!(r.abs() < mb.abs());
    }
    assert_eq!(
        m(5).div_floor(&Magnus::zero()),
        Err(MagnusError::DivisionByZero)
    );
    assert_eq!(
        m(5).mod_floor(&Magnus::zero()),
        Err(MagnusError::DivisionByZero)
    );
    assert_eq!(
        Magnus::zero().div_mod_floor(&Magnus::zero()),
        Err(MagnusError::DivisionByZero)
    );
}

// ---- potentia, shifts, ceilings -----------------------------------------------------------

#[test]
fn potentia_edge_cases() {
    let pow = |b: i128, e: i128| m(b).potentia(&m(e));
    assert_eq!(pow(0, 0), Ok(Magnus::one()), "0^0 == 1");
    assert_eq!(pow(2, 0), Ok(Magnus::one()));
    assert_eq!(pow(-7, 0), Ok(Magnus::one()));
    assert_eq!(pow(0, 5), Ok(Magnus::zero()));
    assert_eq!(pow(1, 1_000_000), Ok(Magnus::one()));
    assert_eq!(pow(-2, 3), Ok(m(-8)));
    assert_eq!(pow(-2, 4), Ok(m(16)));
    // A negative exponent is the ruling-28 error, for every base including 0 and 1.
    for base in [-3, -1, 0, 1, 2] {
        assert_eq!(
            pow(base, -1),
            Err(MagnusError::NegativeExponent),
            "{base}^-1"
        );
    }
    // Exponents beyond u64: only 0, 1, -1 are representable.
    let huge_even = Magnus::one().shl(70).unwrap();
    let huge_odd = &huge_even + &Magnus::one();
    assert_eq!(m(1).potentia(&huge_odd), Ok(Magnus::one()));
    assert_eq!(m(-1).potentia(&huge_even), Ok(Magnus::one()));
    assert_eq!(m(-1).potentia(&huge_odd), Ok(m(-1)));
    assert_eq!(m(0).potentia(&huge_odd), Ok(Magnus::zero()));
    assert_eq!(m(2).potentia(&huge_odd), Err(MagnusError::Unrepresentable));
    // Just-beyond-u64 exponent boundary handled, u64::MAX itself with base 1 is fine.
    assert_eq!(m(-1).potentia(&Magnus::from_u64(u64::MAX)), Ok(m(-1)));
}

#[test]
fn shifts_by_limb_width_and_large_counts() {
    let one = Magnus::one();
    for n in [0_u64, 1, 31, 32, 33, 63, 64, 65, 95, 96, 97, 1000, 4096] {
        let up = one.shl(n).unwrap();
        assert_eq!(up.bit_len(), n + 1, "1 << {n}");
        assert_eq!(up.shr(n), one, "(1 << {n}) >> {n}");
        assert_eq!(up.shr(n + 1), Magnus::zero());
        let down = (-one.clone()).shl(n).unwrap();
        assert_eq!(down.shr(n), m(-1));
    }
    // Shift right of negatives floors: -1 stays -1 forever, -5 >> 1 == -3, -4 >> 2 == -1.
    assert_eq!(m(-1).shr(1), m(-1));
    assert_eq!(m(-1).shr(u64::MAX), m(-1));
    assert_eq!(m(-5).shr(1), m(-3));
    assert_eq!(m(-4).shr(2), m(-1));
    assert_eq!(m(-4).shr(3), m(-1));
    assert_eq!(m(5).shr(u64::MAX), Magnus::zero());
    // Multi-limb negative floor shift agrees with floor division by 2^n.
    let x = &(&Magnus::one().shl(200).unwrap() * &m(-3)) - &m(1);
    for n in [1_u64, 31, 32, 33, 64, 199, 201, 250] {
        let pow = Magnus::one().shl(n).unwrap();
        assert_eq!(x.shr(n), x.div_floor(&pow).unwrap(), "x >> {n}");
    }
    // shl of zero is zero for any count; an unaddressable shift of a non-zero value errs.
    assert_eq!(Magnus::zero().shl(u64::MAX), Ok(Magnus::zero()));
    assert_eq!(m(1).shl(u64::MAX), Err(MagnusError::Unrepresentable));
    // Magnus-valued counts.
    assert_eq!(m(1).shl_by(&m(-1)), Err(MagnusError::NegativeShift));
    assert_eq!(m(1).shr_by(&m(-1)), Err(MagnusError::NegativeShift));
    assert_eq!(m(1).shl_by(&m(10)), Ok(m(1024)));
    assert_eq!(m(-1024).shr_by(&m(10)), Ok(m(-1)));
    assert_eq!(
        m(1024).shr_by(&Magnus::one().shl(80).unwrap()),
        Ok(Magnus::zero())
    );
    assert_eq!(
        Magnus::zero().shl_by(&Magnus::one().shl(80).unwrap()),
        Ok(Magnus::zero())
    );
    assert_eq!(
        m(1).shl_by(&Magnus::one().shl(80).unwrap()),
        Err(MagnusError::Unrepresentable)
    );
}

#[test]
fn ceiling_hooks_fail_fast() {
    assert!(m(2).potentia_within(&m(1000), 1001).is_ok());
    assert_eq!(
        m(2).potentia_within(&m(1000), 1000),
        Err(MagnusError::CeilingExceeded)
    );
    // Fails before computing: 3^(2^60) is never attempted.
    assert_eq!(
        m(3).potentia_within(&Magnus::from_u64(1 << 60), 1 << 31),
        Err(MagnusError::CeilingExceeded)
    );
    // Small bases are free regardless of the ceiling.
    assert_eq!(
        m(1).potentia_within(&Magnus::from_u64(1 << 60), 1),
        Ok(Magnus::one())
    );
    assert!(m(1).shl_within(99, 100).is_ok());
    assert_eq!(m(1).shl_within(100, 100), Err(MagnusError::CeilingExceeded));
    assert_eq!(Magnus::zero().shl_within(1 << 40, 1), Ok(Magnus::zero()));
    let big = Magnus::one().shl(99).unwrap();
    assert!(big.mul_within(&m(2), 101).is_ok());
    assert_eq!(
        big.mul_within(&m(4), 101),
        Err(MagnusError::CeilingExceeded)
    );
    assert!(big.exceeds_ceiling(99) && !big.exceeds_ceiling(100));
}

// ---- conversions --------------------------------------------------------------------------

#[test]
fn checked_narrowing_boundaries() {
    let cases: [(i128, Option<i64>, Option<u64>); 8] = [
        (0, Some(0), Some(0)),
        (-1, Some(-1), None),
        (
            i128::from(i64::MAX),
            Some(i64::MAX),
            Some(i64::MAX.unsigned_abs()),
        ),
        (i128::from(i64::MAX) + 1, None, Some(1 << 63)),
        (i128::from(i64::MIN), Some(i64::MIN), None),
        (i128::from(i64::MIN) - 1, None, None),
        (i128::from(u64::MAX), None, Some(u64::MAX)),
        (i128::from(u64::MAX) + 1, None, None),
    ];
    for (x, i, u) in cases {
        assert_eq!(m(x).to_i64(), i, "to_i64({x})");
        assert_eq!(m(x).to_u64(), u, "to_u64({x})");
    }
    assert_eq!(m(i128::MAX).to_i128(), Some(i128::MAX));
    assert_eq!(m(i128::MIN).to_i128(), Some(i128::MIN));
    assert_eq!((&m(i128::MAX) + &m(1)).to_i128(), None);
    assert_eq!((&m(i128::MIN) - &m(1)).to_i128(), None);
    assert_eq!(Magnus::from_u128(u128::MAX).to_u128(), Some(u128::MAX));
    assert_eq!((&Magnus::from_u128(u128::MAX) + &m(1)).to_u128(), None);
    assert_eq!(m(-1).to_u128(), None);
    assert_eq!(Magnus::from_u128(u128::MAX).to_i128(), None);
    assert_eq!(Magnus::from(i64::MIN).to_i64(), Some(i64::MIN));
    assert_eq!(Magnus::from(u64::MAX).to_u64(), Some(u64::MAX));
    assert_eq!(Magnus::from(-5_i32).to_i64(), Some(-5));
    assert_eq!(Magnus::from(5_u32).to_u64(), Some(5));
}

#[test]
fn low_u128_wrapping_is_twos_complement_mod_2_128() {
    let mut g = Gen(9);
    for _ in 0..ITER {
        let a = g.signed(B120);
        assert_eq!(m(a).low_u128_wrapping(), a.cast_unsigned(), "{a}");
    }
    let two_130 = Magnus::one().shl(130).unwrap();
    assert_eq!((&two_130 + &m(5)).low_u128_wrapping(), 5);
    assert_eq!((&m(7) - &two_130).low_u128_wrapping(), 7);
    assert_eq!(m(-1).low_u128_wrapping(), u128::MAX);
}

#[test]
fn to_f64_matches_i128_cast_nearest_even() {
    let mut g = Gen(10);
    for _ in 0..ITER {
        let a = g.signed(B120);
        // `i128 as f64` rounds to nearest, ties to even: the oracle.
        #[allow(clippy::cast_precision_loss)]
        let want = a as f64;
        assert_eq!(m(a).to_f64().to_bits(), want.to_bits(), "{a}");
    }
}

#[test]
fn to_f64_rounding_ties_and_sticky_bits() {
    let p = |e: u64| Magnus::one().shl(e).unwrap();
    // 2^53 + 1 is a tie between 2^53 (even mantissa) and 2^53 + 2: down. 2^53 + 3 ties up.
    assert_eq!((&p(53) + &m(1)).to_f64(), 9_007_199_254_740_992.0);
    assert_eq!((&p(53) + &m(3)).to_f64(), 9_007_199_254_740_996.0);
    // Beyond 64 bits the tie lives below the dropped limbs: the sticky bit decides.
    // ulp(2^100) == 2^48; half ulp == 2^47.
    let base = p(100);
    let half = p(47);
    assert_eq!(
        (&base + &half).to_f64(),
        2.0_f64.powi(100),
        "exact tie rounds to even"
    );
    assert_eq!(
        (&(&base + &half) + &m(1)).to_f64(),
        2.0_f64.powi(100) + 2.0_f64.powi(48),
        "tie plus a sticky bit far below rounds up"
    );
    assert_eq!(
        (&base + &p(46)).to_f64(),
        2.0_f64.powi(100),
        "below the tie rounds down"
    );
    // The odd neighbour: 2^100 + 2^48 has an odd mantissa, so its tie rounds up to even.
    let odd = &base + &p(48);
    assert_eq!(
        (&odd + &half).to_f64(),
        2.0_f64.powi(100) + 2.0 * 2.0_f64.powi(48)
    );
    // Negatives mirror.
    assert_eq!((-(&base + &half)).to_f64(), -(2.0_f64.powi(100)));
    // Sticky bit only in the lowest limb of a ~300-bit value.
    let wide = &(&p(300) + &p(247)) + &m(1);
    assert_eq!(wide.to_f64(), 2.0_f64.powi(300) + 2.0_f64.powi(248));
    assert_eq!(Magnus::zero().to_f64().to_bits(), 0.0_f64.to_bits());
}

#[test]
fn to_f64_overflow_boundary_at_two_to_the_1024() {
    let p = |e: u64| Magnus::one().shl(e).unwrap();
    assert_eq!(p(1023).to_f64(), 2.0_f64.powi(1023));
    assert_eq!(p(1023).bit_len(), 1024);
    let max = &p(1024) - &p(971); // f64::MAX == 2^1024 - 2^971
    assert_eq!(max.to_f64(), f64::MAX);
    // Exactly half an ulp above f64::MAX is a tie that rounds to even, i.e. up to 2^1024.
    let tie = &p(1024) - &p(970);
    assert_eq!(tie.to_f64(), f64::INFINITY);
    assert_eq!(
        (&tie - &m(1)).to_f64(),
        f64::MAX,
        "one below the tie stays finite"
    );
    assert_eq!((&p(1024) - &m(1)).to_f64(), f64::INFINITY);
    assert_eq!(p(1024).to_f64(), f64::INFINITY);
    assert_eq!((-p(1024)).to_f64(), f64::NEG_INFINITY);
    assert_eq!((-tie).to_f64(), f64::NEG_INFINITY);
    assert_eq!(p(5000).to_f64(), f64::INFINITY);
    assert_eq!(Magnus::from_f64(f64::MAX).unwrap().to_f64(), f64::MAX);
    assert_eq!(Magnus::from_f64(f64::MIN).unwrap().to_f64(), f64::MIN);
}

#[test]
fn from_f64_truncates_toward_zero_and_rejects_non_finite() {
    let f = |x: f64| Magnus::from_f64(x).unwrap();
    assert_eq!(f(2.9), m(2));
    assert_eq!(
        f(-2.9),
        m(-2),
        "negative truncation is toward zero, not floor"
    );
    assert_eq!(f(-0.5), Magnus::zero());
    assert!(!f(-0.5).is_negative());
    assert_eq!(f(-0.0), Magnus::zero());
    assert_eq!(f(0.999_999_999_999_999_9), Magnus::zero());
    assert_eq!(f(-1.0), m(-1));
    assert_eq!(
        f(f64::MIN_POSITIVE / 2.0),
        Magnus::zero(),
        "subnormals truncate to 0"
    );
    assert_eq!(f(9_007_199_254_740_993.0), m(9_007_199_254_740_992));
    assert_eq!(f(1e30).to_string(), "1000000000000000019884624838656");
    assert_eq!(f(-1e30).to_string(), "-1000000000000000019884624838656");
    assert_eq!(
        f(18_446_744_073_709_551_616.0),
        Magnus::one().shl(64).unwrap()
    );
    assert_eq!(
        f(f64::MAX),
        &Magnus::one().shl(1024).unwrap() - &Magnus::one().shl(971).unwrap()
    );
    for bad in [f64::NAN, f64::INFINITY, f64::NEG_INFINITY] {
        assert_eq!(Magnus::from_f64(bad), Err(MagnusError::NotFinite));
    }
    let mut g = Gen(11);
    for _ in 0..ITER {
        let raw = g.u128();
        #[allow(clippy::cast_precision_loss)]
        let x = (raw >> 8) as f64 * if g.next() & 1 == 1 { -1.0 } else { 1.0 } / 4.0;
        #[allow(clippy::cast_possible_truncation)]
        let want = x as i128; // saturating, truncating toward zero
        assert_eq!(f(x).to_i128(), Some(want), "{x}");
    }
}

#[test]
fn bytes_are_minimal_twos_complement() {
    let cases: [(i128, &[u8]); 13] = [
        (0, &[0x00]),
        (1, &[0x01]),
        (127, &[0x7F]),
        (128, &[0x00, 0x80]),
        (255, &[0x00, 0xFF]),
        (256, &[0x01, 0x00]),
        (-1, &[0xFF]),
        (-127, &[0x81]),
        (-128, &[0x80]),
        (-129, &[0xFF, 0x7F]),
        (-256, &[0xFF, 0x00]),
        (32767, &[0x7F, 0xFF]),
        (-32768, &[0x80, 0x00]),
    ];
    for (x, be) in cases {
        assert_eq!(m(x).to_bytes_be(), be, "be {x}");
        let mut le = be.to_vec();
        le.reverse();
        assert_eq!(m(x).to_bytes_le(), le, "le {x}");
        assert_eq!(Magnus::from_bytes_be(be), m(x), "decode be {x}");
        assert_eq!(Magnus::from_bytes_le(&le), m(x), "decode le {x}");
    }
    // Decode accepts any length: redundant sign bytes do not change the value.
    assert_eq!(Magnus::from_bytes_be(&[]), Magnus::zero());
    assert_eq!(Magnus::from_bytes_be(&[0, 0, 0, 5]), m(5));
    assert_eq!(Magnus::from_bytes_be(&[0xFF, 0xFF, 0xFF]), m(-1));
    assert_eq!(
        Magnus::from_bytes_le(&[0xFF, 0xFF, 0xFF, 0xFF, 0xFF]),
        m(-1)
    );
    assert_eq!(Magnus::from_bytes_be(&[0xFF, 0x80]), m(-128));
    let mut g = Gen(12);
    for _ in 0..ITER {
        let big = &(&m(g.signed(B120)) * &m(g.signed(B120))) * &m(g.signed(B120));
        assert_eq!(Magnus::from_bytes_be(&big.to_bytes_be()), big);
        assert_eq!(Magnus::from_bytes_le(&big.to_bytes_le()), big);
        // Minimality: dropping the top byte changes the value.
        let bytes = big.to_bytes_be();
        if bytes.len() > 1 {
            assert_ne!(Magnus::from_bytes_be(&bytes[1..]), big);
        }
    }
}

// ---- text ---------------------------------------------------------------------------------

#[test]
fn decimal_parse_and_render_match_i128() {
    let mut g = Gen(13);
    for _ in 0..ITER {
        let a = g.signed(B120);
        assert_eq!(m(a).to_string(), a.to_string());
        assert_eq!(dec(&a.to_string()).to_i128(), Some(a));
        assert_eq!(a.to_string().parse::<Magnus>().unwrap().to_i128(), Some(a));
    }
    assert_eq!(dec("+5"), m(5));
    assert_eq!(dec("-0"), Magnus::zero());
    assert_eq!(dec("0000"), Magnus::zero());
    assert_eq!(dec("00012"), m(12));
    assert_eq!(format!("{:>6}", m(-42)), "   -42");
    assert_eq!(format!("{:06}", m(42)), "000042");
    assert_eq!(format!("{:?}", m(-42)), "Magnus(-42)");
}

#[test]
fn radix_render_matches_i128_and_round_trips() {
    let mut g = Gen(14);
    for _ in 0..ITER {
        let a = g.signed(B120);
        let mag = a.unsigned_abs();
        let sign = if a < 0 { "-" } else { "" };
        assert_eq!(m(a).to_string_radix(Radix::Hex), format!("{sign}{mag:x}"));
        assert_eq!(m(a).to_string_radix(Radix::Bin), format!("{sign}{mag:b}"));
        assert_eq!(m(a).to_string_radix(Radix::Oct), format!("{sign}{mag:o}"));
        assert_eq!(m(a).to_string_radix(Radix::Dec), a.to_string());
    }
    // Wide round trips in every radix, beyond i128.
    for _ in 0..300 {
        let wide = &(&m(g.signed(B120)) * &m(g.signed(B120))) * &m(g.signed(B120));
        for radix in [Radix::Bin, Radix::Oct, Radix::Dec, Radix::Hex] {
            let text = wide.to_string_radix(radix);
            assert_eq!(
                Magnus::parse_radix(&text, radix),
                Ok(wide.clone()),
                "{radix:?} {text}"
            );
        }
    }
    assert_eq!(m(255).to_string_radix(Radix::Hex), "ff");
    assert_eq!(m(255).to_string_radix(Radix::Bin), "11111111");
    assert_eq!(m(255).to_string_radix(Radix::Oct), "377");
    assert_eq!(Magnus::zero().to_string_radix(Radix::Hex), "0");
    assert_eq!(Magnus::zero().to_string_radix(Radix::Bin), "0");
    assert_eq!(Magnus::parse_radix("FF", Radix::Hex), Ok(m(255)));
    assert_eq!(Magnus::parse_radix("-fF", Radix::Hex), Ok(m(-255)));
    assert_eq!(Magnus::parse_radix("+101", Radix::Bin), Ok(m(5)));
    assert_eq!(Magnus::parse_radix("0777", Radix::Oct), Ok(m(511)));
    // 2^200 in hex is a 1 followed by 50 zeros.
    assert_eq!(
        Magnus::one().shl(200).unwrap().to_string_radix(Radix::Hex),
        format!("1{}", "0".repeat(50))
    );
}

#[test]
fn malformed_text_is_rejected() {
    for bad in [
        "", "+", "-", " 1", "1 ", "1_0", "0x10", "12a", "--1", "+-1", "１２", "1.0", "\u{0}",
    ] {
        assert_eq!(
            Magnus::parse_decimal(bad),
            Err(MagnusError::Malformed),
            "{bad:?}"
        );
    }
    assert_eq!(
        Magnus::parse_radix("2", Radix::Bin),
        Err(MagnusError::Malformed)
    );
    assert_eq!(
        Magnus::parse_radix("8", Radix::Oct),
        Err(MagnusError::Malformed)
    );
    assert_eq!(
        Magnus::parse_radix("g", Radix::Hex),
        Err(MagnusError::Malformed)
    );
    assert_eq!(
        Magnus::parse_radix("0x1", Radix::Hex),
        Err(MagnusError::Malformed)
    );
    assert_eq!(
        Magnus::parse_radix("", Radix::Hex),
        Err(MagnusError::Malformed)
    );
}

#[test]
fn error_display_is_stable() {
    assert_eq!(MagnusError::DivisionByZero.to_string(), "division by zero");
    assert_eq!(
        MagnusError::NegativeExponent.to_string(),
        "negative exponent"
    );
    let boxed: Box<dyn std::error::Error> = Box::new(MagnusError::NotFinite);
    assert!(boxed.to_string().contains("NaN"));
}

// ---- bitwise on wide and negative values --------------------------------------------------

#[test]
fn bitwise_identities_on_wide_values() {
    let mut g = Gen(15);
    for _ in 0..500 {
        let x = &(&m(g.signed(B120)) * &m(g.signed(B120))) * &m(g.signed(B120));
        let y = &(&m(g.signed(B120)) * &m(g.signed(B120))) * &m(g.signed(B120));
        let all_ones = m(-1);
        assert_eq!(x.bit_and(&all_ones), x);
        assert_eq!(x.bit_or(&Magnus::zero()), x);
        assert_eq!(x.bit_xor(&x), Magnus::zero());
        assert_eq!(x.bit_xor(&all_ones), x.bit_not());
        assert_eq!(x.bit_not().bit_not(), x);
        // De Morgan and the sum identity x + y == (x ^ y) + 2 (x & y).
        assert_eq!(x.bit_and(&y).bit_not(), x.bit_not().bit_or(&y.bit_not()));
        let two_and = x.bit_and(&y).shl(1).unwrap();
        assert_eq!(&x + &y, &x.bit_xor(&y) + &two_and);
    }
    let p200 = Magnus::one().shl(200).unwrap();
    assert_eq!((-p200.clone()).bit_and(&m(-1)), -p200.clone());
    assert_eq!((-p200.clone()).bit_or(&m(1)), &(-p200.clone()) + &m(1));
    assert_eq!(p200.bit_not(), &(-p200) - &m(1));
}
