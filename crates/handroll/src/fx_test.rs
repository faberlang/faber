//! Known-answer and behavior tests for the FxHash port (`fx`).
//!
//! The concrete digest values are copied from the `rustc-hash` 2.1.3 test
//! suite (64-bit branch) so a regression here means the port has drifted
//! from the replaced crate.

use super::{FxBuildHasher, FxHashMap, FxHashSet, FxHasher};
use std::hash::{BuildHasher, Hash, Hasher};

fn hash_one<T: Hash>(value: T) -> u64 {
    FxBuildHasher.hash_one(value)
}

#[test]
fn known_answer_unsigned_values_match_rustc_hash() {
    assert_eq!(hash_one(0_u8), 0);
    assert_eq!(hash_one(1_u8), 12_157_901_119_326_311_915);
    assert_eq!(hash_one(100_u8), 16_751_747_135_202_103_309);
    assert_eq!(hash_one(u8::MAX), 1_211_781_028_898_739_645);

    assert_eq!(hash_one(0_u16), 0);
    assert_eq!(hash_one(u16::MAX), 16_279_819_243_059_860_173);

    assert_eq!(hash_one(0_u32), 0);
    assert_eq!(hash_one(u32::MAX), 7_729_994_835_221_066_939);

    assert_eq!(hash_one(0_u64), 0);
    assert_eq!(hash_one(1_u64), 12_157_901_119_326_311_915);
    assert_eq!(hash_one(100_u64), 16_751_747_135_202_103_309);
    assert_eq!(hash_one(u64::MAX), 6_288_842_954_450_348_564);

    assert_eq!(hash_one(0_u128), 0);
    assert_eq!(hash_one(1_u128), 13_032_756_267_696_824_044);
    assert_eq!(hash_one(u128::MAX), 11_702_830_760_530_184_999);

    assert_eq!(hash_one(0_usize), 0);
    assert_eq!(hash_one(usize::MAX), 6_288_842_954_450_348_564);
}

#[test]
fn known_answer_signed_values_match_rustc_hash() {
    assert_eq!(hash_one(i8::MIN), 6_684_841_074_112_525_780);
    assert_eq!(hash_one(i8::MAX), 12_973_684_028_562_874_344);
    assert_eq!(hash_one(i32::MIN), 9_943_947_977_240_134_995);
    assert_eq!(hash_one(i32::MAX), 16_232_790_931_690_483_559);
    assert_eq!(hash_one(i64::MIN), 33_554_432);
    assert_eq!(hash_one(i64::MAX), 6_288_842_954_483_902_996);
    assert_eq!(hash_one(isize::MIN), 33_554_432);
}

#[test]
fn byte_strings_hash_deterministically() {
    let first = {
        let mut hasher = FxHasher::default();
        b"faber locale pack".hash(&mut hasher);
        hasher.finish()
    };
    let second = {
        let mut hasher = FxHasher::default();
        b"faber locale pack".hash(&mut hasher);
        hasher.finish()
    };
    assert_eq!(first, second);
}

#[test]
fn with_seed_is_actually_different() {
    for seed_a in [0_usize, 1, 42, usize::MAX] {
        for seed_b in [0_usize, 1, 42, usize::MAX] {
            if seed_a == seed_b {
                continue;
            }
            let mut a = FxHasher::with_seed(seed_a);
            let mut b = FxHasher::with_seed(seed_b);
            "seeded input".hash(&mut a);
            "seeded input".hash(&mut b);
            assert_ne!(a.finish(), b.finish());
        }
    }
}

#[test]
fn map_and_set_behave_like_the_std_collections() {
    let mut map: FxHashMap<String, u32> = FxHashMap::default();
    map.insert("en".to_owned(), 1);
    map.insert("la".to_owned(), 2);
    assert_eq!(map.get("en"), Some(&1));
    assert_eq!(map.len(), 2);

    let sized: FxHashMap<String, u32> = FxHashMap::with_capacity_and_hasher(16, FxBuildHasher);
    assert!(sized.is_empty());

    let mut set: FxHashSet<u64> = FxHashSet::default();
    set.insert(7);
    assert!(set.contains(&7));
}
