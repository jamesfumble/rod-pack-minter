use anchor_lang::prelude::*;
use solana_sha256_hasher::hashv;

use crate::{
    constants::*,
    error::ErrorCode,
    state::{PackSealed, PendingPack},
};

pub enum SlotLookup {
    Found([u8; 32]),
    /// The slot is not complete yet (the pack was bought in the current slot).
    TooEarly,
    /// The slot is older than every SlotHashes entry (about 512 slots, ~3.4 minutes).
    Expired,
}

/// Finds the hash of `slot` in the SlotHashes sysvar (u64 count, then (slot u64, hash [u8; 32]) newest first).
pub fn lookup_slot_hash(slot_hashes: &AccountInfo, slot: u64) -> Result<SlotLookup> {
    require_keys_eq!(slot_hashes.key(), SLOT_HASHES_ID, ErrorCode::BadSlotHashes);
    let data = slot_hashes.try_borrow_data()?;
    require!(data.len() >= 8, ErrorCode::BadSlotHashes);
    let count = u64::from_le_bytes(data[0..8].try_into().unwrap()) as usize;
    require!(count > 0 && data.len() >= 8 + count * 40, ErrorCode::BadSlotHashes);
    let entry_slot = |i: usize| u64::from_le_bytes(data[8 + i * 40..16 + i * 40].try_into().unwrap());
    if slot > entry_slot(0) {
        return Ok(SlotLookup::TooEarly);
    }
    if slot < entry_slot(count - 1) {
        return Ok(SlotLookup::Expired);
    }
    // Newest first, strictly decreasing: binary search.
    let (mut lo, mut hi) = (0usize, count - 1);
    while lo <= hi {
        let mid = (lo + hi) / 2;
        let s = entry_slot(mid);
        if s == slot {
            return Ok(SlotLookup::Found(data[16 + mid * 40..48 + mid * 40].try_into().unwrap()));
        }
        if s > slot {
            lo = mid + 1;
        } else {
            if mid == 0 {
                break;
            }
            hi = mid - 1;
        }
    }
    // A purchase slot always has a block, so this only happens with a malformed sysvar.
    err!(ErrorCode::BadSlotHashes)
}

/// Used only when nobody sealed the pack within ~512 slots of its purchase. It is fixed at purchase time, so it
/// gives at most one alternative outcome, and only during a total outage of every opener (see docs/security.md).
pub fn fallback_hash(pending_pack: &Pubkey, purchase_slot: u64) -> [u8; 32] {
    hashv(&[b"rod-pack-minter:fallback", pending_pack.as_ref(), &purchase_slot.to_le_bytes()]).to_bytes()
}

/// Records the hash the draw will use. Returns true if the fallback was used.
pub fn seal(pending: &mut PendingPack, pending_key: &Pubkey, slot_hashes: &AccountInfo) -> Result<bool> {
    require!(!pending.sealed, ErrorCode::AlreadySealed);
    let (hash, fallback) = match lookup_slot_hash(slot_hashes, pending.purchase_slot)? {
        SlotLookup::Found(h) => (h, false),
        SlotLookup::TooEarly => return err!(ErrorCode::TooEarlyToOpen),
        SlotLookup::Expired => (fallback_hash(pending_key, pending.purchase_slot), true),
    };
    pending.sealed = true;
    pending.slot_hash = hash;
    Ok(fallback)
}

/// Moves lamports out of a program-owned account (the PendingPack) to `to`.
pub fn pay_from_pending(pending: &AccountInfo, to: &AccountInfo, lamports: u64) -> Result<()> {
    if lamports == 0 {
        return Ok(());
    }
    **pending.try_borrow_mut_lamports()? = pending.lamports().checked_sub(lamports).ok_or(ErrorCode::Overflow)?;
    **to.try_borrow_mut_lamports()? = to.lamports().checked_add(lamports).ok_or(ErrorCode::Overflow)?;
    Ok(())
}

/// Anyone may seal a pack, e.g. when opening has to wait (a full tree). It only stores the purchase slot's hash,
/// so the cards are fixed and SlotHashes expiry no longer matters. The sealer receives the player's seal deposit.
#[event_cpi]
#[derive(Accounts)]
pub struct SealPack<'info> {
    #[account(mut)]
    pub sealer: Signer<'info>,
    #[account(
        mut,
        seeds = [PACK_SEED, pending_pack.player.as_ref(), pending_pack.order_id.as_ref()],
        bump = pending_pack.bump
    )]
    pub pending_pack: Box<Account<'info, PendingPack>>,
    /// CHECK: address checked in `lookup_slot_hash`.
    pub slot_hashes: UncheckedAccount<'info>,
}

pub fn handle_seal_pack(ctx: Context<SealPack>) -> Result<()> {
    let key = ctx.accounts.pending_pack.key();
    let fallback = seal(&mut ctx.accounts.pending_pack, &key, &ctx.accounts.slot_hashes.to_account_info())?;
    let fee = ctx.accounts.pending_pack.seal_fee;
    ctx.accounts.pending_pack.seal_fee = 0;
    pay_from_pending(&ctx.accounts.pending_pack.to_account_info(), &ctx.accounts.sealer.to_account_info(), fee)?;
    emit_cpi!(PackSealed { pending_pack: key, fallback, slot_hash: ctx.accounts.pending_pack.slot_hash });
    Ok(())
}
