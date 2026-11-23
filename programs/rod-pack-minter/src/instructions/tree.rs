use anchor_lang::prelude::*;

use crate::{
    bubblegum::{create_tree_config, merkle_tree_account_size, tree_config_address, TreeConfigView},
    constants::*,
    error::ErrorCode,
    state::{Config, TreeCreated, TreeRotated},
};

/// Makes a program tree. The transaction must first allocate `merkle_tree` with a top-level system `create_account`
/// (owner: account compression, size: the configured tree size): a program can only create 10 KiB through CPI.
/// The mint authority PDA becomes tree creator and delegate, so only this program can mint into it.
#[event_cpi]
#[derive(Accounts)]
pub struct CreateTree<'info> {
    /// The admin or the tree funder; pays the tree config rent.
    #[account(mut)]
    pub authority: Signer<'info>,
    #[account(mut, seeds = [CONFIG_SEED], bump = config.bump)]
    pub config: Account<'info, Config>,
    /// CHECK: owner and size checked in the handler; account compression initialises it.
    #[account(mut)]
    pub merkle_tree: UncheckedAccount<'info>,
    /// CHECK: address checked in the handler; Bubblegum creates it.
    #[account(mut)]
    pub tree_config: UncheckedAccount<'info>,
    /// CHECK: PDA, signs as tree creator.
    #[account(seeds = [MINT_AUTHORITY_SEED], bump = config.mint_authority_bump)]
    pub mint_authority: UncheckedAccount<'info>,
    /// CHECK: address
    #[account(address = NOOP_ID)]
    pub log_wrapper: UncheckedAccount<'info>,
    /// CHECK: address
    #[account(address = ACCOUNT_COMPRESSION_ID)]
    pub compression_program: UncheckedAccount<'info>,
    /// CHECK: address
    #[account(address = BUBBLEGUM_ID)]
    pub bubblegum_program: UncheckedAccount<'info>,
    pub system_program: Program<'info, System>,
}

pub fn handle_create_tree(ctx: Context<CreateTree>) -> Result<()> {
    let config = &ctx.accounts.config;
    let who = ctx.accounts.authority.key();
    require!(who == config.admin || who == config.tree_funder, ErrorCode::Unauthorized);
    require!(config.standby_tree == Pubkey::default(), ErrorCode::StandbyExists);

    let tree = &ctx.accounts.merkle_tree;
    require_keys_eq!(*tree.owner, ACCOUNT_COMPRESSION_ID, ErrorCode::BadTreeAccount);
    let size = merkle_tree_account_size(config.tree_max_depth, config.tree_max_buffer, config.tree_canopy_depth);
    require!(tree.data_len() == size, ErrorCode::BadTreeAccount);
    require_keys_eq!(ctx.accounts.tree_config.key(), tree_config_address(&tree.key()), ErrorCode::BadTreeConfig);

    let bump = [config.mint_authority_bump];
    let seeds: &[&[u8]] = &[MINT_AUTHORITY_SEED, &bump];
    create_tree_config(
        &ctx.accounts.tree_config.to_account_info(),
        &tree.to_account_info(),
        &ctx.accounts.authority.to_account_info(),
        &ctx.accounts.mint_authority.to_account_info(),
        &ctx.accounts.log_wrapper.to_account_info(),
        &ctx.accounts.compression_program.to_account_info(),
        &ctx.accounts.system_program.to_account_info(),
        &ctx.accounts.bubblegum_program.to_account_info(),
        config.tree_max_depth,
        config.tree_max_buffer,
        &[seeds],
    )?;

    let (max_depth, max_buffer, canopy_depth) = (config.tree_max_depth, config.tree_max_buffer, config.tree_canopy_depth);
    let config = &mut ctx.accounts.config;
    let as_active = config.active_tree == Pubkey::default();
    if as_active {
        config.active_tree = tree.key();
    } else {
        config.standby_tree = tree.key();
    }
    config.trees_created = config.trees_created.checked_add(1).ok_or(ErrorCode::Overflow)?;
    emit_cpi!(TreeCreated { tree: tree.key(), as_active, max_depth, max_buffer, canopy_depth });
    Ok(())
}

/// Anyone may call this: it promotes the standby tree once the active tree cannot hold the largest pack.
/// The backend puts it before `buy_pack` in the same transaction when needed.
#[event_cpi]
#[derive(Accounts)]
pub struct RotateTree<'info> {
    #[account(mut, seeds = [CONFIG_SEED], bump = config.bump)]
    pub config: Account<'info, Config>,
    /// CHECK: checked against the active tree in the handler (ignored when there is no active tree).
    pub active_tree_config: UncheckedAccount<'info>,
}

pub fn handle_rotate_tree(ctx: Context<RotateTree>) -> Result<()> {
    let config = &ctx.accounts.config;
    require!(config.standby_tree != Pubkey::default(), ErrorCode::NoStandbyTree);
    if config.active_tree != Pubkey::default() {
        let view = TreeConfigView::load(&ctx.accounts.active_tree_config.to_account_info(), &config.active_tree)?;
        require!(view.free_leaves() < ROTATE_BELOW_FREE_LEAVES, ErrorCode::TreeNotFull);
    }
    let config = &mut ctx.accounts.config;
    let old_tree = config.active_tree;
    config.active_tree = config.standby_tree;
    config.standby_tree = Pubkey::default();
    emit_cpi!(TreeRotated { old_tree, new_tree: config.active_tree });
    Ok(())
}
