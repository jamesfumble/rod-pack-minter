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
    /// Stops `buy_pack` and `admin_mint` for every expansion (already paid packs can still be opened).
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
    // ---- v2 (two-step purchase). Appended so `migrate_config` can extend a v1 account in place. ----
    /// Program-owned $BOOTY token account (PDA ["vault", booty_mint], authority = config) that holds a pack's price
    /// between `buy_pack` and `open_pack` / the admin's `refund_pack`.
    pub vault: Pubkey,
    /// Lamports the player deposits with every purchase; `open_pack` pays them to whoever sends it, so the opener's
    /// transaction fee is covered by the player.
    pub open_fee_lamports: u64,
    /// Lamports paid from the player's retry pool for each `seal_pack` / `redraw_pack` (only needed when opening is
    /// delayed or after an outage).
    pub retry_fee_lamports: u64,
    /// Seconds after purchase before the admin may refund an unopened pack (so a refund never races a slow opener).
    pub refund_after_secs: i64,
    /// Purchases not yet opened or refunded.
    pub pending_packs: u64,
    pub packs_refunded: u64,
    /// Tree leaves promised to pending packs; `buy_pack` only sells when active + standby trees have room for them.
    pub reserved_leaves: u64,
    /// How many seals/redraws the player's deposit pays for (the retry pool is `retry_fee_lamports * paid_retries`).
    /// The unused part of the pool goes back to the player when the pack is opened.
    pub paid_retries: u8,
    pub _reserved: [u8; 55],
}

impl Config {
    /// Size of the v1 account (before the two-step fields), for `migrate_config`.
    pub const V1_LEN: usize = 8 + 32 * 8 + 1 + 4 * 4 + 2 + 8 + 8 + 1 + 1;
}

/// A paid, not yet opened pack. PDA ["pack", player, order_id]. Closed by `open_pack` (or the admin's `refund_pack`),
/// which return its rent to the player. The draw inputs (weights and card counts) are frozen here at purchase, so
/// later admin edits to the expansion cannot change a paid pack.
#[account]
#[derive(InitSpace)]
pub struct PendingPack {
    pub player: Pubkey,
    pub expansion_id: u16,
    pub order_id: [u8; 16],
    /// Raw units of the pack's currency (`currency`) held in that currency's vault.
    pub price: u64,
    pub pack_size: u8,
    /// Slot of the `buy_pack` transaction. The cards come from the hash of this slot, which does not exist until
    /// the slot is complete (after the purchase is irreversible).
    pub purchase_slot: u64,
    pub purchased_at: i64,
    /// Set by `seal_pack` / `open_pack`: the hash the draw uses.
    pub sealed: bool,
    pub slot_hash: [u8; 32],
    /// Paid to whoever opens the pack (from the player's deposit, on top of the rent).
    pub open_fee: u64,
    /// Paid per seal/redraw while the retry pool lasts.
    pub retry_fee: u64,
    pub bump: u8,
    pub weights_regular: [u16; RARITY_COUNT],
    pub weights_guaranteed: [u16; RARITY_COUNT],
    /// Cards per rarity at purchase. Cards are only appended, so the first `rarity_counts[r]` cards of rarity r are
    /// the same set at open time.
    pub rarity_counts: [u16; RARITY_COUNT],
    /// How many times `redraw_pack` gave this pack a new purchase slot (after an outage).
    pub redraws: u8,
    /// What is left of the player's retry deposit; returned to the player when the PendingPack closes.
    pub retry_pool: u64,
    /// CURRENCY_BOOTY or CURRENCY_LUST: which vault holds `price` (raw units of that mint).
    pub currency: u8,
    /// Spare bytes, so fields can be added later without breaking packs that are pending during an upgrade.
    pub _reserved: [u8; 32],
}

/// $LUST payments. PDA ["lust"]. A $LUST price is split on opening: `split_bps[i]` to `destinations[i]` (team,
/// giveaway, stake, absorption pool), the remainder is burned (shop default 10 / 10 / 45 / 25, burn 10).
#[account]
#[derive(InitSpace)]
pub struct LustConfig {
    pub mint: Pubkey,
    /// PDA ["vault", mint], authority = config.
    pub vault: Pubkey,
    pub destinations: [Pubkey; LUST_SPLITS],
    pub split_bps: [u16; LUST_SPLITS],
    pub bump: u8,
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
    /// Rarity weights for every slot (the last one too, unless the guarantee applies).
    pub weights_regular: [u16; RARITY_COUNT],
    /// Rarity weights for the last slot when no earlier slot drew one of these rarities (the guarantee: at least one
    /// card of them, e.g. Rare or better, per pack).
    pub weights_guaranteed: [u16; RARITY_COUNT],
    /// List prices in whole tokens, mirrored from the shop catalog. 0 = not sold for that currency.
    pub price_booty: u64,
    pub price_lust: u64,
    /// One discount at a time, in whole percent (1-99), active while discount_start <= now < discount_end. It applies
    /// to both currencies, rounded like the shop catalog: round(list * (100 - percent) / 100), at least 1.
    pub discount_percent: u8,
    pub discount_start: i64,
    pub discount_end: i64,
    /// Availability period for `buy_pack` (the catalog's availability window); 0 = open on that side.
    pub sale_start: i64,
    pub sale_end: i64,
    pub packs_sold: u64,
    pub cards_minted: u64,
    pub bump: u8,
    /// Spare bytes, so fields can be added later without breaking existing expansions.
    pub _reserved: [u8; 32],
}

