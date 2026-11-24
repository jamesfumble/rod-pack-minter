use anchor_lang::prelude::*;

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

/// Records the hash the draw will use: the purchase slot's own hash. If nobody sealed or opened the pack within the
/// SlotHashes window (~512 slots, ~3.4 minutes), there is no fair hash left: `redraw_pack` then gives the pack a new
/// purchase slot. There is deliberately no fallback hash known in advance.
pub fn seal(pending: &mut PendingPack, slot_hashes: &AccountInfo) -> Result<()> {
    require!(!pending.sealed, ErrorCode::AlreadySealed);
    let hash = match lookup_slot_hash(slot_hashes, pending.purchase_slot)? {
        SlotLookup::Found(h) => h,
        SlotLookup::TooEarly => return err!(ErrorCode::TooEarlyToOpen),
        SlotLookup::Expired => return err!(ErrorCode::SlotHashUnavailable),
    };
    pending.sealed = true;
    pending.slot_hash = hash;
    Ok(())
}

/// Pays one retry (seal or redraw) from the player's retry pool, while it lasts. Returns the amount paid.
pub fn pay_retry(pending: &mut Account<PendingPack>, to: &AccountInfo) -> Result<u64> {
    let amount = pending.retry_fee.min(pending.retry_pool);
    pending.retry_pool -= amount;
    pay_from_pending(&pending.to_account_info(), to, amount)?;
    Ok(amount)
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

/// Anyone may seal a pack, e.g. when opening has to wait. It only stores the purchase slot's hash, so the cards are
/// fixed and SlotHashes expiry no longer matters. From SEAL_PAY_DELAY_SLOTS after purchase the sealer is paid one
/// retry fee from the player's retry pool.
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
    seal(&mut ctx.accounts.pending_pack, &ctx.accounts.slot_hashes.to_account_info())?;
    // Paid only when opening is really delayed; in normal operation open_pack seals for free.
    if Clock::get()?.slot > ctx.accounts.pending_pack.purchase_slot.saturating_add(SEAL_PAY_DELAY_SLOTS) {
        pay_retry(&mut ctx.accounts.pending_pack, &ctx.accounts.sealer.to_account_info())?;
    }
    emit_cpi!(PackSealed { pending_pack: key, slot_hash: ctx.accounts.pending_pack.slot_hash });
    Ok(())
}
