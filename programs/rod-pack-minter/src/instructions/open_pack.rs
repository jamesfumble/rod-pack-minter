use anchor_lang::prelude::*;
use anchor_spl::token_interface::{transfer_checked, Mint, TokenAccount, TokenInterface, TransferChecked};

use crate::instructions::mint::*;
use crate::instructions::seal::{pay_from_pending, seal};
use crate::{
    constants::*,
    draw::{draw_pack, pack_seed},
    error::ErrorCode,
    state::{nth_of_rarity, CardTable, Config, Expansion, PackMinted, PackSealed, PendingPack},
};

/// Step 2 of a purchase. Anyone may send it (normally the backend, right after the purchase; the game client as a
/// fallback); the result does not depend on who sends it or when. It seals the pack if needed, draws the cards from
/// the purchase slot's hash, mints them to the player, moves the price from the vault to the shop, pays the opener the
/// player's fee deposit and closes the PendingPack (its rent goes back to the player). Works while paused, because the
/// pack is already paid.
#[event_cpi]
#[derive(Accounts)]
pub struct OpenPack<'info> {
    /// Sends the transaction and receives the player's fee deposit. Also the Bubblegum payer (which pays nothing).
    #[account(mut)]
    pub opener: Signer<'info>,
    #[account(mut, seeds = [CONFIG_SEED], bump = config.bump)]
    pub config: Box<Account<'info, Config>>,
    #[account(
        mut, close = player,
        seeds = [PACK_SEED, pending_pack.player.as_ref(), pending_pack.order_id.as_ref()],
        bump = pending_pack.bump
    )]
    pub pending_pack: Box<Account<'info, PendingPack>>,
    /// Receives the cards and the PendingPack's rent.
    /// CHECK: must be the pack's player.
    #[account(mut, address = pending_pack.player @ ErrorCode::WrongPlayer)]
    pub player: UncheckedAccount<'info>,
    #[account(mut, seeds = [EXPANSION_SEED, pending_pack.expansion_id.to_le_bytes().as_ref()], bump = expansion.bump)]
    pub expansion: Box<Account<'info, Expansion>>,
    #[account(seeds = [CARDS_SEED, pending_pack.expansion_id.to_le_bytes().as_ref()], bump = card_table.bump)]
    pub card_table: Box<Account<'info, CardTable>>,
    #[account(mut, address = config.vault @ ErrorCode::WrongVault)]
    pub vault: Box<InterfaceAccount<'info, TokenAccount>>,
    #[account(mut, address = config.booty_destination @ ErrorCode::WrongPaymentAccount)]
    pub booty_destination: Box<InterfaceAccount<'info, TokenAccount>>,
    #[account(address = config.booty_mint @ ErrorCode::WrongPaymentAccount)]
    pub booty_mint: Box<InterfaceAccount<'info, Mint>>,
    pub token_program: Interface<'info, TokenInterface>,
    pub mint: MintAccounts<'info>,
    /// CHECK: address checked in `lookup_slot_hash`.
    pub slot_hashes: UncheckedAccount<'info>,
}

pub fn handle_open_pack(ctx: Context<OpenPack>) -> Result<()> {
    let pending_key = ctx.accounts.pending_pack.key();
    if !ctx.accounts.pending_pack.sealed {
        seal(&mut ctx.accounts.pending_pack, &ctx.accounts.slot_hashes.to_account_info())?;
        emit_cpi!(PackSealed { pending_pack: pending_key, slot_hash: ctx.accounts.pending_pack.slot_hash });
    }

    let pending = &ctx.accounts.pending_pack;
    let config = &ctx.accounts.config;
    let expansion = &ctx.accounts.expansion;
    let table = &ctx.accounts.card_table;
    let pack_size = pending.pack_size;
    let view = ctx.accounts.mint.prepare(config, expansion, pack_size as u64)?;

    let seed = pack_seed(&pending.slot_hash, &pending.order_id, &pending.player.to_bytes(), pending.purchase_slot, pending.expansion_id);
    let mut card_ids = Vec::with_capacity(pack_size as usize);
    {
        let info = table.to_account_info();
        let data = info.try_borrow_data()?;
        // Frozen at purchase: later edits to the expansion do not change a paid pack.
        let picks = draw_pack(&seed, pack_size, &pending.weights_regular, &pending.weights_guaranteed, &pending.rarity_counts).ok_or(ErrorCode::BadWeights)?;
        for (rarity, index) in picks {
            card_ids.push(nth_of_rarity(&data, table.count, rarity, index).ok_or(ErrorCode::BadCardId)?);
        }
    }

    let player = ctx.accounts.player.to_account_info();
    let opener = ctx.accounts.opener.to_account_info();
    ctx.accounts.mint.mint_cards(config, expansion, table, &player, &opener, &card_ids)?;

    // The price leaves the vault for the shop.
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
                    to: ctx.accounts.booty_destination.to_account_info(),
                    authority: ctx.accounts.config.to_account_info(),
                },
                &[seeds],
            ),
            price,
            ctx.accounts.booty_mint.decimals,
        )?;
    }

    // The opener gets the player's remaining fee deposit; `close = player` then returns the rent to the player.
    let fees = pending.open_fee.checked_add(pending.seal_fee).ok_or(ErrorCode::Overflow)?;
    let (expansion_id, order_id, player_key, tree, first_leaf) =
        (pending.expansion_id, pending.order_id, pending.player, config.active_tree, view.num_minted);
    pay_from_pending(&ctx.accounts.pending_pack.to_account_info(), &opener, fees)?;

    let n = card_ids.len() as u64;
    let config = &mut ctx.accounts.config;
    config.pending_packs = config.pending_packs.checked_sub(1).ok_or(ErrorCode::Overflow)?;
    config.reserved_leaves = config.reserved_leaves.saturating_sub(pack_size as u64);
    config.packs_sold = config.packs_sold.checked_add(1).ok_or(ErrorCode::Overflow)?;
    config.cards_minted = config.cards_minted.checked_add(n).ok_or(ErrorCode::Overflow)?;
    let expansion = &mut ctx.accounts.expansion;
    expansion.packs_sold = expansion.packs_sold.checked_add(1).ok_or(ErrorCode::Overflow)?;
    expansion.cards_minted = expansion.cards_minted.checked_add(n).ok_or(ErrorCode::Overflow)?;

    emit_cpi!(PackMinted { expansion_id, order_id, player: player_key, tree, first_leaf_index: first_leaf, card_ids, price, seed });
    Ok(())
}
