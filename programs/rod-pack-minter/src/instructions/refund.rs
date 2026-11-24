use anchor_lang::prelude::*;
use anchor_spl::token_interface::{transfer_checked, Mint, TokenAccount, TokenInterface, TransferChecked};

use crate::instructions::seal::pay_from_pending;
use crate::{
    constants::*,
    error::ErrorCode,
    state::{Config, PackRefunded, PendingPack},
};

/// Support tool, not part of normal operation (failed transactions are retried and outages are handled by
/// `redraw_pack`): the admin returns the price of an unopened pack. Admin-only on purpose: the cards of any paid pack are public from the slot after purchase, so a refund
/// the player could trigger would let them keep good packs and return bad ones. The price goes to the player's own
/// $BOOTY account, the open fee to the admin (who sent the transaction instead of an opener), and the rent and the
/// unused retry pool back to the player.
#[event_cpi]
#[derive(Accounts)]
pub struct RefundPack<'info> {
    #[account(mut)]
    pub admin: Signer<'info>,
    #[account(mut, seeds = [CONFIG_SEED], bump = config.bump, has_one = admin @ ErrorCode::Unauthorized)]
    pub config: Box<Account<'info, Config>>,
    #[account(
        mut, close = player,
        seeds = [PACK_SEED, pending_pack.player.as_ref(), pending_pack.order_id.as_ref()],
        bump = pending_pack.bump
    )]
    pub pending_pack: Box<Account<'info, PendingPack>>,
    /// CHECK: must be the pack's player; receives the PendingPack rent.
    #[account(mut, address = pending_pack.player @ ErrorCode::WrongPlayer)]
    pub player: UncheckedAccount<'info>,
    #[account(mut, address = config.vault @ ErrorCode::WrongVault)]
    pub vault: Box<InterfaceAccount<'info, TokenAccount>>,
    /// A $BOOTY account owned by the player.
    #[account(mut, token::mint = booty_mint, token::authority = player)]
    pub player_token: Box<InterfaceAccount<'info, TokenAccount>>,
    #[account(address = config.booty_mint @ ErrorCode::WrongPaymentAccount)]
    pub booty_mint: Box<InterfaceAccount<'info, Mint>>,
    pub token_program: Interface<'info, TokenInterface>,
}

pub fn handle_refund_pack(ctx: Context<RefundPack>) -> Result<()> {
    require_keys_neq!(ctx.accounts.player_token.key(), ctx.accounts.vault.key(), ErrorCode::VaultNotAllowed);
    let now = Clock::get()?.unix_timestamp;
    let pending = &ctx.accounts.pending_pack;
    let config = &ctx.accounts.config;
    require!(now >= pending.purchased_at.saturating_add(config.refund_after_secs), ErrorCode::TooEarlyToRefund);

    let price = pending.price;
    if price > 0 {
        let bump = [config.bump];
        let seeds: &[&[u8]] = &[CONFIG_SEED, &bump];
        transfer_checked(
            CpiContext::new_with_signer(
                ctx.accounts.token_program.key(),
                TransferChecked {
                    from: ctx.accounts.vault.to_account_info(),
                    mint: ctx.accounts.booty_mint.to_account_info(),
                    to: ctx.accounts.player_token.to_account_info(),
                    authority: ctx.accounts.config.to_account_info(),
                },
                &[seeds],
            ),
            price,
            ctx.accounts.booty_mint.decimals,
        )?;
    }
    let (expansion_id, order_id, player, pack_size) = (pending.expansion_id, pending.order_id, pending.player, pending.pack_size);
    // The admin sent this transaction instead of an opener: it gets the open fee; the rest returns to the player.
    let fees = pending.open_fee;
    pay_from_pending(&ctx.accounts.pending_pack.to_account_info(), &ctx.accounts.admin.to_account_info(), fees)?;
    let config = &mut ctx.accounts.config;
    config.pending_packs = config.pending_packs.checked_sub(1).ok_or(ErrorCode::Overflow)?;
    config.reserved_leaves = config.reserved_leaves.saturating_sub(pack_size as u64);
    config.packs_refunded = config.packs_refunded.checked_add(1).ok_or(ErrorCode::Overflow)?;
    emit_cpi!(PackRefunded { expansion_id, order_id, player, price });
    Ok(())
}
