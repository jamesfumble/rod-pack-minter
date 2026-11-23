use anchor_lang::prelude::*;

use crate::instructions::seal::{lookup_slot_hash, pay_from_pending, SlotLookup};
use crate::{
    constants::*,
    error::ErrorCode,
    state::{PackRedrawn, PendingPack},
};

/// Gives a pack that can no longer be opened a fresh purchase slot, instead of a refund. Allowed only when nobody
/// sealed or opened it while its purchase slot's hash was in SlotHashes (~512 slots, ~3.4 minutes, i.e. an outage of
/// every opener). The price stays in the vault; the cards now come from the hash of the current slot, which does not
/// exist yet, so the redraw is as fair as a new purchase. Anyone may call it (the backend does so automatically); the
/// caller receives the player's seal deposit (once), so the player still pays every fee.
#[event_cpi]
#[derive(Accounts)]
pub struct RedrawPack<'info> {
    #[account(mut)]
    pub caller: Signer<'info>,
    #[account(
        mut,
        seeds = [PACK_SEED, pending_pack.player.as_ref(), pending_pack.order_id.as_ref()],
        bump = pending_pack.bump
    )]
    pub pending_pack: Box<Account<'info, PendingPack>>,
    /// CHECK: address checked in `lookup_slot_hash`.
    pub slot_hashes: UncheckedAccount<'info>,
}

pub fn handle_redraw_pack(ctx: Context<RedrawPack>) -> Result<()> {
    let pending = &ctx.accounts.pending_pack;
    require!(!pending.sealed, ErrorCode::AlreadySealed);
    match lookup_slot_hash(&ctx.accounts.slot_hashes.to_account_info(), pending.purchase_slot)? {
        SlotLookup::Expired => {}
        _ => return err!(ErrorCode::PackStillOpenable),
    }
    let old_slot = pending.purchase_slot;
    let new_slot = Clock::get()?.slot;
    let fee = pending.seal_fee;

    let key = ctx.accounts.pending_pack.key();
    let p = &mut ctx.accounts.pending_pack;
    p.purchase_slot = new_slot;
    p.seal_fee = 0;
    p.redraws = p.redraws.saturating_add(1);
    let redraws = p.redraws;
    pay_from_pending(&ctx.accounts.pending_pack.to_account_info(), &ctx.accounts.caller.to_account_info(), fee)?;
    emit_cpi!(PackRedrawn { pending_pack: key, old_slot, new_slot, redraws });
    Ok(())
}
