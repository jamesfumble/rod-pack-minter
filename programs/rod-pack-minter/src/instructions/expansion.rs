use anchor_lang::prelude::*;
use anchor_lang::system_program::{transfer, Transfer};

use crate::{
    constants::*,
    error::ErrorCode,
    state::{write_card, CardTable, Config, Expansion},
};

#[derive(AnchorSerialize, AnchorDeserialize, Clone, Debug)]
pub struct ExpansionParams {
    pub name: String,
    pub collection_mint: Pubkey,
    pub creator: Pubkey,
    pub royalty_bps: u16,
    pub symbol: String,
    pub uri_prefix: String,
    pub pack_size: u8,
    pub weights_regular: [u16; RARITY_COUNT],
    pub weights_guaranteed: [u16; RARITY_COUNT],
    pub sale_start: i64,
    pub sale_end: i64,
}

fn validate(p: &ExpansionParams) -> Result<()> {
    require!(p.name.len() <= MAX_EXPANSION_NAME_LEN, ErrorCode::TextTooLong);
    require!(p.symbol.len() <= MAX_SYMBOL_LEN, ErrorCode::TextTooLong);
    require!(p.uri_prefix.len() <= MAX_URI_PREFIX_LEN, ErrorCode::TextTooLong);
    require!(p.royalty_bps <= 10_000, ErrorCode::BadRoyalty);
    require!(p.pack_size >= 1 && p.pack_size <= MAX_PACK_SIZE, ErrorCode::BadPackSize);
    require!(p.weights_regular.iter().map(|w| *w as u32).sum::<u32>() > 0, ErrorCode::BadWeights);
    Ok(())
}

fn apply(e: &mut Expansion, p: ExpansionParams) {
    e.name = p.name;
    e.collection_mint = p.collection_mint;
    e.creator = p.creator;
    e.royalty_bps = p.royalty_bps;
    e.symbol = p.symbol;
    e.uri_prefix = p.uri_prefix;
    e.pack_size = p.pack_size;
    e.weights_regular = p.weights_regular;
    e.weights_guaranteed = p.weights_guaranteed;
    e.sale_start = p.sale_start;
    e.sale_end = p.sale_end;
}

#[derive(Accounts)]
pub struct AddExpansion<'info> {
    #[account(mut)]
    pub admin: Signer<'info>,
    #[account(mut, seeds = [CONFIG_SEED], bump = config.bump, has_one = admin @ ErrorCode::Unauthorized)]
    pub config: Account<'info, Config>,
    #[account(
        init, payer = admin, space = 8 + Expansion::INIT_SPACE,
        seeds = [EXPANSION_SEED, config.next_expansion_id.to_le_bytes().as_ref()], bump
    )]
    pub expansion: Account<'info, Expansion>,
    #[account(
        init, payer = admin, space = CardTable::HEADER_LEN,
        seeds = [CARDS_SEED, config.next_expansion_id.to_le_bytes().as_ref()], bump
    )]
    pub card_table: Account<'info, CardTable>,
    pub system_program: Program<'info, System>,
}

/// Creates the next expansion, paused, with an empty card table. Price in raw $BOOTY units.
pub fn handle_add_expansion(ctx: Context<AddExpansion>, params: ExpansionParams, price: u64) -> Result<()> {
    validate(&params)?;
    let id = ctx.accounts.config.next_expansion_id;
    let e = &mut ctx.accounts.expansion;
    e.id = id;
    e.paused = true;
    e.price = price;
    e.discount_price = 0;
    e.discount_start = 0;
    e.discount_end = 0;
    e.packs_sold = 0;
    e.cards_minted = 0;
    e.bump = ctx.bumps.expansion;
    apply(e, params);
    let t = &mut ctx.accounts.card_table;
    t.expansion_id = id;
    t.bump = ctx.bumps.card_table;
    t.count = 0;
    ctx.accounts.config.next_expansion_id = id.checked_add(1).ok_or(ErrorCode::Overflow)?;
    Ok(())
}

#[derive(Accounts)]
#[instruction(expansion_id: u16)]
pub struct ExpansionAdmin<'info> {
    pub admin: Signer<'info>,
    #[account(seeds = [CONFIG_SEED], bump = config.bump, has_one = admin @ ErrorCode::Unauthorized)]
    pub config: Account<'info, Config>,
    #[account(mut, seeds = [EXPANSION_SEED, expansion_id.to_le_bytes().as_ref()], bump = expansion.bump)]
    pub expansion: Account<'info, Expansion>,
}

pub fn handle_update_expansion(ctx: Context<ExpansionAdmin>, _expansion_id: u16, params: ExpansionParams) -> Result<()> {
    validate(&params)?;
    let e = &ctx.accounts.expansion;
    // Cards already minted belong to the current collection; moving the expansion to another one would split it.
    require!(params.collection_mint == e.collection_mint || e.cards_minted == 0, ErrorCode::CollectionLocked);
    // Paid packs are minted with this metadata; it may not change under them. (Weights and pack size are frozen in
    // each PendingPack, so those may change.)
    if ctx.accounts.config.pending_packs > 0 {
        require!(
            params.collection_mint == e.collection_mint
                && params.creator == e.creator
                && params.royalty_bps == e.royalty_bps
                && params.symbol == e.symbol
                && params.uri_prefix == e.uri_prefix,
            ErrorCode::LockedWhilePending
        );
    }
    apply(&mut ctx.accounts.expansion, params);
    Ok(())
}

