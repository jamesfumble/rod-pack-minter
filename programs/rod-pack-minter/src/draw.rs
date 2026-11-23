//! The pack draw. Pure functions, so they are unit-tested on the host and give the same result on chain.
//!
//! Seed = sha256(hash of the purchase slot | order_id | player | purchase slot | expansion_id). The purchase slot's
//! hash only exists after that slot is complete, when the purchase (and its payment) can no longer be undone. It does
//! not depend on who opens the pack, when, or on the tree, so nobody can steer it afterwards. Anyone can recompute a
//! pack from the emitted seed.

use solana_sha256_hasher::hashv;

use crate::constants::RARITY_COUNT;

pub fn pack_seed(slot_hash: &[u8; 32], order_id: &[u8; 16], player: &[u8; 32], purchase_slot: u64, expansion_id: u16) -> [u8; 32] {
    hashv(&[slot_hash, order_id, player, &purchase_slot.to_le_bytes(), &expansion_id.to_le_bytes()]).to_bytes()
}

/// Weights with rarities that have no cards set to 0.
pub fn effective_weights(weights: &[u16; RARITY_COUNT], counts: &[u16; RARITY_COUNT]) -> [u64; RARITY_COUNT] {
    let mut w = [0u64; RARITY_COUNT];
    for r in 0..RARITY_COUNT {
        if counts[r] > 0 {
            w[r] = weights[r] as u64;
        }
    }
    w
}

fn pick_rarity(roll: u64, weights: &[u64; RARITY_COUNT]) -> Option<u8> {
    let total: u64 = weights.iter().sum();
    if total == 0 {
        return None;
    }
    let mut x = roll % total;
    for (r, w) in weights.iter().enumerate() {
        if x < *w {
            return Some(r as u8);
        }
        x -= w;
    }
    None
}

/// Draws `pack_size` cards as (rarity, index within that rarity). Slots 0..pack_size-1 use `regular`, the last slot
/// uses `guaranteed` (when it has any weight; otherwise `regular`). Returns None if a slot has no possible rarity.
pub fn draw_pack(
    seed: &[u8; 32],
    pack_size: u8,
    regular: &[u16; RARITY_COUNT],
    guaranteed: &[u16; RARITY_COUNT],
    counts: &[u16; RARITY_COUNT],
) -> Option<Vec<(u8, u16)>> {
    let reg = effective_weights(regular, counts);
    let gua = effective_weights(guaranteed, counts);
    let has_guarantee = gua.iter().sum::<u64>() > 0;
    let mut out = Vec::with_capacity(pack_size as usize);
    for slot in 0..pack_size {
        let h = hashv(&[seed, &[slot]]).to_bytes();
        let roll_rarity = u64::from_le_bytes(h[0..8].try_into().unwrap());
        let roll_card = u64::from_le_bytes(h[8..16].try_into().unwrap());
        let weights = if slot + 1 == pack_size && has_guarantee { &gua } else { &reg };
        let rarity = pick_rarity(roll_rarity, weights)?;
        let index = (roll_card % counts[rarity as usize] as u64) as u16;
        out.push((rarity, index));
    }
    Some(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    const REG: [u16; 4] = [70, 25, 5, 0];
    const GUA: [u16; 4] = [0, 25, 5, 0];
    const COUNTS: [u16; 4] = [22, 22, 22, 0];

    #[test]
    fn odds_over_many_packs() {
        let n = 100_000u32;
        let mut regular = [0u32; 4];
        let mut last = [0u32; 4];
        let mut per_card = [[0u32; 22]; 3];
        for i in 0..n {
            let seed = hashv(&[&i.to_le_bytes()]).to_bytes();
            let pack = draw_pack(&seed, 6, &REG, &GUA, &COUNTS).unwrap();
            assert_eq!(pack.len(), 6);
            for (slot, (r, idx)) in pack.iter().enumerate() {
                assert!((*idx as usize) < 22);
                if slot == 5 {
                    last[*r as usize] += 1;
                } else {
                    regular[*r as usize] += 1;
                }
                per_card[*r as usize][*idx as usize] += 1;
            }
        }
        let total_regular = (n * 5) as f64;
        let pct = |c: u32, t: f64| c as f64 * 100.0 / t;
        // 70 / 25 / 5 within 0.5 percentage points over 500,000 slots
        assert!((pct(regular[0], total_regular) - 70.0).abs() < 0.5, "common {}", pct(regular[0], total_regular));
        assert!((pct(regular[1], total_regular) - 25.0).abs() < 0.5, "rare {}", pct(regular[1], total_regular));
        assert!((pct(regular[2], total_regular) - 5.0).abs() < 0.5, "epic {}", pct(regular[2], total_regular));
        // the guaranteed slot is never Common: 83.3% Rare / 16.7% Epic
        assert_eq!(last[0], 0);
        assert!((pct(last[1], n as f64) - 83.333).abs() < 0.6, "last rare {}", pct(last[1], n as f64));
        assert!((pct(last[2], n as f64) - 16.667).abs() < 0.6, "last epic {}", pct(last[2], n as f64));
        // every card of a rarity is roughly equally likely (within 10% of the mean)
        for r in 0..3 {
            let mean = per_card[r].iter().sum::<u32>() as f64 / 22.0;
            for c in per_card[r] {
                assert!((c as f64 - mean).abs() < mean * 0.1, "rarity {r} card count {c} mean {mean}");
            }
        }
    }

    #[test]
    fn same_seed_same_pack_and_different_seed_differs() {
        let a = draw_pack(&[7u8; 32], 6, &REG, &GUA, &COUNTS).unwrap();
        let b = draw_pack(&[7u8; 32], 6, &REG, &GUA, &COUNTS).unwrap();
        let c = draw_pack(&[8u8; 32], 6, &REG, &GUA, &COUNTS).unwrap();
        assert_eq!(a, b);
        assert_ne!(a, c);
    }

    #[test]
    fn rarity_without_cards_is_never_drawn() {
        let counts = [22, 0, 22, 0];
        for i in 0..2000u32 {
            let seed = hashv(&[&i.to_le_bytes()]).to_bytes();
            for (r, _) in draw_pack(&seed, 6, &REG, &GUA, &counts).unwrap() {
                assert_ne!(r, 1);
            }
        }
    }

    #[test]
    fn no_possible_rarity_fails() {
        assert!(draw_pack(&[1u8; 32], 6, &REG, &GUA, &[0, 0, 0, 0]).is_none());
        // guarantee impossible (no Rare or Epic cards): the last slot falls back to the regular weights
        let pack = draw_pack(&[1u8; 32], 6, &REG, &GUA, &[5, 0, 0, 0]).unwrap();
        assert!(pack.iter().all(|(r, _)| *r == 0));
    }
}
