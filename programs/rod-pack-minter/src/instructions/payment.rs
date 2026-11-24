//! The two payment currencies: $BOOTY (config: mint, vault, one shop destination) and $LUST (LustConfig: mint, vault,
//! four split destinations and a burned remainder).

use anchor_lang::prelude::*;
use anchor_spl::token_interface::{burn, transfer_checked, Burn, TransferChecked};

use crate::{
    constants::*,
    error::ErrorCode,
    state::{Config, LustConfig},
};

/// The mint and vault of `currency`.
pub fn mint_and_vault(currency: u8, config: &Config, lust: Option<&LustConfig>) -> Result<(Pubkey, Pubkey)> {
    match currency {
        CURRENCY_BOOTY => {
            require!(config.vault != Pubkey::default(), ErrorCode::WrongVault);
            Ok((config.booty_mint, config.vault))
        }
        CURRENCY_LUST => {
            let l = lust.ok_or(ErrorCode::BadCurrency)?;
            require!(l.mint != Pubkey::default(), ErrorCode::BadCurrency);
            Ok((l.mint, l.vault))
        }
        _ => err!(ErrorCode::BadCurrency),
    }
}

/// Whole tokens to raw units of a mint with `decimals`.
pub fn to_raw(whole: u64, decimals: u8) -> Result<u64> {
    10u64.checked_pow(decimals as u32).and_then(|m| whole.checked_mul(m)).ok_or(error!(ErrorCode::Overflow))
}

/// Pays a pack's price out of the vault (the config PDA signs): all to the shop for $BOOTY; for $LUST the split to
/// the four destinations and the remainder burned. `destinations` are the instruction's remaining accounts, which
/// must be exactly the configured payout accounts, in order.
#[allow(clippy::too_many_arguments)]
pub fn pay_out<'info>(
    token_program: &AccountInfo<'info>,
    vault: &AccountInfo<'info>,
    mint: &AccountInfo<'info>,
    decimals: u8,
    config_info: &AccountInfo<'info>,
    config: &Config,
    lust: Option<&LustConfig>,
    currency: u8,
    destinations: &[AccountInfo<'info>],
    price: u64,
) -> Result<()> {
    if price == 0 {
        return Ok(());
    }
    let bump = [config.bump];
    let seeds: &[&[u8]] = &[CONFIG_SEED, &bump];
    let send = |to: &AccountInfo<'info>, amount: u64| -> Result<()> {
        if amount == 0 {
            return Ok(());
        }
        transfer_checked(
            CpiContext::new_with_signer(
                token_program.key(),
                TransferChecked { from: vault.clone(), mint: mint.clone(), to: to.clone(), authority: config_info.clone() },
                &[seeds],
            ),
            amount,
            decimals,
        )
    };
    match currency {
        CURRENCY_BOOTY => {
            let to = destinations.first().ok_or(ErrorCode::WrongDestination)?;
            require_keys_eq!(to.key(), config.booty_destination, ErrorCode::WrongDestination);
            send(to, price)
        }
        CURRENCY_LUST => {
            let l = lust.ok_or(ErrorCode::BadCurrency)?;
            require!(destinations.len() >= LUST_SPLITS, ErrorCode::WrongDestination);
            let mut paid = 0u64;
            for i in 0..LUST_SPLITS {
                require_keys_eq!(destinations[i].key(), l.destinations[i], ErrorCode::WrongDestination);
                let amount = ((price as u128 * l.split_bps[i] as u128) / 10_000) as u64;
                send(&destinations[i], amount)?;
                paid = paid.checked_add(amount).ok_or(ErrorCode::Overflow)?;
            }
            let rest = price.checked_sub(paid).ok_or(ErrorCode::Overflow)?;
            if rest > 0 {
                burn(
                    CpiContext::new_with_signer(
                        token_program.key(),
                        Burn { mint: mint.clone(), from: vault.clone(), authority: config_info.clone() },
                        &[seeds],
                    ),
                    rest,
                )?;
            }
            Ok(())
        }
        _ => err!(ErrorCode::BadCurrency),
    }
}