/// Normal price and the single discount period (discount_start = discount_end = 0 means none).
pub fn handle_set_price(
    ctx: Context<ExpansionAdmin>,
    _expansion_id: u16,
    price: u64,
    discount_price: u64,
    discount_start: i64,
    discount_end: i64,
) -> Result<()> {
    let none = discount_start == 0 && discount_end == 0;
    require!(none || discount_end > discount_start, ErrorCode::BadDiscount);
    require!(none || discount_price > 0, ErrorCode::ZeroDiscount);
    let e = &mut ctx.accounts.expansion;
    e.price = price;
    e.discount_price = if none { 0 } else { discount_price };
    e.discount_start = discount_start;
    e.discount_end = discount_end;
    Ok(())
}

pub fn handle_set_expansion_paused(ctx: Context<ExpansionAdmin>, _expansion_id: u16, paused: bool) -> Result<()> {
    ctx.accounts.expansion.paused = paused;
    Ok(())
}

#[derive(AnchorSerialize, AnchorDeserialize, Clone, Debug)]
pub struct CardInput {
    pub rarity: u8,
    pub name: String,
    pub uri_suffix: String,
}

#[derive(Accounts)]
#[instruction(expansion_id: u16)]
pub struct UpsertCards<'info> {
    #[account(mut)]
    pub admin: Signer<'info>,
    #[account(seeds = [CONFIG_SEED], bump = config.bump, has_one = admin @ ErrorCode::Unauthorized)]
    pub config: Account<'info, Config>,
    #[account(seeds = [EXPANSION_SEED, expansion_id.to_le_bytes().as_ref()], bump = expansion.bump)]
    pub expansion: Account<'info, Expansion>,
    #[account(mut, seeds = [CARDS_SEED, expansion_id.to_le_bytes().as_ref()], bump = card_table.bump)]
    pub card_table: Account<'info, CardTable>,
    pub system_program: Program<'info, System>,
}

/// Writes `cards` starting at card id `start_index`: replaces existing entries and appends past the end. Card ids are
/// the positions in the table and must stay stable, so cards are never removed.
pub fn handle_upsert_cards(ctx: Context<UpsertCards>, _expansion_id: u16, start_index: u16, cards: Vec<CardInput>) -> Result<()> {
    let prefix_len = ctx.accounts.expansion.uri_prefix.len();
    let start = start_index as usize;
    let old_count = ctx.accounts.card_table.count as usize;
    require!(start <= old_count, ErrorCode::BadCardIndex);
    require!(start + cards.len() <= MAX_CARDS, ErrorCode::TooManyCards);
    // The cards of a paid pack are public before it is opened; editing an existing entry (name or URI) then would
    // let the admin choose what a buyer receives. Appending is always allowed (pending packs ignore new cards).
    require!(start >= old_count || ctx.accounts.config.pending_packs == 0, ErrorCode::LockedWhilePending);
    for c in &cards {
        require!((c.rarity as usize) < RARITY_COUNT, ErrorCode::BadRarity);
        require!(!c.name.is_empty() && c.name.len() <= MAX_NAME_LEN, ErrorCode::TextTooLong);
        require!(c.uri_suffix.len() <= MAX_URI_SUFFIX_LEN && prefix_len + c.uri_suffix.len() <= MAX_URI_LEN, ErrorCode::TextTooLong);
    }
    // A card's rarity is fixed once it is in the table: pending packs drew their rarity counts at purchase.
    {
        let info = ctx.accounts.card_table.to_account_info();
        let data = info.try_borrow_data()?;
        for (i, c) in cards.iter().enumerate() {
            if start + i < old_count {
                let old = crate::state::card_at(&data, start + i).ok_or(ErrorCode::BadCardId)?;
                require!(old.rarity == c.rarity, ErrorCode::RarityLocked);
            }
        }
    }
    let new_count = old_count.max(start + cards.len());
    let new_len = CardTable::len_for(new_count);
    let info = ctx.accounts.card_table.to_account_info();
    if new_len > info.data_len() {
        let needed = Rent::get()?.minimum_balance(new_len).saturating_sub(info.lamports());
        if needed > 0 {
            transfer(
                CpiContext::new(
                    ctx.accounts.system_program.key(),
                    Transfer { from: ctx.accounts.admin.to_account_info(), to: info.clone() },
                ),
                needed,
            )?;
        }
        info.resize(new_len)?;
    }
    {
        let mut data = info.try_borrow_mut_data()?;
        for (i, c) in cards.iter().enumerate() {
            write_card(&mut data, start + i, c.rarity, c.name.as_bytes(), c.uri_suffix.as_bytes());
        }
    }
    // Card ids are positions in the table and must stay stable, so cards are replaced or appended, never removed.
    ctx.accounts.card_table.count = new_count as u16;
    Ok(())
}
