//! Expected values come from the 26.3 server jar (`SimpleBitStorage`, `PalettedContainer` over an
//! identity `IdMap` of 1000 or 70 ids, `FriendlyByteBuf`), with `pack` checked against Paper's
//! Moonrise `pack` (`PaletteReadoutCache`) run as Java over the same containers. Hashes are
//! `h = h * 31 + x` in Java `long` arithmetic.

use std::io::Cursor;

use super::bit_storage::{BitStorage, word_count};
use super::paletted::{ContainerError, PackedData, PalettedContainer, Strategy};

fn hash(values: impl IntoIterator<Item = i64>) -> i64 {
    values
        .into_iter()
        .fold(0i64, |h, v| h.wrapping_mul(31).wrapping_add(v))
}

/// Java's 64-bit LCG of the generator program, `(int) (seed >>> 33)`.
struct Lcg(i64);
impl Lcg {
    fn next(&mut self) -> usize {
        self.0 = self
            .0
            .wrapping_mul(6364136223846793005)
            .wrapping_add(1442695040888963407);
        ((self.0 as u64) >> 33) as usize
    }
}

#[test]
fn packs_like_simple_bit_storage() {
    // (bits, size, longs, hash of the longs)
    const JAVA: &[(u8, usize, usize, i64)] = &[
        (1, 64, 1, 6148914691236517205),
        (1, 4096, 64, -26651026664205312),
        (2, 64, 2, 2821266740684990240),
        (2, 4096, 128, -4607995689804052480),
        (3, 64, 4, 8371345032543593290),
        (3, 4096, 196, 6098904874550286826),
        (4, 64, 4, 5820981388788769344),
        (4, 4096, 256, -5671416543608795136),
        (5, 64, 6, -5376139038632325660),
        (5, 4096, 342, 232004878525778308),
        (6, 64, 7, -6782250716266948457),
        (6, 4096, 410, 7645957484378123970),
        (7, 64, 8, 4308003434642922212),
        (7, 4096, 456, 5449300716050892100),
        (8, 64, 8, 3002791623851085472),
        (8, 4096, 512, 3821713482412791808),
        (9, 64, 10, 183654899698491091),
        (9, 4096, 586, -1587662486870368077),
        (10, 64, 11, 6700959656483626775),
        (10, 4096, 683, -6209352848993073417),
        (13, 64, 16, 1614354778165751584),
        (13, 4096, 1024, 2966418016034768896),
        (15, 64, 16, 9116163225012031264),
        (15, 4096, 1024, -7740920843421267968),
        (21, 64, 22, -3364114386486699471),
        (21, 4096, 1366, -7459742193384349679),
        (31, 64, 32, 8281521846783682592),
        (31, 4096, 2048, -3511147235206625280),
        (32, 64, 32, -6791281667933886432),
        (32, 4096, 2048, 5795897636521904128),
    ];
    for &(bits, size, longs, expected) in JAVA {
        let mask = (1u64 << bits) - 1;
        let values: Vec<u32> = (0..size as u64)
            .map(|i| ((i * 2654435761 + 12345) & mask & 0x7fff_ffff) as u32)
            .collect();
        let packed = BitStorage::from_values(bits, &values);
        assert_eq!(packed.words().len(), longs, "{bits} bits");
        assert_eq!(
            hash(packed.words().iter().map(|&w| w as i64)),
            expected,
            "{bits} bits x {size}"
        );
        // Element access agrees with the bulk packing, and set rebuilds the same words.
        let mut rebuilt = BitStorage::new(bits, size);
        for (i, &v) in values.iter().enumerate() {
            assert_eq!(packed.get(i), v);
            assert_eq!(rebuilt.get_and_set(i, v), 0);
        }
        assert_eq!(rebuilt, packed);
        let mut all = Vec::new();
        packed.for_each(|v| all.push(v));
        assert_eq!(all, values);
    }
}

#[test]
fn index_magic_is_exact_division() {
    for bits in 1..=32u8 {
        let per = 64 / usize::from(bits);
        let mut storage = BitStorage::new(bits, 4096);
        for i in 0..4096 {
            storage.set(i, 1);
            let word = storage.words()[i / per];
            assert_eq!(
                word,
                1u64 << ((i % per) * usize::from(bits)),
                "{bits} bits, index {i}"
            );
            storage.set(i, 0);
        }
    }
}

