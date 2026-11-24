use anchor_lang::prelude::*;
use anchor_lang::system_program::{transfer, Transfer};
use anchor_lang::Discriminator;
use anchor_spl::token_interface::{Mint, TokenAccount, TokenInterface};

use crate::{constants::*, error::ErrorCode, instructions::initialize::require_plain_mint, state::{Config, LustConfig}};

#[derive(Accounts)]
pub struct AdminOnly<'info> {
    pub admin: Signer<'info>,
    #[account(mut, seeds = [CONFIG_SEED], bump = config.bump, has_one = admin @ ErrorCode::Unauthorized)]
    pub config: Box<Account<'info, Config>>,
}

pub fn validate_tree_size(max_depth: u32, max_buffer: u32, canopy_depth: u32) -> Result<()> {
    require!((MIN_TREE_DEPTH..=MAX_TREE_DEPTH).contains(&max_depth), ErrorCode::BadTreeAccount);
    require!(canopy_depth < max_depth && max_buffer >= 8 && max_buffer <= 2048 && max_buffer.is_power_of_two(), ErrorCode::BadTreeAccount);
    Ok(())
}

pub fn validate_fees(open_fee: u64, retry_fee: u64, paid_retries: u8, refund_after_secs: i64) -> Result<()> {
    require!(open_fee <= MAX_FEE_LAMPORTS && retry_fee <= MAX_FEE_LAMPORTS && paid_retries <= MAX_PAID_RETRIES, ErrorCode::FeeTooHigh);
    // At least 10 minutes, so an admin refund can never race an opener that is merely slow.
    require!(refund_after_secs >= 600, ErrorCode::TooEarlyToRefund);
    Ok(())
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
    validate_tree_size(max_depth, max_buffer, canopy_depth)?;
    let c = &mut ctx.accounts.config;
    c.tree_max_depth = max_depth;
    c.tree_max_buffer = max_buffer;
    c.tree_canopy_depth = canopy_depth;
    Ok(())
}

/// The player's per-purchase deposit (open fee + `paid_retries` retry fees) and the refund delay. Applies to new
/// purchases.
pub fn handle_set_fees(ctx: Context<AdminOnly>, open_fee_lamports: u64, retry_fee_lamports: u64, paid_retries: u8, refund_after_secs: i64) -> Result<()> {
    validate_fees(open_fee_lamports, retry_fee_lamports, paid_retries, refund_after_secs)?;
    let c = &mut ctx.accounts.config;
    c.open_fee_lamports = open_fee_lamports;
    c.retry_fee_lamports = retry_fee_lamports;
    c.paid_retries = paid_retries;
    c.refund_after_secs = refund_after_secs;
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
    pub config: Box<Account<'info, Config>>,
}

pub fn handle_accept_admin(ctx: Context<AcceptAdmin>) -> Result<()> {
    let c = &mut ctx.accounts.config;
    require!(c.pending_admin != Pubkey::default(), ErrorCode::NotPendingAdmin);
    require_keys_eq!(c.pending_admin, ctx.accounts.new_admin.key(), ErrorCode::NotPendingAdmin);
    c.admin = c.pending_admin;
    c.pending_admin = Pubkey::default();
    Ok(())
}

/// Creates the $BOOTY vault (PDA ["vault", mint], authority = config) for a mint, ahead of `set_payment`.
#[derive(Accounts)]
pub struct InitVault<'info> {
    #[account(mut)]
    pub admin: Signer<'info>,
    #[account(seeds = [CONFIG_SEED], bump = config.bump, has_one = admin @ ErrorCode::Unauthorized)]
    pub config: Box<Account<'info, Config>>,
    pub booty_mint: Box<InterfaceAccount<'info, Mint>>,
    #[account(
        init, payer = admin, seeds = [VAULT_SEED, booty_mint.key().as_ref()], bump,
        token::mint = booty_mint, token::authority = config, token::token_program = token_program
    )]
    pub vault: Box<InterfaceAccount<'info, TokenAccount>>,
    pub token_program: Interface<'info, TokenInterface>,
    pub system_program: Program<'info, System>,
}

