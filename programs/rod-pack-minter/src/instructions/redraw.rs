use anchor_lang::prelude::*;

use crate::instructions::seal::{lookup_slot_hash, pay_retry, SlotLookup};
use crate::{
    constants::*,
    error::ErrorCode,
    state::{Config, PackRedrawn, PendingPack},
};

/// Gives a pack that can no longer be opened a fresh purchase slot, instead of a refund. Allowed only when nobody
/// sealed or opened it while its purchase slot's hash was in SlotHashes (~512 slots, ~3.4 minutes, i.e. an outage of
/// every opener). The price stays in the vault; the cards now come from the hash of the current slot, which does not
/// exist yet, so the redraw is as fair as a new purchase. The backend approver calls it automatically (or the admin);
/// the caller is paid one retry fee from the player's retry pool (while it lasts), so the player still pays the fees.
#[event_cpi]
#[derive(Accounts)]
pub struct RedrawPack<'info> {
    /// The backend approver (automatically) or the admin. Not anyone: the new draw slot is the slot the redraw lands
    /// in, so a player or a colluding validator must not be able to time it.
    #[account(mut)]
    pub caller: Signer<'info>,
    #[account(seeds = [CONFIG_SEED], bump = config.bump)]
    pub config: Box<Account<'info, Config>>,
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
    let who = ctx.accounts.caller.key();
    require!(who == ctx.accounts.config.approver || who == ctx.accounts.config.admin, ErrorCode::NotRedrawer);
    let pending = &ctx.accounts.pending_pack;
    require!(!pending.sealed, ErrorCode::AlreadySealed);
    match lookup_slot_hash(&ctx.accounts.slot_hashes.to_account_info(), pending.purchase_slot)? {
        SlotLookup::Expired => {}
        _ => return err!(ErrorCode::PackStillOpenable),
    }
    let old_slot = pending.purchase_slot;
    let new_slot = Clock::get()?.slot;

    let key = ctx.accounts.pending_pack.key();
    let p = &mut ctx.accounts.pending_pack;
    p.purchase_slot = new_slot;
    p.redraws = p.redraws.saturating_add(1);
    let redraws = p.redraws;
    pay_retry(&mut ctx.accounts.pending_pack, &ctx.accounts.caller.to_account_info())?;
    let retry_pool = ctx.accounts.pending_pack.retry_pool;
    emit_cpi!(PackRedrawn { pending_pack: key, old_slot, new_slot, redraws, retry_pool });
    Ok(())
}
