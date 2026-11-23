use anchor_lang::prelude::*;

use crate::{
    bubblegum::{metadata_args, MintCpi, TreeConfigView},
    constants::*,
    error::ErrorCode,
    state::{card_at, CardTable, Config, Expansion},
};

/// The accounts every mint needs. Shared by `buy_pack` and `admin_mint`.
#[derive(Accounts)]
pub struct MintAccounts<'info> {
    /// CHECK: the mint authority PDA (checked in `prepare`).
    pub mint_authority: UncheckedAccount<'info>,
    /// CHECK: Bubblegum tree config of `merkle_tree` (checked in `prepare`).
    #[account(mut)]
    pub tree_config: UncheckedAccount<'info>,
    /// CHECK: must be the active tree (checked in `prepare`).
    #[account(mut)]
    pub merkle_tree: UncheckedAccount<'info>,
    /// CHECK: must be the expansion's collection (checked in `prepare`); Bubblegum checks the rest.
    pub collection_mint: UncheckedAccount<'info>,
    /// CHECK: Bubblegum checks it against the collection mint.
    #[account(mut)]
    pub collection_metadata: UncheckedAccount<'info>,
    /// CHECK: Bubblegum checks it.
    pub collection_edition: UncheckedAccount<'info>,
    /// CHECK: Token Metadata collection-authority record of the mint authority; Bubblegum checks it.
    pub collection_authority_record: UncheckedAccount<'info>,
    /// CHECK: Bubblegum's "collection_cpi" PDA; Bubblegum checks it.
    pub bubblegum_signer: UncheckedAccount<'info>,
    /// CHECK: address
    #[account(address = NOOP_ID)]
    pub log_wrapper: UncheckedAccount<'info>,
    /// CHECK: address
    #[account(address = ACCOUNT_COMPRESSION_ID)]
    pub compression_program: UncheckedAccount<'info>,
    /// CHECK: address
    #[account(address = TOKEN_METADATA_ID)]
    pub token_metadata_program: UncheckedAccount<'info>,
    /// CHECK: address
    #[account(address = BUBBLEGUM_ID)]
    pub bubblegum_program: UncheckedAccount<'info>,
    pub system_program: Program<'info, System>,
}

impl<'info> MintAccounts<'info> {
    /// Checks the mint accounts against the config and expansion, and that the active tree has room for `count`.
    pub fn prepare(&self, config: &Config, expansion: &Expansion, count: u64) -> Result<TreeConfigView> {
        let authority = Pubkey::create_program_address(&[MINT_AUTHORITY_SEED, &[config.mint_authority_bump]], &crate::ID)
            .map_err(|_| error!(ErrorCode::Unauthorized))?;
        require_keys_eq!(self.mint_authority.key(), authority, ErrorCode::Unauthorized);
        require!(config.active_tree != Pubkey::default(), ErrorCode::NoActiveTree);
        require_keys_eq!(self.merkle_tree.key(), config.active_tree, ErrorCode::NotActiveTree);
        require_keys_eq!(self.collection_mint.key(), expansion.collection_mint, ErrorCode::WrongCollection);
        let view = TreeConfigView::load(&self.tree_config.to_account_info(), &config.active_tree)?;
        require_keys_eq!(view.tree_delegate, authority, ErrorCode::BadTreeConfig);
        require!(view.free_leaves() >= count, ErrorCode::TreeFull);
        Ok(view)
    }

    /// Mints the given cards (ids into the card table) to `leaf_owner`, one Bubblegum CPI each.
    pub fn mint_cards(
        &self,
        config: &Config,
        expansion: &Expansion,
        table: &Account<'info, CardTable>,
        leaf_owner: &AccountInfo<'info>,
        payer: &AccountInfo<'info>,
        card_ids: &[u16],
    ) -> Result<()> {
        let tree_config = self.tree_config.to_account_info();
        let merkle_tree = self.merkle_tree.to_account_info();
        let mint_authority = self.mint_authority.to_account_info();
        let record = self.collection_authority_record.to_account_info();
        let collection_mint = self.collection_mint.to_account_info();
        let collection_metadata = self.collection_metadata.to_account_info();
        let collection_edition = self.collection_edition.to_account_info();
        let bubblegum_signer = self.bubblegum_signer.to_account_info();
        let log_wrapper = self.log_wrapper.to_account_info();
        let compression_program = self.compression_program.to_account_info();
        let token_metadata_program = self.token_metadata_program.to_account_info();
        let system_program = self.system_program.to_account_info();
        let bubblegum_program = self.bubblegum_program.to_account_info();
        let cpi = MintCpi {
            tree_config: &tree_config,
            leaf_owner,
            merkle_tree: &merkle_tree,
            payer,
            mint_authority: &mint_authority,
            collection_authority_record: &record,
            collection_mint: &collection_mint,
            collection_metadata: &collection_metadata,
            collection_edition: &collection_edition,
            bubblegum_signer: &bubblegum_signer,
            log_wrapper: &log_wrapper,
            compression_program: &compression_program,
            token_metadata_program: &token_metadata_program,
            system_program: &system_program,
            bubblegum_program: &bubblegum_program,
        };
        let bump = [config.mint_authority_bump];
        let seeds: &[&[u8]] = &[MINT_AUTHORITY_SEED, &bump];
        let mut uri = String::with_capacity(MAX_URI_LEN);
        let mut name = String::with_capacity(MAX_NAME_LEN);
        for id in card_ids {
            require!(*id < table.count, ErrorCode::BadCardId);
            {
                let info = table.to_account_info();
                let data = info.try_borrow_data()?;
                let card = card_at(&data, *id as usize).ok_or(ErrorCode::BadCardId)?;
                name.clear();
                name.push_str(core::str::from_utf8(card.name).map_err(|_| error!(ErrorCode::TextTooLong))?);
                uri.clear();
                uri.push_str(&expansion.uri_prefix);
                uri.push_str(core::str::from_utf8(card.uri_suffix).map_err(|_| error!(ErrorCode::TextTooLong))?);
            }
            let metadata = metadata_args(&name, &expansion.symbol, &uri, expansion.royalty_bps, &expansion.collection_mint, &expansion.creator);
            cpi.mint(&metadata, &[seeds])?;
        }
        Ok(())
    }
}
