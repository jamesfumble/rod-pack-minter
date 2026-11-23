use anchor_lang::prelude::*;
use anchor_spl::token_interface::{Mint, TokenAccount};

use crate::{constants::*, error::ErrorCode, state::Config};

/// One-off set-up. Only the program's upgrade authority may call it, so nobody can front-run the deploy and make
/// themselves admin. The upgrade authority becomes the admin.
#[derive(Accounts)]
pub struct Initialize<'info> {
    #[account(mut)]
    pub admin: Signer<'info>,
    #[account(init, payer = admin, space = 8 + Config::INIT_SPACE, seeds = [CONFIG_SEED], bump)]
    pub config: Account<'info, Config>,
    /// CHECK: PDA that signs mints; holds no data.
    #[account(seeds = [MINT_AUTHORITY_SEED], bump)]
    pub mint_authority: UncheckedAccount<'info>,
    #[account(constraint = program.programdata_address()? == Some(program_data.key()) @ ErrorCode::Unauthorized)]
    pub program: Program<'info, crate::program::RodPackMinter>,
    #[account(constraint = program_data.upgrade_authority_address == Some(admin.key()) @ ErrorCode::Unauthorized)]
    pub program_data: Account<'info, ProgramData>,
    pub booty_mint: InterfaceAccount<'info, Mint>,
    pub booty_destination: InterfaceAccount<'info, TokenAccount>,
    pub system_program: Program<'info, System>,
}

pub fn handle_initialize(
    ctx: Context<Initialize>,
    approver: Pubkey,
    tree_funder: Pubkey,
    tree_max_depth: u32,
    tree_max_buffer: u32,
    tree_canopy_depth: u32,
) -> Result<()> {
    require_keys_eq!(ctx.accounts.booty_destination.mint, ctx.accounts.booty_mint.key(), ErrorCode::WrongPaymentAccount);
    require!(tree_canopy_depth < tree_max_depth, ErrorCode::BadTreeAccount);
    let c = &mut ctx.accounts.config;
    c.admin = ctx.accounts.admin.key();
    c.pending_admin = Pubkey::default();
    c.approver = approver;
    c.tree_funder = tree_funder;
    c.paused = true;
    c.booty_mint = ctx.accounts.booty_mint.key();
    c.booty_destination = ctx.accounts.booty_destination.key();
    c.active_tree = Pubkey::default();
    c.standby_tree = Pubkey::default();
    c.tree_max_depth = tree_max_depth;
    c.tree_max_buffer = tree_max_buffer;
    c.tree_canopy_depth = tree_canopy_depth;
    c.trees_created = 0;
    c.next_expansion_id = 1;
    c.packs_sold = 0;
    c.cards_minted = 0;
    c.bump = ctx.bumps.config;
    c.mint_authority_bump = ctx.bumps.mint_authority;
    Ok(())
}
