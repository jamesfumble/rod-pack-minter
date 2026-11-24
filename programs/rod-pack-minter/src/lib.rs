//! rod_pack_minter: sells card packs for Reign of Desire expansions as Bubblegum v1 cNFTs.
//! A purchase has two steps. `buy_pack` (player + backend approver) moves the $BOOTY price into the program's vault.
//! `open_pack` (anyone, normally the backend a moment later) draws the cards from the purchase slot's hash, mints them,
//! and releases the price to the shop. A pack nobody opened in time (an outage) gets a fresh slot via `redraw_pack`.
//! The player pays every fee: the opener is repaid from the player's deposit. Only `admin_mint` can choose a card.

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

declare_id!("RoD7T8KRzHoTPnbS15uibfKAaqV5bYzcVDW8X9zWmZN");

#[program]
pub mod rod_pack_minter {
    use super::*;

    pub fn initialize(ctx: Context<Initialize>, params: InitParams) -> Result<()> {
        instructions::initialize::handle_initialize(ctx, params)
    }
    pub fn migrate_config(ctx: Context<MigrateConfig>, open_fee_lamports: u64, retry_fee_lamports: u64, paid_retries: u8, refund_after_secs: i64) -> Result<()> {
        instructions::admin::handle_migrate_config(ctx, open_fee_lamports, retry_fee_lamports, paid_retries, refund_after_secs)
    }
    pub fn init_vault(ctx: Context<InitVault>) -> Result<()> {
        instructions::admin::handle_init_vault(ctx)
    }
    pub fn set_destination(ctx: Context<SetDestination>) -> Result<()> {
        instructions::admin::handle_set_destination(ctx)
    }
    pub fn set_fees(ctx: Context<AdminOnly>, open_fee_lamports: u64, retry_fee_lamports: u64, paid_retries: u8, refund_after_secs: i64) -> Result<()> {
        instructions::admin::handle_set_fees(ctx, open_fee_lamports, retry_fee_lamports, paid_retries, refund_after_secs)
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

    // Purchases
    pub fn buy_pack(ctx: Context<BuyPack>, expansion_id: u16, order_id: [u8; 16], max_price: u64) -> Result<()> {
        instructions::buy_pack::handle_buy_pack(ctx, expansion_id, order_id, max_price)
    }
    pub fn open_pack(ctx: Context<OpenPack>) -> Result<()> {
        instructions::open_pack::handle_open_pack(ctx)
    }
    pub fn seal_pack(ctx: Context<SealPack>) -> Result<()> {
        instructions::seal::handle_seal_pack(ctx)
    }
    pub fn redraw_pack(ctx: Context<RedrawPack>) -> Result<()> {
        instructions::redraw::handle_redraw_pack(ctx)
    }
    pub fn refund_pack(ctx: Context<RefundPack>) -> Result<()> {
        instructions::refund::handle_refund_pack(ctx)
    }
    pub fn admin_mint(ctx: Context<AdminMint>, expansion_id: u16, card_id: u16, count: u8) -> Result<()> {
        instructions::admin_mint::handle_admin_mint(ctx, expansion_id, card_id, count)
    }
}