pub fn handle_init_vault(ctx: Context<InitVault>) -> Result<()> {
    require_plain_mint(&ctx.accounts.booty_mint.to_account_info())
}

/// Switches the payment mint, destination and vault. Only while no pack is pending, so every pending price stays in
/// the vault it was paid into.
#[derive(Accounts)]
pub struct SetPayment<'info> {
    pub admin: Signer<'info>,
    #[account(mut, seeds = [CONFIG_SEED], bump = config.bump, has_one = admin @ ErrorCode::Unauthorized)]
    pub config: Box<Account<'info, Config>>,
    pub booty_mint: Box<InterfaceAccount<'info, Mint>>,
    pub booty_destination: Box<InterfaceAccount<'info, TokenAccount>>,
    #[account(seeds = [VAULT_SEED, booty_mint.key().as_ref()], bump, token::mint = booty_mint, token::authority = config)]
    pub vault: Box<InterfaceAccount<'info, TokenAccount>>,
}

pub fn handle_set_payment(ctx: Context<SetPayment>) -> Result<()> {
    require!(ctx.accounts.config.pending_packs == 0, ErrorCode::PacksPending);
    require_keys_neq!(ctx.accounts.booty_destination.key(), ctx.accounts.vault.key(), ErrorCode::VaultNotAllowed);
    require_keys_eq!(ctx.accounts.booty_destination.mint, ctx.accounts.booty_mint.key(), ErrorCode::WrongPaymentAccount);
    require_plain_mint(&ctx.accounts.booty_mint.to_account_info())?;
    let c = &mut ctx.accounts.config;
    c.booty_mint = ctx.accounts.booty_mint.key();
    c.booty_destination = ctx.accounts.booty_destination.key();
    c.vault = ctx.accounts.vault.key();
    Ok(())
}

/// Replaces the shop's $BOOTY account (same mint) at any time, e.g. if the old one was closed or frozen, which would
/// otherwise block every `open_pack`.
#[derive(Accounts)]
pub struct SetDestination<'info> {
    pub admin: Signer<'info>,
    #[account(mut, seeds = [CONFIG_SEED], bump = config.bump, has_one = admin @ ErrorCode::Unauthorized)]
    pub config: Box<Account<'info, Config>>,
    #[account(token::mint = config.booty_mint)]
    pub booty_destination: Box<InterfaceAccount<'info, TokenAccount>>,
}

pub fn handle_set_destination(ctx: Context<SetDestination>) -> Result<()> {
    require_keys_neq!(ctx.accounts.booty_destination.key(), ctx.accounts.config.vault, ErrorCode::VaultNotAllowed);
    ctx.accounts.config.booty_destination = ctx.accounts.booty_destination.key();
    Ok(())
}

/// Sets up or changes $LUST payments: the mint (its vault must exist: `init_vault`), the four split destinations
/// (team, giveaway, stake, absorption pool) and their shares in basis points; the rest of every $LUST price is burned.
/// The mint can only change while no pack is pending.
#[derive(Accounts)]
pub struct SetLustPayment<'info> {
    #[account(mut)]
    pub admin: Signer<'info>,
    #[account(seeds = [CONFIG_SEED], bump = config.bump, has_one = admin @ ErrorCode::Unauthorized)]
    pub config: Box<Account<'info, Config>>,
    #[account(init_if_needed, payer = admin, space = 8 + LustConfig::INIT_SPACE, seeds = [LUST_SEED], bump)]
    pub lust_config: Box<Account<'info, LustConfig>>,
    pub lust_mint: Box<InterfaceAccount<'info, Mint>>,
    #[account(seeds = [VAULT_SEED, lust_mint.key().as_ref()], bump, token::mint = lust_mint, token::authority = config)]
    pub vault: Box<InterfaceAccount<'info, TokenAccount>>,
    #[account(token::mint = lust_mint)]
    pub team: Box<InterfaceAccount<'info, TokenAccount>>,
    #[account(token::mint = lust_mint)]
    pub giveaway: Box<InterfaceAccount<'info, TokenAccount>>,
    #[account(token::mint = lust_mint)]
    pub stake: Box<InterfaceAccount<'info, TokenAccount>>,
    #[account(token::mint = lust_mint)]
    pub absorption: Box<InterfaceAccount<'info, TokenAccount>>,
    pub system_program: Program<'info, System>,
}

