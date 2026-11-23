use anchor_lang::prelude::*;

// PDA seeds
pub const CONFIG_SEED: &[u8] = b"config";
pub const EXPANSION_SEED: &[u8] = b"expansion";
pub const CARDS_SEED: &[u8] = b"cards";
pub const MINT_AUTHORITY_SEED: &[u8] = b"mint_authority";

// External programs (same addresses on mainnet, devnet and Surfpool)
pub const BUBBLEGUM_ID: Pubkey = pubkey!("BGUMAp9Gq7iTEuizy4pqaxsTyUCBK68MDfK752saRPUY");
pub const ACCOUNT_COMPRESSION_ID: Pubkey = pubkey!("cmtDvXumGCrqC1Age74AVPhSRVXJMd8PJS91L8KbNCK");
pub const NOOP_ID: Pubkey = pubkey!("noopb9bkMVfRPU8AsbpTUg8AQkHtKwMYZiFUjNRtMmV");
pub const TOKEN_METADATA_ID: Pubkey = pubkey!("metaqbxxUerdq28cj1RbAWkYQm3ybzjb6a8bt518x1s");
pub const COMPUTE_BUDGET_ID: Pubkey = pubkey!("ComputeBudget111111111111111111111111111111");
/// Phantom adds Lighthouse assertion instructions to transactions it signs.
pub const LIGHTHOUSE_ID: Pubkey = pubkey!("L2TExMFKdjpN9kozasaurPirfHy9P8sbXoAN1qA3S95");
pub const SLOT_HASHES_ID: Pubkey = pubkey!("SysvarS1otHashes111111111111111111111111111");

/// Largest pack an expansion may sell. A pack is minted in one transaction, so this is bounded by compute (1.4M CU).
pub const MAX_PACK_SIZE: u8 = 10;
/// The active tree may be rotated to the standby tree once it has fewer free leaves than this.
pub const ROTATE_BELOW_FREE_LEAVES: u64 = MAX_PACK_SIZE as u64;
pub const MAX_ADMIN_MINT: u8 = 6;

/// Rarities 0..RARITY_COUNT (First Crush: 0 Common, 1 Rare, 2 Epic). Four slots leave room for a later Legendary tier.
pub const RARITY_COUNT: usize = 4;

// Bubblegum / Token Metadata limits
pub const MAX_NAME_LEN: usize = 32;
pub const MAX_SYMBOL_LEN: usize = 10;
pub const MAX_URI_LEN: usize = 200;
pub const MAX_URI_PREFIX_LEN: usize = 128;
pub const MAX_URI_SUFFIX_LEN: usize = 72;
pub const MAX_EXPANSION_NAME_LEN: usize = 32;
pub const MAX_CARDS: usize = 400;