impl Expansion {
    /// Price in whole tokens for `currency` at `now`, or None when the pack is not sold for it. Same rule as the shop
    /// catalog's pricing (shop/pricing.js priceNow): Math.round on halves rounds up; never below 1.
    pub fn current_price(&self, currency: u8, now: i64) -> Option<u64> {
        let list = match currency {
            CURRENCY_BOOTY => self.price_booty,
            CURRENCY_LUST => self.price_lust,
            _ => return None,
        };
        if list == 0 {
            return None;
        }
        let active = self.discount_percent > 0 && now >= self.discount_start && now < self.discount_end;
        if !active {
            return Some(list);
        }
        let x = list as u128 * (100 - self.discount_percent as u128);
        Some((((x + 50) / 100) as u64).max(1))
    }

    pub fn on_sale(&self, now: i64) -> bool {
        (self.sale_start == 0 || now >= self.sale_start) && (self.sale_end == 0 || now < self.sale_end)
    }
}

/// The cards of one expansion. PDA ["cards", expansion id as u16 LE]. Only this small header is (de)serialised by
/// Anchor; the cards follow it as fixed-size entries read and written in place, so even a full table never has to
/// fit in the 32 KiB heap. Entry: rarity u8 | name_len u8 | name [u8; 32] | suffix_len u8 | uri_suffix [u8; 72].
#[account]
pub struct CardTable {
    pub expansion_id: u16,
    pub bump: u8,
    pub count: u16,
}

impl CardTable {
    pub const HEADER_LEN: usize = 8 + 2 + 1 + 2;
    pub const ENTRY_LEN: usize = 1 + 1 + MAX_NAME_LEN + 1 + MAX_URI_SUFFIX_LEN;

    pub fn len_for(count: usize) -> usize {
        Self::HEADER_LEN + count * Self::ENTRY_LEN
    }
}

/// One card, borrowed from the account data.
pub struct CardRef<'a> {
    pub rarity: u8,
    pub name: &'a [u8],
    pub uri_suffix: &'a [u8],
}

pub fn card_at(data: &[u8], id: usize) -> Option<CardRef<'_>> {
    let o = CardTable::HEADER_LEN + id * CardTable::ENTRY_LEN;
    let e = data.get(o..o + CardTable::ENTRY_LEN)?;
    let name_len = (e[1] as usize).min(MAX_NAME_LEN);
    let s = 2 + MAX_NAME_LEN;
    let suffix_len = (e[s] as usize).min(MAX_URI_SUFFIX_LEN);
    Some(CardRef { rarity: e[0], name: &e[2..2 + name_len], uri_suffix: &e[s + 1..s + 1 + suffix_len] })
}

pub fn write_card(data: &mut [u8], id: usize, rarity: u8, name: &[u8], uri_suffix: &[u8]) {
    let o = CardTable::HEADER_LEN + id * CardTable::ENTRY_LEN;
    let e = &mut data[o..o + CardTable::ENTRY_LEN];
    e.fill(0);
    e[0] = rarity;
    e[1] = name.len() as u8;
    e[2..2 + name.len()].copy_from_slice(name);
    let s = 2 + MAX_NAME_LEN;
    e[s] = uri_suffix.len() as u8;
    e[s + 1..s + 1 + uri_suffix.len()].copy_from_slice(uri_suffix);
}

pub fn rarity_counts(data: &[u8], count: u16) -> [u16; RARITY_COUNT] {
    let mut counts = [0u16; RARITY_COUNT];
    for id in 0..count as usize {
        let r = data[CardTable::HEADER_LEN + id * CardTable::ENTRY_LEN] as usize;
        if r < RARITY_COUNT {
            counts[r] += 1;
        }
    }
    counts
}

/// Card id of the `index`-th card (0-based) with the given rarity.
pub fn nth_of_rarity(data: &[u8], count: u16, rarity: u8, index: u16) -> Option<u16> {
    let mut seen = 0u16;
    for id in 0..count as usize {
        if data[CardTable::HEADER_LEN + id * CardTable::ENTRY_LEN] == rarity {
            if seen == index {
                return Some(id as u16);
            }
            seen += 1;
        }
    }
    None
}

#[event]
pub struct PackBought {
    pub expansion_id: u16,
    pub order_id: [u8; 16],
    pub player: Pubkey,
    pub pending_pack: Pubkey,
    pub currency: u8,
    /// Raw units of the payment mint.
    pub price: u64,
    pub purchase_slot: u64,
}

#[event]
pub struct PackSealed {
    pub pending_pack: Pubkey,
    pub slot_hash: [u8; 32],
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
    pub currency: u8,
    pub price: u64,
    pub seed: [u8; 32],
}

#[event]
pub struct PackRedrawn {
    pub pending_pack: Pubkey,
    pub old_slot: u64,
    pub new_slot: u64,
    pub redraws: u8,
    /// What is left of the player's retry deposit; returned to the player when the PendingPack closes.
    pub retry_pool: u64,
}

#[event]
pub struct PackRefunded {
    pub expansion_id: u16,
    pub order_id: [u8; 16],
    pub player: Pubkey,
    pub currency: u8,
    pub price: u64,
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
