use anchor_lang::prelude::*;
use anchor_lang::solana_program::instruction::get_stack_height;
use anchor_spl::token_interface::{transfer_checked, Mint, TokenAccount, TokenInterface, TransferChecked};
use solana_instructions_sysvar::{load_current_index_checked, load_instruction_at_checked};

use crate::instructions::mint::*;
use crate::{
    constants::*,
    draw::{draw_pack, pack_seed},
    error::ErrorCode,
    state::{CardTable, Config, Expansion, PackMinted},
};

#[event_cpi]
#[derive(Accounts)]
#[instruction(expansion_id: u16)]
pub struct BuyPack<'info> {
    /// Pays the price and the fees, and receives the cards.
    #[account(mut)]
    pub player: Signer<'info>,
    /// The backend's approver key: it signs last and sends the transaction, so a player cannot wrap the purchase.
    pub approver: Signer<'info>,
    #[account(mut, seeds = [CONFIG_SEED], bump = config.bump)]
    pub config: Box<Account<'info, Config>>,
    #[account(mut, seeds = [EXPANSION_SEED, expansion_id.to_le_bytes().as_ref()], bump = expansion.bump)]
    pub expansion: Box<Account<'info, Expansion>>,
    #[account(seeds = [CARDS_SEED, expansion_id.to_le_bytes().as_ref()], bump = card_table.bump)]
    pub card_table: Box<Account<'info, CardTable>>,
    /// The player's $BOOTY account (transfer_checked enforces owner and mint).
    #[account(mut)]
    pub player_token: Box<InterfaceAccount<'info, TokenAccount>>,
    #[account(mut)]
    pub booty_destination: Box<InterfaceAccount<'info, TokenAccount>>,
    pub booty_mint: Box<InterfaceAccount<'info, Mint>>,
    pub token_program: Interface<'info, TokenInterface>,
    pub mint: MintAccounts<'info>,
    /// CHECK: address
    #[account(address = SLOT_HASHES_ID)]
    pub slot_hashes: UncheckedAccount<'info>,
    /// CHECK: address
    #[account(address = solana_instructions_sysvar::ID)]
    pub instructions: UncheckedAccount<'info>,
}

/// Rejects a CPI call and any instruction after this one other than compute budget or Lighthouse assertions that
/// do not touch the pack accounts. A later instruction could otherwise inspect the result and fail the transaction
/// to re-roll the cards.
fn check_instructions(ctx: &Context<BuyPack>) -> Result<()> {
    require!(get_stack_height() == 1, ErrorCode::NotTopLevel);
    let ixs = ctx.accounts.instructions.to_account_info();
    let current = load_current_index_checked(&ixs)? as usize;
    let me = load_instruction_at_checked(current, &ixs)?;
    require_keys_eq!(me.program_id, crate::ID, ErrorCode::NotTopLevel);
    let protected = [
        ctx.accounts.mint.merkle_tree.key(),
        ctx.accounts.mint.tree_config.key(),
        ctx.accounts.config.key(),
        ctx.accounts.expansion.key(),
        ctx.accounts.card_table.key(),
    ];
    let mut i = current + 1;
    while let Ok(ix) = load_instruction_at_checked(i, &ixs) {
        if ix.program_id == LIGHTHOUSE_ID {
            require!(!ix.accounts.iter().any(|m| protected.contains(&m.pubkey)), ErrorCode::ForbiddenLighthouseAccount);
        } else {
            require_keys_eq!(ix.program_id, COMPUTE_BUDGET_ID, ErrorCode::ForbiddenFollowingInstruction);
        }
        i += 1;
    }
    Ok(())
}

