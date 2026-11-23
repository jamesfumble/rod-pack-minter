use anchor_lang::prelude::*;

use crate::constants::*;

/// Global settings. PDA ["config"].
#[account]
#[derive(InitSpace)]
pub struct Config {
    /// Signs every admin instruction (James's admin key).
    pub admin: Pubkey,
    /// Set by `propose_admin`; becomes admin when it signs `accept_admin`. Default = none.
    pub pending_admin: Pubkey,
    /// Backend key that must co-sign every `buy_pack`.
    pub approver: Pubkey,
    /// Backend key allowed to call `create_tree` (besides the admin). It only spends its own SOL.
    pub tree_funder: Pubkey,
    /// Stops `buy_pack` and `admin_mint` for every expansion.
    pub paused: bool,
    pub booty_mint: Pubkey,
    /// The shop wallet's $BOOTY token account.
    pub booty_destination: Pubkey,
    /// Tree every mint goes into. Default = none yet.
    pub active_tree: Pubkey,
    /// Spare tree, promoted by `rotate_tree` when the active tree is nearly full. Default = none.
    pub standby_tree: Pubkey,
    pub tree_max_depth: u32,
    pub tree_max_buffer: u32,
    pub tree_canopy_depth: u32,
    pub trees_created: u32,
    pub next_expansion_id: u16,
    pub packs_sold: u64,
    pub cards_minted: u64,
    pub bump: u8,
    pub mint_authority_bump: u8,
}

/// One card expansion (First Crush = 1). PDA ["expansion", id as u16 LE].
#[account]
#[derive(InitSpace)]
pub struct Expansion {
    pub id: u16,
    /// Stops `buy_pack` and `admin_mint` for this expansion. New expansions start paused.
    pub paused: bool,
    #[max_len(MAX_EXPANSION_NAME_LEN)]
    pub name: String,
    /// Token Metadata collection the cards join (First Crush: CARDZ...).
    pub collection_mint: Pubkey,
    /// Royalty creator (unverified, 100% share).
    pub creator: Pubkey,
    pub royalty_bps: u16,
    #[max_len(MAX_SYMBOL_LEN)]
    pub symbol: String,
    /// Shared start of every card URI; each card stores the rest.
    #[max_len(MAX_URI_PREFIX_LEN)]
    pub uri_prefix: String,
    pub pack_size: u8,
    /// Rarity weights for every slot except the last.
    pub weights_regular: [u16; RARITY_COUNT],
    /// Rarity weights for the last slot (the guarantee, e.g. Rare or better).
    pub weights_guaranteed: [u16; RARITY_COUNT],
    /// Normal price in raw $BOOTY units.
    pub price: u64,
    /// One discount period at a time: `discount_price` applies when discount_start <= now < discount_end.
    pub discount_price: u64,
    pub discount_start: i64,
    pub discount_end: i64,
    /// Sale period for `buy_pack`; 0 = no limit on that side.
    pub sale_start: i64,
    pub sale_end: i64,
    pub packs_sold: u64,
    pub cards_minted: u64,
    pub bump: u8,
}

impl Expansion {
    pub fn current_price(&self, now: i64) -> u64 {
        if self.discount_end > self.discount_start && now >= self.discount_start && now < self.discount_end {
            self.discount_price
        } else {
            self.price
        }
    }

    pub fn on_sale(&self, now: i64) -> bool {
        (self.sale_start == 0 || now >= self.sale_start) && (self.sale_end == 0 || now < self.sale_end)
    }
}

#[derive(AnchorSerialize, AnchorDeserialize, Clone, Debug, PartialEq, Eq)]
pub struct CardEntry {
    pub rarity: u8,
    pub name: String,
    pub uri_suffix: String,
}

impl CardEntry {
    pub fn serialized_len(&self) -> usize {
        1 + 4 + self.name.len() + 4 + self.uri_suffix.len()
    }
}

/// The cards of one expansion. PDA ["cards", expansion id as u16 LE]. Grows with `upsert_cards`.
#[account]
pub struct CardTable {
    pub expansion_id: u16,
    pub bump: u8,
    pub cards: Vec<CardEntry>,
}

impl CardTable {
    pub const BASE_LEN: usize = 8 + 2 + 1 + 4;

    pub fn serialized_len(&self) -> usize {
        Self::BASE_LEN + self.cards.iter().map(|c| c.serialized_len()).sum::<usize>()
    }

    pub fn rarity_counts(&self) -> [u16; RARITY_COUNT] {
        let mut counts = [0u16; RARITY_COUNT];
        for c in &self.cards {
            counts[c.rarity as usize] += 1;
        }
        counts
    }

    /// Card id of the `index`-th card (0-based) with the given rarity.
    pub fn nth_of_rarity(&self, rarity: u8, index: u16) -> Option<u16> {
        let mut seen = 0u16;
        for (id, c) in self.cards.iter().enumerate() {
            if c.rarity == rarity {
                if seen == index {
                    return Some(id as u16);
                }
                seen += 1;
            }
        }
        None
    }
}

#[event]
pub struct PackMinted {
    pub expansion_id: u16,
    pub order_id: [u8; 16],
    pub player: Pubkey,
    pub tree: Pubkey,
    /// Leaf index of the first card; card i has leaf index first_leaf_index + i.
    pub first_leaf_index: u64,
    pub card_ids: Vec<u16>,
    pub price: u64,
    pub seed: [u8; 32],
}

#[event]
pub struct AdminMinted {
    pub expansion_id: u16,
    pub card_id: u16,
    pub recipient: Pubkey,
    pub tree: Pubkey,
    pub first_leaf_index: u64,
    pub count: u8,
}

#[event]
pub struct TreeCreated {
    pub tree: Pubkey,
    pub as_active: bool,
    pub max_depth: u32,
    pub max_buffer: u32,
    pub canopy_depth: u32,
}

#[event]
pub struct TreeRotated {
    pub old_tree: Pubkey,
    pub new_tree: Pubkey,
}