#[test]
fn fill_writes_padding_and_wrong_length_is_refused() {
    let mut storage = BitStorage::new(5, 4096);
    storage.fill(0b10101);
    // 12 slots and the low 4 bits of a 13th copy in the padding.
    let pattern = (0..13).fold(0u64, |w, k| w | 0b10101u64.wrapping_shl(5 * k));
    assert!(storage.words().iter().all(|&w| w == pattern));
    assert_eq!(word_count(5, 4096), 342);
    let err = BitStorage::from_words(9, 4096, vec![0; 5]).unwrap_err();
    assert_eq!((err.got, err.expected), (5, 586));
}

/// Steps where the bit count changed: (step, bits, palette size).
type Growth = &'static [(usize, u8, usize)];
/// (step, hash of all values, wire length, wire hash, first wire bytes,
/// packed palette length, packed palette hash, packed data hash).
type Checkpoint = (usize, i64, usize, i64, [u8; 8], usize, i64, Option<i64>);

fn scenario(
    strategy: Strategy,
    seed: i64,
    limits: [usize; 4],
    growth: Growth,
    checkpoints: &[Checkpoint],
) {
    let n = strategy.entry_count();
    let mut rng = Lcg(seed);
    let mut c = PalettedContainer::new(strategy, 0u32);
    let (mut last_bits, mut seen_growth, mut seen_cp) = (None, 0, 0);
    for step in 0..3000 {
        let index = rng.next() % n;
        let limit = match step {
            0..100 => limits[0],
            100..500 => limits[1],
            500..1000 => limits[2],
            _ => limits[3],
        };
        let value = (rng.next() % limit) as u32;
        c.get_and_set(index, value);
        if last_bits != Some(c.bits()) {
            assert_eq!(
                growth[seen_growth],
                (step, c.bits(), c.palette_size()),
                "growth"
            );
            seen_growth += 1;
            last_bits = Some(c.bits());
        }
        let Some(cp) = checkpoints.get(seen_cp).filter(|cp| cp.0 == step) else {
            continue;
        };
        seen_cp += 1;
        assert_eq!(
            hash((0..n).map(|i| i64::from(c.get(i)))),
            cp.1,
            "values at {step}"
        );
        let mut wire = Vec::new();
        c.write(&mut wire);
        assert_eq!(
            (wire.len(), c.serialized_size()),
            (cp.2, cp.2),
            "wire size at {step}"
        );
        assert_eq!(
            hash(wire.iter().map(|&b| i64::from(b))),
            cp.3,
            "wire at {step}"
        );
        assert_eq!(wire[..8], cp.4, "wire head at {step}");
        let packed = c.pack();
        assert_eq!(packed.palette.len(), cp.5, "packed palette at {step}");
        assert_eq!(
            hash(packed.palette.iter().map(|&v| i64::from(v))),
            cp.6,
            "packed palette at {step}"
        );
        assert_eq!(
            packed.data.as_ref().map(|d| hash(d.iter().copied())),
            cp.7,
            "packed data at {step}"
        );
        // The saved form and the wire form load back to the same values.
        let loaded = PalettedContainer::unpack(strategy, packed).unwrap();
        assert!((0..n).all(|i| loaded.get(i) == c.get(i)));
        let mut read = PalettedContainer::new(strategy, 7u32);
        let mut cursor = Cursor::new(&wire[..]);
        read.read(&mut cursor).unwrap();
        assert_eq!((read, cursor.position() as usize), (c.clone(), wire.len()));
    }
    assert_eq!((seen_growth, seen_cp), (growth.len(), checkpoints.len()));
}

#[test]
#[rustfmt::skip]
fn old_values_follow_java() {
    // The hash of the values getAndSet returned over the whole run.
    for (strategy, seed, limits, expected) in [
        (Strategy::block_states(1000), 0x1234, [3, 20, 200, 400], 7171649446174813368i64),
        (Strategy::biomes(70), 0x99, [2, 5, 9, 70], 6649947525356474766),
    ] {
        let mut rng = Lcg(seed);
        let mut c = PalettedContainer::new(strategy, 0u32);
        let mut olds = 0i64;
        for step in 0..3000 {
            let index = rng.next() % strategy.entry_count();
            let limit = limits[[100, 500, 1000].iter().filter(|&&s| step >= s).count()];
            let old = c.get_and_set(index, (rng.next() % limit) as u32);
            olds = olds.wrapping_mul(31).wrapping_add(i64::from(old));
        }
        assert_eq!(olds, expected);
    }
}