fn newest_slot_hash(slot_hashes: &AccountInfo) -> Result<[u8; 32]> {
    // SlotHashes: u64 count, then (slot u64, hash [u8; 32]) newest first.
    let data = slot_hashes.try_borrow_data()?;
    require!(data.len() >= 48, ErrorCode::BadSlotHashes);
    let count = u64::from_le_bytes(data[0..8].try_into().unwrap());
    require!(count > 0, ErrorCode::BadSlotHashes);
    Ok(data[16..48].try_into().unwrap())
}

pub fn handle_buy_pack(ctx: Context<BuyPack>, expansion_id: u16, order_id: [u8; 16], max_price: u64) -> Result<()> {
    let config = &ctx.accounts.config;
    let expansion = &ctx.accounts.expansion;
    require!(!config.paused, ErrorCode::ProgramPaused);
    require!(!expansion.paused, ErrorCode::ExpansionPaused);
    require_keys_eq!(ctx.accounts.approver.key(), config.approver, ErrorCode::WrongApprover);
    let now = Clock::get()?.unix_timestamp;
    require!(expansion.on_sale(now), ErrorCode::OutsideSalePeriod);
    check_instructions(&ctx)?;

    let price = expansion.current_price(now);
    require!(price <= max_price, ErrorCode::PriceAboveMax);
    require_keys_eq!(ctx.accounts.booty_mint.key(), config.booty_mint, ErrorCode::WrongPaymentAccount);
    require_keys_eq!(ctx.accounts.booty_destination.key(), config.booty_destination, ErrorCode::WrongPaymentAccount);

    let pack_size = expansion.pack_size;
    let view = ctx.accounts.mint.prepare(config, expansion, pack_size as u64)?;

    // Draw the cards.
    let table = &ctx.accounts.card_table;
    require!(!table.cards.is_empty(), ErrorCode::NoCards);
    let slot_hash = newest_slot_hash(&ctx.accounts.slot_hashes.to_account_info())?;
    let tree = config.active_tree;
    let seed = pack_seed(&slot_hash, &order_id, &ctx.accounts.player.key().to_bytes(), &tree.to_bytes(), view.num_minted, expansion_id);
    let counts = table.rarity_counts();
    let picks = draw_pack(&seed, pack_size, &expansion.weights_regular, &expansion.weights_guaranteed, &counts).ok_or(ErrorCode::BadWeights)?;
    let mut card_ids = Vec::with_capacity(picks.len());
    for (rarity, index) in picks {
        card_ids.push(table.nth_of_rarity(rarity, index).ok_or(ErrorCode::BadCardId)?);
    }

    // Payment.
    if price > 0 {
        transfer_checked(
            CpiContext::new(
                ctx.accounts.token_program.key(),
                TransferChecked {
                    from: ctx.accounts.player_token.to_account_info(),
                    mint: ctx.accounts.booty_mint.to_account_info(),
                    to: ctx.accounts.booty_destination.to_account_info(),
                    authority: ctx.accounts.player.to_account_info(),
                },
            ),
            price,
            ctx.accounts.booty_mint.decimals,
        )?;
    }

    // Mint.
    let player = ctx.accounts.player.to_account_info();
    ctx.accounts.mint.mint_cards(config, expansion, table, &player, &player, &card_ids)?;

    let n = card_ids.len() as u64;
    let config = &mut ctx.accounts.config;
    config.packs_sold = config.packs_sold.checked_add(1).ok_or(ErrorCode::Overflow)?;
    config.cards_minted = config.cards_minted.checked_add(n).ok_or(ErrorCode::Overflow)?;
    let expansion = &mut ctx.accounts.expansion;
    expansion.packs_sold = expansion.packs_sold.checked_add(1).ok_or(ErrorCode::Overflow)?;
    expansion.cards_minted = expansion.cards_minted.checked_add(n).ok_or(ErrorCode::Overflow)?;

    emit_cpi!(PackMinted {
        expansion_id,
        order_id,
        player: ctx.accounts.player.key(),
        tree,
        first_leaf_index: view.num_minted,
        card_ids,
        price,
        seed,
    });
    Ok(())
}
