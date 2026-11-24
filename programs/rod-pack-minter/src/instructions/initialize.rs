use anchor_lang::prelude::*;
use anchor_spl::token_interface::{Mint, TokenAccount, TokenInterface};

use crate::{constants::*, error::ErrorCode, instructions::admin::validate_fees, instructions::admin::validate_tree_size, state::Config};

/// SPL mints are 82 bytes; Token-2022 mints with extensions are longer. Extensions such as transfer fees, transfer
/// hooks or a permanent delegate would break or undermine payments, so only plain mints are accepted.
pub fn require_plain_mint(mint: &AccountInfo) -> Result<()> {
    require!(mint.data_len() == 82, ErrorCode::MintHasExtensions);
    Ok(())
}

/// One-off set-up. Only the program's upgrade authority may call it, so nobody can front-run the deploy and make
/// themselves admin. The upgrade authority becomes the admin. Creates the $BOOTY vault.
#[derive(Accounts)]
pub struct Initialize<'info> {
    #[account(mut)]
    pub admin: Signer<'info>,
    #[account(init, payer = admin, space = 8 + Config::INIT_SPACE, seeds = [CONFIG_SEED], bump)]
    pub config: Box<Account<'info, Config>>,
    /// CHECK: PDA that signs mints; holds no data.
    #[account(seeds = [MINT_AUTHORITY_SEED], bump)]
    pub mint_authority: UncheckedAccount<'info>,
    #[account(constraint = program.programdata_address()? == Some(program_data.key()) @ ErrorCode::Unauthorized)]
    pub program: Program<'info, crate::program::RodPackMinter>,
    #[account(constraint = program_data.upgrade_authority_address == Some(admin.key()) @ ErrorCode::Unauthorized)]
    pub program_data: Account<'info, ProgramData>,
    pub booty_mint: Box<InterfaceAccount<'info, Mint>>,
    pub booty_destination: Box<InterfaceAccount<'info, TokenAccount>>,
    #[account(
        init, payer = admin, seeds = [VAULT_SEED, booty_mint.key().as_ref()], bump,
        token::mint = booty_mint, token::authority = config, token::token_program = token_program
    )]
    pub vault: Box<InterfaceAccount<'info, TokenAccount>>,
    pub token_program: Interface<'info, TokenInterface>,
    pub system_program: Program<'info, System>,
}

#[derive(AnchorSerialize, AnchorDeserialize, Clone, Debug)]
pub struct InitParams {
    pub approver: Pubkey,
    pub tree_funder: Pubkey,
    pub tree_max_depth: u32,
    pub tree_max_buffer: u32,
    pub tree_canopy_depth: u32,
    pub open_fee_lamports: u64,
    pub retry_fee_lamports: u64,
    pub paid_retries: u8,
    pub refund_after_secs: i64,
}

pub fn handle_initialize(ctx: Context<Initialize>, p: InitParams) -> Result<()> {
    require_keys_eq!(ctx.accounts.booty_destination.mint, ctx.accounts.booty_mint.key(), ErrorCode::WrongPaymentAccount);
    require_plain_mint(&ctx.accounts.booty_mint.to_account_info())?;
    require_keys_neq!(ctx.accounts.booty_destination.key(), ctx.accounts.vault.key(), ErrorCode::VaultNotAllowed);
    validate_tree_size(p.tree_max_depth, p.tree_max_buffer, p.tree_canopy_depth)?;
    validate_fees(p.open_fee_lamports, p.retry_fee_lamports, p.paid_retries, p.refund_after_secs)?;
    let c = &mut ctx.accounts.config;
    c.admin = ctx.accounts.admin.key();
    c.pending_admin = Pubkey::default();
    c.approver = p.approver;
    c.tree_funder = p.tree_funder;
    c.paused = true;
    c.booty_mint = ctx.accounts.booty_mint.key();
    c.booty_destination = ctx.accounts.booty_destination.key();
    c.active_tree = Pubkey::default();
    c.standby_tree = Pubkey::default();
    c.tree_max_depth = p.tree_max_depth;
    c.tree_max_buffer = p.tree_max_buffer;
    c.tree_canopy_depth = p.tree_canopy_depth;
    c.trees_created = 0;
    c.next_expansion_id = 1;
    c.packs_sold = 0;
    c.cards_minted = 0;
    c.bump = ctx.bumps.config;
    c.mint_authority_bump = ctx.bumps.mint_authority;
    c.vault = ctx.accounts.vault.key();
    c.open_fee_lamports = p.open_fee_lamports;
    c.retry_fee_lamports = p.retry_fee_lamports;
    c.paid_retries = p.paid_retries;
    c.refund_after_secs = p.refund_after_secs;
    c.pending_packs = 0;
    c.packs_refunded = 0;
    c.reserved_leaves = 0;
    c._reserved = [0u8; 55];
    Ok(())
}