#[test]
#[rustfmt::skip]
fn block_states_grow_and_serialize_like_leaf() {
    scenario(
        Strategy::block_states(1000),
        0x1234,
        [3, 20, 200, 400],
        &[(0, 0, 1), (2, 4, 2), (150, 5, 17), (517, 6, 33), (563, 7, 65), (681, 8, 128), (1173, 10, 1000)],
        &[
            (50, -3040698483747692736, 2053, -5729293116524900927, [4, 3, 0, 2, 1, 0, 0, 0], 3, 63, Some(-3824569553190898876)),
            (400, -419788647386037453, 2758, -7102255631195517659, [5, 20, 0, 19, 2, 12, 13, 1], 20, -6581500826582485882, Some(-6722263813820891726)),
            (900, 3164713651577496709, 4328, -8149308090742143779, [8, 170, 1, 0, 17, 19, 6, 194], 170, -3665787192621724194, Some(3288917713211902486)),
            (2999, 5726257355380619283, 5465, 6841236719879639656, [10, 0, 252, 40, 0, 0, 0, 1], 395, 652920367023576635, Some(1058796877046940775)),
        ],
    );
}

#[test]
#[rustfmt::skip]
fn biomes_grow_and_serialize_like_leaf() {
    scenario(
        Strategy::biomes(70),
        0x99,
        [2, 5, 9, 70],
        &[(0, 0, 1), (3, 1, 2), (100, 2, 3), (105, 3, 5), (513, 7, 70)],
        &[
            (50, 5352017443382901697, 12, 27049185184536723, [1, 2, 0, 1, 18, 112, 146, 16], 2, 1, Some(1328722488063008774)),
            (400, -540597624222630142, 39, 2003943354311204726, [3, 5, 0, 1, 4, 2, 3, 48], 5, 33700, Some(-8376900599124430380)),
            (900, 2664418021839555680, 65, 3586009056886132128, [7, 7, 8, 32, 0, 0, 160, 193], 9, 5175177352056, Some(112104245991863622)),
            (2999, 7749971317132307892, 65, 8461850224076580298, [7, 33, 123, 13, 49, 198, 15, 137], 40, 1863861800224670213, Some(-544217433169373686)),
        ],
    );
}

#[test]
fn unpacks_like_vanilla() {
    let strategy = Strategy::block_states(1000);
    let palette: Vec<u32> = (0..300).map(|i| (i * 7 + 3) % 1000).collect();
    let ids: Vec<u32> = (0..4096).map(|i| (i * 13) % 300).collect();
    let data: Vec<i64> = BitStorage::from_values(9, &ids)
        .words()
        .iter()
        .map(|&w| w as i64)
        .collect();
    let c = PalettedContainer::unpack(
        strategy,
        PackedData {
            palette: palette.clone(),
            data: Some(data),
        },
    )
    .unwrap();
    assert!(c.is_global());
    assert_eq!(c.bits(), 10);
    assert_eq!(
        hash((0..4096).map(|i| i64::from(c.get(i)))),
        -1434621226098730096
    );
    let mut wire = Vec::new();
    c.write(&mut wire);
    assert_eq!(
        (wire.len(), hash(wire.iter().map(|&b| i64::from(b)))),
        (5465, -7276814021828441477)
    );

    let err = PalettedContainer::unpack(
        strategy,
        PackedData {
            palette: palette.clone(),
            data: Some(vec![0; 5]),
        },
    );
    assert_eq!(
        err.unwrap_err().to_string(),
        "Failed to read PalettedContainer: Invalid length given for storage, got: 5 but expected: 586"
    );
    let err = PalettedContainer::unpack(
        strategy,
        PackedData {
            palette: palette[..2].to_vec(),
            data: None,
        },
    );
    assert_eq!(
        err.unwrap_err().to_string(),
        "Missing values for non-zero storage"
    );
    // A single entry ignores whatever data there is; an index past the palette fails the load.
    let single = PalettedContainer::unpack(
        strategy,
        PackedData {
            palette: vec![5u32],
            data: Some(vec![1, 2]),
        },
    )
    .unwrap();
    assert_eq!((single.bits(), single.get(4095)), (0, 5));
    let bad = PalettedContainer::unpack(
        strategy,
        PackedData {
            palette: vec![1u32, 2, 3],
            data: Some(vec![7; 256]),
        },
    );
    assert!(matches!(bad, Err(ContainerError::MissingEntry(7))));
    assert!(matches!(
        PalettedContainer::<u32>::unpack(
            strategy,
            PackedData {
                palette: vec![],
                data: None
            }
        ),
        Err(ContainerError::EmptyPalette)
    ));
}

