//! rod_pack_minter: sells card packs for Reign of Desire expansions. One `buy_pack` transaction takes the $BOOTY price
//! and mints the whole pack as Bubblegum v1 cNFTs into the expansion's collection; if any step fails, nothing happens.
//! Cards are drawn on chain when the transaction runs. Only `admin_mint` (admin-signed) can choose a card.

pub mod bubblegum;
pub mod constants;
pub mod draw;
pub mod error;
pub mod instructions;
pub mod state;

use anchor_lang::prelude::*;

pub use constants::*;
pub use instructions::*;
pub use state::*;

declare_id!("Br5i2LnUyYwtCFm1NkjPNYWpAeHaRtEwMUjMiaupu6Uw");

#[program]
pub mod rod_pack_minter {
    use super::*;

    pub fn initialize(
        ctx: Context<Initialize>,
        approver: Pubkey,
        tree_funder: Pubkey,
        tree_max_depth: u32,
        tree_max_buffer: u32,
        tree_canopy_depth: u32,
    ) -> Result<()> {
        instructions::initialize::handle_initialize(ctx, approver, tree_funder, tree_max_depth, tree_max_buffer, tree_canopy_depth)
    }

    // Admin: whole program
    pub fn set_paused(ctx: Context<AdminOnly>, paused: bool) -> Result<()> {
        instructions::admin::handle_set_paused(ctx, paused)
    }
    pub fn set_approver(ctx: Context<AdminOnly>, approver: Pubkey) -> Result<()> {
        instructions::admin::handle_set_approver(ctx, approver)
    }
    pub fn set_tree_funder(ctx: Context<AdminOnly>, tree_funder: Pubkey) -> Result<()> {
        instructions::admin::handle_set_tree_funder(ctx, tree_funder)
    }
    pub fn set_tree_size(ctx: Context<AdminOnly>, max_depth: u32, max_buffer: u32, canopy_depth: u32) -> Result<()> {
        instructions::admin::handle_set_tree_size(ctx, max_depth, max_buffer, canopy_depth)
    }
    pub fn propose_admin(ctx: Context<AdminOnly>, new_admin: Pubkey) -> Result<()> {
        instructions::admin::handle_propose_admin(ctx, new_admin)
    }
    pub fn accept_admin(ctx: Context<AcceptAdmin>) -> Result<()> {
        instructions::admin::handle_accept_admin(ctx)
    }
    pub fn set_payment(ctx: Context<SetPayment>) -> Result<()> {
        instructions::admin::handle_set_payment(ctx)
    }

    // Admin: expansions
    pub fn add_expansion(ctx: Context<AddExpansion>, params: ExpansionParams, price: u64) -> Result<()> {
        instructions::expansion::handle_add_expansion(ctx, params, price)
    }
    pub fn update_expansion(ctx: Context<ExpansionAdmin>, expansion_id: u16, params: ExpansionParams) -> Result<()> {
        instructions::expansion::handle_update_expansion(ctx, expansion_id, params)
    }
    pub fn set_price(
        ctx: Context<ExpansionAdmin>,
        expansion_id: u16,
        price: u64,
        discount_price: u64,
        discount_start: i64,
        discount_end: i64,
    ) -> Result<()> {
        instructions::expansion::handle_set_price(ctx, expansion_id, price, discount_price, discount_start, discount_end)
    }
    pub fn set_expansion_paused(ctx: Context<ExpansionAdmin>, expansion_id: u16, paused: bool) -> Result<()> {
        instructions::expansion::handle_set_expansion_paused(ctx, expansion_id, paused)
    }
    pub fn upsert_cards(ctx: Context<UpsertCards>, expansion_id: u16, start_index: u16, cards: Vec<CardInput>) -> Result<()> {
        instructions::expansion::handle_upsert_cards(ctx, expansion_id, start_index, cards)
    }

    // Trees
    pub fn create_tree(ctx: Context<CreateTree>) -> Result<()> {
        instructions::tree::handle_create_tree(ctx)
    }
    pub fn rotate_tree(ctx: Context<RotateTree>) -> Result<()> {
        instructions::tree::handle_rotate_tree(ctx)
    }

    // Minting
    pub fn buy_pack(ctx: Context<BuyPack>, expansion_id: u16, order_id: [u8; 16], max_price: u64) -> Result<()> {
        instructions::buy_pack::handle_buy_pack(ctx, expansion_id, order_id, max_price)
    }
    pub fn admin_mint(ctx: Context<AdminMint>, expansion_id: u16, card_id: u16, count: u8) -> Result<()> {
        instructions::admin_mint::handle_admin_mint(ctx, expansion_id, card_id, count)
    }
}
