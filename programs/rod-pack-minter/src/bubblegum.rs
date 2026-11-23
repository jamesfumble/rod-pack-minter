//! Hand-written CPI into Bubblegum v1. Layouts copied from the generated mpl-bubblegum Rust client
//! (clients/rust/src/generated/instructions/{mint_to_collection_v1,create_tree_config}.rs), so this program does not
//! depend on the mpl-bubblegum crate (which targets older Solana crates than Anchor 1.2).

use anchor_lang::prelude::*;
use anchor_lang::solana_program::instruction::{AccountMeta, Instruction};
use anchor_lang::solana_program::program::invoke_signed;

use crate::constants::*;
use crate::error::ErrorCode;

const MINT_TO_COLLECTION_V1: [u8; 8] = [153, 18, 178, 47, 197, 158, 86, 15];
const CREATE_TREE_CONFIG: [u8; 8] = [165, 83, 136, 142, 89, 202, 47, 220];

/// Size of a spl-account-compression tree account: header (56) + tree (24 + change logs + rightmost proof) + canopy.
pub fn merkle_tree_account_size(max_depth: u32, max_buffer: u32, canopy_depth: u32) -> usize {
    let path = 40 + 32 * max_depth as usize;
    let canopy_nodes = (1usize << (canopy_depth + 1)) - 2;
    56 + 24 + max_buffer as usize * path + path + canopy_nodes * 32
}

pub fn tree_config_address(merkle_tree: &Pubkey) -> Pubkey {
    Pubkey::find_program_address(&[merkle_tree.as_ref()], &BUBBLEGUM_ID).0
}

pub fn bubblegum_signer_address() -> Pubkey {
    Pubkey::find_program_address(&[b"collection_cpi"], &BUBBLEGUM_ID).0
}

/// Bubblegum TreeConfig (v1 layout): disc 8 | creator 32 | delegate 32 | total_mint_capacity u64 | num_minted u64 | ...
pub struct TreeConfigView {
    pub tree_creator: Pubkey,
    pub tree_delegate: Pubkey,
    pub total_mint_capacity: u64,
    pub num_minted: u64,
}

impl TreeConfigView {
    pub fn load(tree_config: &AccountInfo, merkle_tree: &Pubkey) -> Result<Self> {
        require_keys_eq!(*tree_config.owner, BUBBLEGUM_ID, ErrorCode::BadTreeConfig);
        require_keys_eq!(tree_config.key(), tree_config_address(merkle_tree), ErrorCode::BadTreeConfig);
        let data = tree_config.try_borrow_data()?;
        require!(data.len() >= 88, ErrorCode::BadTreeConfig);
        let read_u64 = |o: usize| u64::from_le_bytes(data[o..o + 8].try_into().unwrap());
        Ok(Self {
            tree_creator: Pubkey::try_from(&data[8..40]).unwrap(),
            tree_delegate: Pubkey::try_from(&data[40..72]).unwrap(),
            total_mint_capacity: read_u64(72),
            num_minted: read_u64(80),
        })
    }

    pub fn free_leaves(&self) -> u64 {
        self.total_mint_capacity.saturating_sub(self.num_minted)
    }
}

fn put_str(out: &mut Vec<u8>, s: &str) {
    out.extend_from_slice(&(s.len() as u32).to_le_bytes());
    out.extend_from_slice(s.as_bytes());
}

/// Borsh of Bubblegum `MetadataArgs` for one card.
pub fn metadata_args(name: &str, symbol: &str, uri: &str, royalty_bps: u16, collection: &Pubkey, creator: &Pubkey) -> Vec<u8> {
    let mut out = Vec::with_capacity(4 + name.len() + 4 + symbol.len() + 4 + uri.len() + 90);
    put_str(&mut out, name);
    put_str(&mut out, symbol);
    put_str(&mut out, uri);
    out.extend_from_slice(&royalty_bps.to_le_bytes());
    out.push(0); // primary_sale_happened = false
    out.push(1); // is_mutable = true
    out.push(0); // edition_nonce = None
    out.extend_from_slice(&[1, 0]); // token_standard = Some(NonFungible)
    out.push(1); // collection = Some
    out.push(0); //   verified = false (Bubblegum verifies it in mint_to_collection_v1)
    out.extend_from_slice(collection.as_ref());
    out.push(0); // uses = None
    out.push(0); // token_program_version = Original
    out.extend_from_slice(&1u32.to_le_bytes()); // creators: 1
    out.extend_from_slice(creator.as_ref());
    out.push(0); //   verified = false
    out.push(100); //   share = 100
    out
}

