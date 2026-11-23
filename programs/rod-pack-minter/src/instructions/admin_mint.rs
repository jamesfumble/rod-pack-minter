use anchor_lang::prelude::*;

use crate::instructions::mint::*;
use crate::{
    constants::*,
    error::ErrorCode,
    state::{AdminMinted, CardTable, Config, Expansion},
};

/// The only way to choose a card: admin-signed, no payment. Blocked while the program or the expansion is paused.
#[event_cpi]
#[derive(Accounts)]
#[instruction(expansion_id: u16)]
pub struct AdminMint<'info> {
    #[account(mut)]
    pub admin: Signer<'info>,
    #[account(mut, seeds = [CONFIG_SEED], bump = config.bump, has_one = admin @ ErrorCode::Unauthorized)]
    pub config: Box<Account<'info, Config>>,
    #[account(mut, seeds = [EXPANSION_SEED, expansion_id.to_le_bytes().as_ref()], bump = expansion.bump)]
    pub expansion: Box<Account<'info, Expansion>>,
    #[account(seeds = [CARDS_SEED, expansion_id.to_le_bytes().as_ref()], bump = card_table.bump)]
    pub card_table: Box<Account<'info, CardTable>>,
    /// CHECK: any wallet; it only receives the cards.
    pub recipient: UncheckedAccount<'info>,
    pub mint: MintAccounts<'info>,
}

pub fn handle_admin_mint(ctx: Context<AdminMint>, expansion_id: u16, card_id: u16, count: u8) -> Result<()> {
    let config = &ctx.accounts.config;
    let expansion = &ctx.accounts.expansion;
    require!(!config.paused, ErrorCode::ProgramPaused);
    require!(!expansion.paused, ErrorCode::ExpansionPaused);
    require!(count >= 1 && count <= MAX_ADMIN_MINT, ErrorCode::BadCount);
    require!(card_id < ctx.accounts.card_table.count, ErrorCode::BadCardId);

    let view = ctx.accounts.mint.prepare(config, expansion, count as u64)?;
    let card_ids = vec![card_id; count as usize];
    let recipient = ctx.accounts.recipient.to_account_info();
    let admin = ctx.accounts.admin.to_account_info();
    ctx.accounts.mint.mint_cards(config, expansion, &ctx.accounts.card_table, &recipient, &admin, &card_ids)?;

    let tree = config.active_tree;
    let config = &mut ctx.accounts.config;
    config.cards_minted = config.cards_minted.checked_add(count as u64).ok_or(ErrorCode::Overflow)?;
    let expansion = &mut ctx.accounts.expansion;
    expansion.cards_minted = expansion.cards_minted.checked_add(count as u64).ok_or(ErrorCode::Overflow)?;

    emit_cpi!(AdminMinted {
        expansion_id,
        card_id,
        recipient: ctx.accounts.recipient.key(),
        tree,
        first_leaf_index: view.num_minted,
        count,
    });
    Ok(())
}