pub fn handle_set_lust_payment(ctx: Context<SetLustPayment>, split_bps: [u16; LUST_SPLITS]) -> Result<()> {
    require!(split_bps.iter().map(|b| *b as u32).sum::<u32>() <= 10_000, ErrorCode::BadSplit);
    require_plain_mint(&ctx.accounts.lust_mint.to_account_info())?;
    let mint = ctx.accounts.lust_mint.key();
    let l = &ctx.accounts.lust_config;
    require!(l.mint == Pubkey::default() || l.mint == mint || ctx.accounts.config.pending_packs == 0, ErrorCode::PacksPending);
    let vault = ctx.accounts.vault.key();
    let destinations = [ctx.accounts.team.key(), ctx.accounts.giveaway.key(), ctx.accounts.stake.key(), ctx.accounts.absorption.key()];
    require!(!destinations.contains(&vault), ErrorCode::VaultNotAllowed);
    let bump = ctx.bumps.lust_config;
    let l = &mut ctx.accounts.lust_config;
    l.mint = mint;
    l.vault = vault;
    l.destinations = destinations;
    l.split_bps = split_bps;
    l.bump = bump;
    Ok(())
}

/// One-off: extends a v1 config (created before the two-step purchase) to the current layout. The new fields are
/// appended, so the old bytes stay valid; `init_vault` + `set_payment` then register the vault. A fresh `initialize`
/// already creates the current layout, so this fails on it.
#[derive(Accounts)]
pub struct MigrateConfig<'info> {
    #[account(mut)]
    pub admin: Signer<'info>,
    /// CHECK: owner, discriminator, size and admin are checked by hand (it cannot be deserialised before the resize).
    #[account(mut, seeds = [CONFIG_SEED], bump)]
    pub config: UncheckedAccount<'info>,
    pub system_program: Program<'info, System>,
}

pub fn handle_migrate_config(ctx: Context<MigrateConfig>, open_fee_lamports: u64, retry_fee_lamports: u64, paid_retries: u8, refund_after_secs: i64) -> Result<()> {
    validate_fees(open_fee_lamports, retry_fee_lamports, paid_retries, refund_after_secs)?;
    let info = ctx.accounts.config.to_account_info();
    require_keys_eq!(*info.owner, crate::ID, ErrorCode::Unauthorized);
    require!(info.data_len() == Config::V1_LEN, ErrorCode::AlreadyMigrated);
    {
        let data = info.try_borrow_data()?;
        require!(&data[0..8] == Config::DISCRIMINATOR, ErrorCode::Unauthorized);
        require!(data[8..40] == ctx.accounts.admin.key().to_bytes(), ErrorCode::Unauthorized);
    }
    let new_len = 8 + Config::INIT_SPACE;
    let needed = Rent::get()?.minimum_balance(new_len).saturating_sub(info.lamports());
    if needed > 0 {
        transfer(
            CpiContext::new(ctx.accounts.system_program.key(), Transfer { from: ctx.accounts.admin.to_account_info(), to: info.clone() }),
            needed,
        )?;
    }
    info.resize(new_len)?;
    let mut data = info.try_borrow_mut_data()?;
    let o = Config::V1_LEN + 32; // vault stays zero (none) until set_payment
    data[o..o + 8].copy_from_slice(&open_fee_lamports.to_le_bytes());
    data[o + 8..o + 16].copy_from_slice(&retry_fee_lamports.to_le_bytes());
    data[o + 16..o + 24].copy_from_slice(&refund_after_secs.to_le_bytes());
    // vault 32 | open 8 | retry 8 | refund 8 | pending 8 | refunded 8 | reserved_leaves 8 | paid_retries u8
    data[Config::V1_LEN + 80] = paid_retries;
    Ok(())
}