/// Accounts of one `mint_to_collection_v1` CPI. The mint authority PDA signs as tree delegate and collection authority.
pub struct MintCpi<'a, 'info> {
    pub tree_config: &'a AccountInfo<'info>,
    pub leaf_owner: &'a AccountInfo<'info>,
    pub merkle_tree: &'a AccountInfo<'info>,
    pub payer: &'a AccountInfo<'info>,
    pub mint_authority: &'a AccountInfo<'info>,
    pub collection_authority_record: &'a AccountInfo<'info>,
    pub collection_mint: &'a AccountInfo<'info>,
    pub collection_metadata: &'a AccountInfo<'info>,
    pub collection_edition: &'a AccountInfo<'info>,
    pub bubblegum_signer: &'a AccountInfo<'info>,
    pub log_wrapper: &'a AccountInfo<'info>,
    pub compression_program: &'a AccountInfo<'info>,
    pub token_metadata_program: &'a AccountInfo<'info>,
    pub system_program: &'a AccountInfo<'info>,
    pub bubblegum_program: &'a AccountInfo<'info>,
}

impl<'a, 'info> MintCpi<'a, 'info> {
    pub fn mint(&self, metadata: &[u8], signer_seeds: &[&[&[u8]]]) -> Result<()> {
        let mut data = Vec::with_capacity(8 + metadata.len());
        data.extend_from_slice(&MINT_TO_COLLECTION_V1);
        data.extend_from_slice(metadata);
        let ix = Instruction {
            program_id: BUBBLEGUM_ID,
            accounts: vec![
                AccountMeta::new(self.tree_config.key(), false),
                AccountMeta::new_readonly(self.leaf_owner.key(), false),
                AccountMeta::new_readonly(self.leaf_owner.key(), false), // leaf_delegate = owner
                AccountMeta::new(self.merkle_tree.key(), false),
                AccountMeta::new_readonly(self.payer.key(), true),
                AccountMeta::new_readonly(self.mint_authority.key(), true), // tree_creator_or_delegate
                AccountMeta::new_readonly(self.mint_authority.key(), true), // collection_authority
                AccountMeta::new_readonly(self.collection_authority_record.key(), false),
                AccountMeta::new_readonly(self.collection_mint.key(), false),
                AccountMeta::new(self.collection_metadata.key(), false),
                AccountMeta::new_readonly(self.collection_edition.key(), false),
                AccountMeta::new_readonly(self.bubblegum_signer.key(), false),
                AccountMeta::new_readonly(self.log_wrapper.key(), false),
                AccountMeta::new_readonly(self.compression_program.key(), false),
                AccountMeta::new_readonly(self.token_metadata_program.key(), false),
                AccountMeta::new_readonly(self.system_program.key(), false),
            ],
            data,
        };
        invoke_signed(
            &ix,
            &[
                self.tree_config.clone(),
                self.leaf_owner.clone(),
                self.merkle_tree.clone(),
                self.payer.clone(),
                self.mint_authority.clone(),
                self.collection_authority_record.clone(),
                self.collection_mint.clone(),
                self.collection_metadata.clone(),
                self.collection_edition.clone(),
                self.bubblegum_signer.clone(),
                self.log_wrapper.clone(),
                self.compression_program.clone(),
                self.token_metadata_program.clone(),
                self.system_program.clone(),
                self.bubblegum_program.clone(),
            ],
            signer_seeds,
        )?;
        Ok(())
    }
}

/// `create_tree_config` with the mint authority PDA as tree creator (and so delegate), not public.
#[allow(clippy::too_many_arguments)]
pub fn create_tree_config<'info>(
    tree_config: &AccountInfo<'info>,
    merkle_tree: &AccountInfo<'info>,
    payer: &AccountInfo<'info>,
    mint_authority: &AccountInfo<'info>,
    log_wrapper: &AccountInfo<'info>,
    compression_program: &AccountInfo<'info>,
    system_program: &AccountInfo<'info>,
    bubblegum_program: &AccountInfo<'info>,
    max_depth: u32,
    max_buffer: u32,
    signer_seeds: &[&[&[u8]]],
) -> Result<()> {
    let mut data = Vec::with_capacity(8 + 4 + 4 + 2);
    data.extend_from_slice(&CREATE_TREE_CONFIG);
    data.extend_from_slice(&max_depth.to_le_bytes());
    data.extend_from_slice(&max_buffer.to_le_bytes());
    data.extend_from_slice(&[1, 0]); // public = Some(false)
    let ix = Instruction {
        program_id: BUBBLEGUM_ID,
        accounts: vec![
            AccountMeta::new(tree_config.key(), false),
            AccountMeta::new(merkle_tree.key(), false),
            AccountMeta::new(payer.key(), true),
            AccountMeta::new_readonly(mint_authority.key(), true),
            AccountMeta::new_readonly(log_wrapper.key(), false),
            AccountMeta::new_readonly(compression_program.key(), false),
            AccountMeta::new_readonly(system_program.key(), false),
        ],
        data,
    };
    invoke_signed(
        &ix,
        &[
            tree_config.clone(),
            merkle_tree.clone(),
            payer.clone(),
            mint_authority.clone(),
            log_wrapper.clone(),
            compression_program.clone(),
            system_program.clone(),
            bubblegum_program.clone(),
        ],
        signer_seeds,
    )?;
    Ok(())
}