#[test]
fn pack_keeps_a_fully_used_palette_and_compacts_otherwise() {
    let strategy = Strategy::block_states(1000);
    let mut c = PalettedContainer::new(strategy, 0u32);
    c.set(0, 9);
    c.set(1, 8);
    c.set(0, 0);
    // Palette [0, 9, 8], all used, 4 bits on both sides: kept as is (vanilla would write [0, 8, 9]).
    c.set(2, 9);
    let kept = c.pack();
    assert_eq!(kept.palette, vec![0, 9, 8]);
    // One entry unused: re-encoded in first-occurrence order.
    c.set(2, 0);
    let compact = c.pack();
    assert_eq!(compact.palette, vec![0, 8]);
    let ids: Vec<u32> = (0..4096).map(|i| u32::from(i == 1)).collect();
    let expected: Vec<i64> = BitStorage::from_values(4, &ids)
        .words()
        .iter()
        .map(|&w| w as i64)
        .collect();
    assert_eq!(compact.data, Some(expected));
    // Back to one value: no data.
    c.set(1, 0);
    assert_eq!(
        c.pack(),
        PackedData {
            palette: vec![0],
            data: None
        }
    );
}

#[test]
fn counts_and_positions_in_moonrise_order() {
    let mut c = PalettedContainer::new(Strategy::block_states(1000), 1u32);
    for i in [10, 3, 4000] {
        c.set(i, 5);
    }
    c.set(7, 6);
    let mut counts = Vec::new();
    c.count(|v, n| counts.push((v, n)));
    assert_eq!(counts, vec![(1, 4092), (5, 3), (6, 1)]);
    let mut positions = vec![99];
    c.positions_where(|v| v == 6 || v == 5, &mut positions);
    assert_eq!(positions, vec![99, 3, 10, 4000, 7]);
    let single = PalettedContainer::new(Strategy::biomes(10), 2u32);
    let mut all = Vec::new();
    single.positions_where(|v| v == 2, &mut all);
    assert_eq!(all, (0..64).collect::<Vec<u16>>());
    assert!(single.maybe_has(|v| v == 2) && !single.maybe_has(|v| v == 3));
}

#[test]
fn wire_read_rejects_what_leaf_rejects() {
    let strategy = Strategy::biomes(70);
    let mut c = PalettedContainer::new(strategy, 0u32);
    // Single value outside the registry.
    assert!(matches!(
        c.read(&mut Cursor::new(&[0, 70][..])),
        Err(ContainerError::UnknownId(70))
    ));
    // A 1-bit list of 3 entries overflows FastHashMapPalette's array.
    assert!(matches!(
        c.read(&mut Cursor::new(&[1, 3, 0, 1, 2][..])),
        Err(ContainerError::PaletteTooLarge { .. })
    ));
    // Truncated words.
    assert!(c.read(&mut Cursor::new(&[1, 2, 0, 1, 0, 0][..])).is_err());
    assert_eq!(c, PalettedContainer::new(strategy, 0));
    // A negative bits byte selects the global palette, read with the global bits.
    let mut wire = vec![0xff];
    wire.extend(std::iter::repeat_n(0u8, 8 * word_count(7, 64)));
    c.read(&mut Cursor::new(&wire[..])).unwrap();
    assert!(c.is_global() && c.bits() == 7);
}
