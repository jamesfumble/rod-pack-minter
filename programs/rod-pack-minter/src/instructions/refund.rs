use anchor_lang::prelude::*;
use anchor_spl::token_interface::{transfer_checked, Mint, TokenAccount, TokenInterface, TransferChecked};

use crate::{
    constants::*,
    error::ErrorCode,
    state::{Config, PackRefunded, PendingPack},
};

/// The player takes back the price of a pack that nobody opened within `refund_after_secs` (a long outage). The
/// PendingPack is closed: its rent and any unused fee deposit go back to the player too.
#[event_cpi]
#[derive(Accounts)]
pub struct RefundPack<'info> {
    #[account(mut)]
    pub player: Signer<'info>,
    #[account(mut, seeds = [CONFIG_SEED], bump = config.bump)]
    pub config: Box<Account<'info, Config>>,
    #[account(
        mut, close = player, has_one = player @ ErrorCode::WrongPlayer,
        seeds = [PACK_SEED, player.key().as_ref(), pending_pack.order_id.as_ref()],
        bump = pending_pack.bump
    )]
    pub pending_pack: Box<Account<'info, PendingPack>>,
    #[account(mut, address = config.vault @ ErrorCode::WrongVault)]
    pub vault: Box<InterfaceAccount<'info, TokenAccount>>,
    /// The player's $BOOTY account (any account of the mint the player chooses).
    #[account(mut)]
    pub player_token: Box<InterfaceAccount<'info, TokenAccount>>,
    #[account(address = config.booty_mint @ ErrorCode::WrongPaymentAccount)]
    pub booty_mint: Box<InterfaceAccount<'info, Mint>>,
    pub token_program: Interface<'info, TokenInterface>,
}

pub fn handle_refund_pack(ctx: Context<RefundPack>) -> Result<()> {
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
    let (expansion_id, order_id) = (pending.expansion_id, pending.order_id);
    let config = &mut ctx.accounts.config;
    config.pending_packs = config.pending_packs.checked_sub(1).ok_or(ErrorCode::Overflow)?;
    config.packs_refunded = config.packs_refunded.checked_add(1).ok_or(ErrorCode::Overflow)?;
    emit_cpi!(PackRefunded { expansion_id, order_id, player: ctx.accounts.player.key(), price });
    Ok(())
}
