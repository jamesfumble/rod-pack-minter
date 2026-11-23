use anchor_lang::prelude::*;
use anchor_lang::system_program::{transfer, Transfer};
use anchor_spl::token_interface::{transfer_checked, Mint, TokenAccount, TokenInterface, TransferChecked};

use crate::{
    constants::*,
    error::ErrorCode,
    state::{CardTable, Config, Expansion, PackBought, PendingPack},
};

/// Step 1 of a purchase: the price moves into the program's vault and a PendingPack records the purchase slot.
/// No card is drawn here. The cards come from the hash of the purchase slot, which only exists once this
/// transaction's slot is complete, when the purchase can no longer be undone, so nobody (player, backend or opener)
/// can see the cards before paying or choose them afterwards.
#[event_cpi]
#[derive(Accounts)]
#[instruction(expansion_id: u16, order_id: [u8; 16])]
pub struct BuyPack<'info> {
    /// Pays the price, the transaction fee, the PendingPack rent (returned when the pack is opened or refunded) and
    /// the opener's fee deposit.
    #[account(mut)]
    pub player: Signer<'info>,
    /// The backend's approver key: every purchase goes through the game's order flow.
    pub approver: Signer<'info>,
    #[account(mut, seeds = [CONFIG_SEED], bump = config.bump)]
    pub config: Box<Account<'info, Config>>,
    #[account(seeds = [EXPANSION_SEED, expansion_id.to_le_bytes().as_ref()], bump = expansion.bump)]
    pub expansion: Box<Account<'info, Expansion>>,
    #[account(seeds = [CARDS_SEED, expansion_id.to_le_bytes().as_ref()], bump = card_table.bump)]
    pub card_table: Box<Account<'info, CardTable>>,
    #[account(
        init, payer = player, space = 8 + PendingPack::INIT_SPACE,
        seeds = [PACK_SEED, player.key().as_ref(), order_id.as_ref()], bump
    )]
    pub pending_pack: Box<Account<'info, PendingPack>>,
    /// The player's $BOOTY account (transfer_checked enforces owner and mint).
    #[account(mut)]
    pub player_token: Box<InterfaceAccount<'info, TokenAccount>>,
    #[account(mut, address = config.vault @ ErrorCode::WrongVault)]
    pub vault: Box<InterfaceAccount<'info, TokenAccount>>,
    #[account(address = config.booty_mint @ ErrorCode::WrongPaymentAccount)]
    pub booty_mint: Box<InterfaceAccount<'info, Mint>>,
    pub token_program: Interface<'info, TokenInterface>,
    pub system_program: Program<'info, System>,
}

pub fn handle_buy_pack(ctx: Context<BuyPack>, expansion_id: u16, order_id: [u8; 16], max_price: u64) -> Result<()> {
    let config = &ctx.accounts.config;
    let expansion = &ctx.accounts.expansion;
    require!(!config.paused, ErrorCode::ProgramPaused);
    require!(!expansion.paused, ErrorCode::ExpansionPaused);
    require_keys_eq!(ctx.accounts.approver.key(), config.approver, ErrorCode::WrongApprover);
    require!(config.vault != Pubkey::default(), ErrorCode::WrongVault);
    let clock = Clock::get()?;
    require!(expansion.on_sale(clock.unix_timestamp), ErrorCode::OutsideSalePeriod);
    require!(ctx.accounts.card_table.count > 0, ErrorCode::NoCards);
    let price = expansion.current_price(clock.unix_timestamp);
    require!(price <= max_price, ErrorCode::PriceAboveMax);

    if price > 0 {
        transfer_checked(
            CpiContext::new(
                ctx.accounts.token_program.key(),
                TransferChecked {
                    from: ctx.accounts.player_token.to_account_info(),
                    mint: ctx.accounts.booty_mint.to_account_info(),
                    to: ctx.accounts.vault.to_account_info(),
                    authority: ctx.accounts.player.to_account_info(),
                },
            ),
            price,
            ctx.accounts.booty_mint.decimals,
        )?;
    }

    // The opener's (and, if needed, the sealer's) fee, paid by the player now and handed out by the program later.
    let (open_fee, seal_fee) = (config.open_fee_lamports, config.seal_fee_lamports);
    let deposit = open_fee.checked_add(seal_fee).ok_or(ErrorCode::Overflow)?;
    if deposit > 0 {
        transfer(
            CpiContext::new(
                ctx.accounts.system_program.key(),
                Transfer { from: ctx.accounts.player.to_account_info(), to: ctx.accounts.pending_pack.to_account_info() },
            ),
            deposit,
        )?;
    }

    let pack_size = expansion.pack_size;
    let p = &mut ctx.accounts.pending_pack;
    p.player = ctx.accounts.player.key();
    p.expansion_id = expansion_id;
    p.order_id = order_id;
    p.price = price;
    p.pack_size = pack_size;
    p.purchase_slot = clock.slot;
    p.purchased_at = clock.unix_timestamp;
    p.sealed = false;
    p.slot_hash = [0u8; 32];
    p.open_fee = open_fee;
    p.seal_fee = seal_fee;
    p.bump = ctx.bumps.pending_pack;

    let config = &mut ctx.accounts.config;
    config.pending_packs = config.pending_packs.checked_add(1).ok_or(ErrorCode::Overflow)?;

    emit_cpi!(PackBought {
        expansion_id,
        order_id,
        player: ctx.accounts.player.key(),
        pending_pack: ctx.accounts.pending_pack.key(),
        price,
        purchase_slot: clock.slot,
    });
    Ok(())
}
