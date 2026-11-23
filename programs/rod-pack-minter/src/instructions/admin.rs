use anchor_lang::prelude::*;
use anchor_spl::token_interface::{Mint, TokenAccount};

use crate::{constants::*, error::ErrorCode, state::Config};

#[derive(Accounts)]
pub struct AdminOnly<'info> {
    pub admin: Signer<'info>,
    #[account(mut, seeds = [CONFIG_SEED], bump = config.bump, has_one = admin @ ErrorCode::Unauthorized)]
    pub config: Account<'info, Config>,
}

pub fn handle_set_paused(ctx: Context<AdminOnly>, paused: bool) -> Result<()> {
    ctx.accounts.config.paused = paused;
    Ok(())
}

pub fn handle_set_approver(ctx: Context<AdminOnly>, approver: Pubkey) -> Result<()> {
    ctx.accounts.config.approver = approver;
    Ok(())
}

pub fn handle_set_tree_funder(ctx: Context<AdminOnly>, tree_funder: Pubkey) -> Result<()> {
    ctx.accounts.config.tree_funder = tree_funder;
    Ok(())
}

/// Size of the trees `create_tree` makes from now on (existing trees are unchanged).
pub fn handle_set_tree_size(ctx: Context<AdminOnly>, max_depth: u32, max_buffer: u32, canopy_depth: u32) -> Result<()> {
    require!(canopy_depth < max_depth, ErrorCode::BadTreeAccount);
    let c = &mut ctx.accounts.config;
    c.tree_max_depth = max_depth;
    c.tree_max_buffer = max_buffer;
    c.tree_canopy_depth = canopy_depth;
    Ok(())
}

/// First step of an admin change; the new admin must sign `accept_admin`. Pubkey::default() cancels.
pub fn handle_propose_admin(ctx: Context<AdminOnly>, new_admin: Pubkey) -> Result<()> {
    ctx.accounts.config.pending_admin = new_admin;
    Ok(())
}

#[derive(Accounts)]
pub struct AcceptAdmin<'info> {
    pub new_admin: Signer<'info>,
    #[account(mut, seeds = [CONFIG_SEED], bump = config.bump)]
    pub config: Account<'info, Config>,
}

pub fn handle_accept_admin(ctx: Context<AcceptAdmin>) -> Result<()> {
    let c = &mut ctx.accounts.config;
    require!(c.pending_admin != Pubkey::default(), ErrorCode::NotPendingAdmin);
    require_keys_eq!(c.pending_admin, ctx.accounts.new_admin.key(), ErrorCode::NotPendingAdmin);
    c.admin = c.pending_admin;
    c.pending_admin = Pubkey::default();
    Ok(())
}

#[derive(Accounts)]
pub struct SetPayment<'info> {
    pub admin: Signer<'info>,
    #[account(mut, seeds = [CONFIG_SEED], bump = config.bump, has_one = admin @ ErrorCode::Unauthorized)]
    pub config: Account<'info, Config>,
    pub booty_mint: InterfaceAccount<'info, Mint>,
    pub booty_destination: InterfaceAccount<'info, TokenAccount>,
}

pub fn handle_set_payment(ctx: Context<SetPayment>) -> Result<()> {
    require_keys_eq!(ctx.accounts.booty_destination.mint, ctx.accounts.booty_mint.key(), ErrorCode::WrongPaymentAccount);
    let c = &mut ctx.accounts.config;
    c.booty_mint = ctx.accounts.booty_mint.key();
    c.booty_destination = ctx.accounts.booty_destination.key();
    Ok(())
}
