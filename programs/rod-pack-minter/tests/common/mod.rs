//! Test harness: LiteSVM with this program plus the real Bubblegum, account compression, noop and Token Metadata
//! programs (binaries dumped from devnet into tests/fixtures by scripts/fetch-fixtures.ps1). Test keys only.
#![allow(dead_code)]

use anchor_lang::{
    prelude::Pubkey,
    solana_program::{instruction::{AccountMeta, Instruction}, system_instruction, system_program},
    InstructionData, ToAccountMetas,
};
use litesvm::{types::TransactionMetadata, LiteSVM};
use rod_pack_minter::{bubblegum, constants::*};
use solana_account::Account;
use solana_keypair::Keypair;
use solana_message::{Message, VersionedMessage};
use solana_signer::Signer;
use solana_transaction::versioned::VersionedTransaction;

pub const TOKEN_PROGRAM: Pubkey = anchor_lang::pubkey!("TokenkegQfeZyiNwAJbNbGKPFXCWuBvf9Ss623VQ5DA");
pub const RENT_SYSVAR: Pubkey = anchor_lang::pubkey!("SysvarRent111111111111111111111111111111111");
pub const INSTRUCTIONS_SYSVAR: Pubkey = anchor_lang::pubkey!("Sysvar1nstructions1111111111111111111111111");
pub const BOOTY_DECIMALS: u8 = 9;
pub const BOOTY: u64 = 1_000_000_000;

pub fn fixture(name: &str) -> Vec<u8> {
    let path = format!("{}/../../tests/fixtures/{}.so", env!("CARGO_MANIFEST_DIR"), name);
    std::fs::read(&path).unwrap_or_else(|_| panic!("missing {path}: run scripts/fetch-fixtures.ps1"))
}

pub fn pda(seeds: &[&[u8]], program: &Pubkey) -> Pubkey {
    Pubkey::find_program_address(seeds, program).0
}

pub fn config_pda() -> Pubkey {
    pda(&[CONFIG_SEED], &rod_pack_minter::ID)
}
pub fn expansion_pda(id: u16) -> Pubkey {
    pda(&[EXPANSION_SEED, &id.to_le_bytes()], &rod_pack_minter::ID)
}
pub fn cards_pda(id: u16) -> Pubkey {
    pda(&[CARDS_SEED, &id.to_le_bytes()], &rod_pack_minter::ID)
}
pub fn mint_authority_pda() -> Pubkey {
    pda(&[MINT_AUTHORITY_SEED], &rod_pack_minter::ID)
}
pub fn event_authority_pda() -> Pubkey {
    pda(&[b"__event_authority"], &rod_pack_minter::ID)
}
pub fn metadata_pda(mint: &Pubkey) -> Pubkey {
    pda(&[b"metadata", TOKEN_METADATA_ID.as_ref(), mint.as_ref()], &TOKEN_METADATA_ID)
}
pub fn edition_pda(mint: &Pubkey) -> Pubkey {
    pda(&[b"metadata", TOKEN_METADATA_ID.as_ref(), mint.as_ref(), b"edition"], &TOKEN_METADATA_ID)
}
pub fn collection_record_pda(mint: &Pubkey, authority: &Pubkey) -> Pubkey {
    pda(&[b"metadata", TOKEN_METADATA_ID.as_ref(), mint.as_ref(), b"collection_authority", authority.as_ref()], &TOKEN_METADATA_ID)
}

fn coption_key(out: &mut Vec<u8>, k: Option<&Pubkey>) {
    match k {
        Some(k) => {
            out.extend_from_slice(&1u32.to_le_bytes());
            out.extend_from_slice(k.as_ref());
        }
        None => out.extend_from_slice(&[0u8; 36]),
    }
}

/// SPL Token mint, 82 bytes.
pub fn mint_data(authority: Option<&Pubkey>, supply: u64, decimals: u8) -> Vec<u8> {
    mint_data_with_freeze(authority, supply, decimals, None)
}

pub fn mint_data_with_freeze(authority: Option<&Pubkey>, supply: u64, decimals: u8, freeze: Option<&Pubkey>) -> Vec<u8> {
    let mut d = Vec::with_capacity(82);
    coption_key(&mut d, authority);
    d.extend_from_slice(&supply.to_le_bytes());
    d.push(decimals);
    d.push(1);
    coption_key(&mut d, freeze);
    d
}

/// SPL Token account, 165 bytes.
pub fn token_account_data(mint: &Pubkey, owner: &Pubkey, amount: u64) -> Vec<u8> {
    let mut d = Vec::with_capacity(165);
    d.extend_from_slice(mint.as_ref());
    d.extend_from_slice(owner.as_ref());
    d.extend_from_slice(&amount.to_le_bytes());
    coption_key(&mut d, None);
    d.push(1);
    d.extend_from_slice(&[0u8; 12]);
    d.extend_from_slice(&0u64.to_le_bytes());
    coption_key(&mut d, None);
    d
}

pub fn token_amount(svm: &LiteSVM, account: &Pubkey) -> u64 {
    let a = svm.get_account(account).unwrap();
    u64::from_le_bytes(a.data[64..72].try_into().unwrap())
}

pub fn set_owned(svm: &mut LiteSVM, address: Pubkey, owner: Pubkey, data: Vec<u8>) {
    let lamports = svm.minimum_balance_for_rent_exemption(data.len());
    svm.set_account(address, Account { lamports, data, owner, executable: false, rent_epoch: 0 }).unwrap();
}

/// SlotHashes with the given (slot, hash) entries, newest first.
pub fn set_slot_hashes(svm: &mut LiteSVM, entries: &[(u64, [u8; 32])]) {
    let mut d = Vec::with_capacity(8 + entries.len() * 40);
    d.extend_from_slice(&(entries.len() as u64).to_le_bytes());
    for (slot, hash) in entries {
        d.extend_from_slice(&slot.to_le_bytes());
        d.extend_from_slice(hash);
    }
    let sysvar_owner = anchor_lang::pubkey!("Sysvar1111111111111111111111111111111111111");
    set_owned(svm, SLOT_HASHES_ID, sysvar_owner, d);
}

/// The test chain's "hash" of a slot.
pub fn slot_hash_of(slot: u64) -> [u8; 32] {
    solana_sha256_hasher::hashv(&[b"test slot", &slot.to_le_bytes()]).to_bytes()
}

pub fn current_slot(svm: &LiteSVM) -> u64 {
    svm.get_sysvar::<anchor_lang::prelude::Clock>().slot
}

/// Moves to `slot` and fills SlotHashes with the 512 slots before it (as the runtime would).
pub fn warp(svm: &mut LiteSVM, slot: u64) {
    svm.warp_to_slot(slot);
    let entries: Vec<(u64, [u8; 32])> = (1..=512u64).filter(|d| *d <= slot).map(|d| (slot - d, slot_hash_of(slot - d))).collect();
    set_slot_hashes(svm, &entries);
}

pub fn pending_pda(player: &Pubkey, order_id: &[u8; 16]) -> Pubkey {
    pda(&[PACK_SEED, player.as_ref(), order_id], &rod_pack_minter::ID)
}

pub fn vault_pda(mint: &Pubkey) -> Pubkey {
    pda(&[VAULT_SEED, mint.as_ref()], &rod_pack_minter::ID)
}

pub const OPEN_FEE: u64 = 20_000;
pub const RETRY_FEE: u64 = 5_000;
pub const PAID_RETRIES: u8 = 3;
pub const RETRY_POOL: u64 = RETRY_FEE * PAID_RETRIES as u64;
pub const REFUND_AFTER: i64 = 86_400;

pub fn compute_limit_ix(units: u32) -> Instruction {
    let mut data = vec![2u8];
    data.extend_from_slice(&units.to_le_bytes());
    Instruction { program_id: COMPUTE_BUDGET_ID, accounts: vec![], data }
}

pub fn send(svm: &mut LiteSVM, ixs: &[Instruction], payer: &Keypair, signers: &[&Keypair]) -> Result<TransactionMetadata, String> {
    svm.expire_blockhash();
    let blockhash = svm.latest_blockhash();
    let msg = Message::new_with_blockhash(ixs, Some(&payer.pubkey()), &blockhash);
    let mut all: Vec<&Keypair> = vec![payer];
    for s in signers {
        if s.pubkey() != payer.pubkey() {
            all.push(s);
        }
    }
    let tx = VersionedTransaction::try_new(VersionedMessage::Legacy(msg), &all).map_err(|e| e.to_string())?;
    svm.send_transaction(tx).map_err(|e| format!("{:?}\n{}", e.err, e.meta.logs.join("\n")))
}

/// Asserts the error output names an Anchor error (by its name in the logs).
pub fn assert_err<T>(res: Result<T, String>, needle: &str) {
    match res {
        Ok(_) => panic!("expected failure containing {needle:?}, but the transaction succeeded"),
        Err(e) => assert!(e.contains(needle), "expected {needle:?} in:\n{e}"),
    }
}

pub struct Env {
    pub svm: LiteSVM,
    pub opener: Keypair,
    pub next_order: u64,
    pub admin: Keypair,
    pub approver: Keypair,
    pub funder: Keypair,
    pub player: Keypair,
    pub booty_mint: Pubkey,
    pub player_token: Pubkey,
    pub shop_token: Pubkey,
    pub collection_owner: Keypair,
    pub collection_mint: Pubkey,
    pub creator: Pubkey,
}

pub const TREE_DEPTH: u32 = 14;
pub const TREE_BUFFER: u32 = 64;
pub const TREE_CANOPY: u32 = 8;

impl Env {
    /// Programs loaded, tokens and a sized test collection created, program initialised (paused) with a 14/64/8 tree size.
    pub fn new() -> Self {
        Self::with_tree_size(TREE_DEPTH, TREE_BUFFER, TREE_CANOPY)
    }

    pub fn with_tree_size(depth: u32, buffer: u32, canopy: u32) -> Self {
        let mut env = Self::setup();
        env.initialize(depth, buffer, canopy).expect("initialize");
        env
    }

    /// Programs, tokens and collection, but the program is not initialised yet.
    pub fn setup() -> Self {
        let mut svm = LiteSVM::new();
        let admin = Keypair::new();
        let approver = Keypair::new();
        let funder = Keypair::new();
        let player = Keypair::new();
        let collection_owner = Keypair::new();
        for k in [&admin, &approver, &funder, &player, &collection_owner] {
            svm.airdrop(&k.pubkey(), 100_000_000_000).unwrap();
        }

        svm.add_program(BUBBLEGUM_ID, &fixture("mpl_bubblegum")).unwrap();
        svm.add_program(ACCOUNT_COMPRESSION_ID, &fixture("spl_account_compression")).unwrap();
        svm.add_program(NOOP_ID, &fixture("spl_noop")).unwrap();
        svm.add_program(TOKEN_METADATA_ID, &fixture("mpl_token_metadata")).unwrap();
        let so = std::fs::read(concat!(env!("CARGO_MANIFEST_DIR"), "/../../target/deploy/rod_pack_minter.so"))
            .expect("run `anchor build` first");
        svm.add_program(rod_pack_minter::ID, &so).unwrap();
        // Make `admin` the upgrade authority, as `solana program deploy` would.
        let programdata = pda(&[rod_pack_minter::ID.as_ref()], &anchor_lang::solana_program::bpf_loader_upgradeable::ID);
        let mut pd = svm.get_account(&programdata).unwrap();
        pd.data[12] = 1;
        pd.data[13..45].copy_from_slice(admin.pubkey().as_ref());
        svm.set_account(programdata, pd).unwrap();

        warp(&mut svm, 1000);

        // $BOOTY (9 decimals): player holds 10,000, shop wallet account starts at 0.
        let booty_mint = Pubkey::new_unique();
        set_owned(&mut svm, booty_mint, TOKEN_PROGRAM, mint_data(None, 1_000_000 * BOOTY, BOOTY_DECIMALS));
        let player_token = Pubkey::new_unique();
        set_owned(&mut svm, player_token, TOKEN_PROGRAM, token_account_data(&booty_mint, &player.pubkey(), 10_000 * BOOTY));
        let shop_token = Pubkey::new_unique();
        set_owned(&mut svm, shop_token, TOKEN_PROGRAM, token_account_data(&booty_mint, &Pubkey::new_unique(), 0));

        let opener = Keypair::new();
        svm.airdrop(&opener.pubkey(), 1_000_000_000).unwrap();
        let mut env = Env {
            svm,
            opener,
            next_order: 1,
            admin,
            approver,
            funder,
            player,
            booty_mint,
            player_token,
            shop_token,
            collection_owner,
            collection_mint: Pubkey::default(),
            creator: Pubkey::new_unique(),
        };
        env.create_collection();
        env
    }

    /// A sized Token Metadata collection NFT owned by `collection_owner`, and the collection-authority record for the
    /// program's mint authority (the one-off mainnet step signed by the collection's update authority).
    fn create_collection(&mut self) {
        let owner = self.collection_owner.pubkey();
        let mint = Pubkey::new_unique();
        set_owned(&mut self.svm, mint, TOKEN_PROGRAM, mint_data_with_freeze(Some(&owner), 1, 0, Some(&owner)));
        let holder = Pubkey::new_unique();
        set_owned(&mut self.svm, holder, TOKEN_PROGRAM, token_account_data(&mint, &owner, 1));
        let metadata = metadata_pda(&mint);
        let edition = edition_pda(&mint);

        // CreateMetadataAccountV3 (33): DataV2, is_mutable, collection_details = Some(V1 { size: 0 })
        let mut d = vec![33u8];
        for s in ["Test Cards", "", "https://example.com/collection.json"] {
            d.extend_from_slice(&(s.len() as u32).to_le_bytes());
            d.extend_from_slice(s.as_bytes());
        }
        d.extend_from_slice(&0u16.to_le_bytes());
        d.extend_from_slice(&[0, 0, 0]); // creators, collection, uses = None
        d.push(1); // is_mutable
        d.extend_from_slice(&[1, 0]); // Some(CollectionDetails::V1
        d.extend_from_slice(&0u64.to_le_bytes()); //   size 0)
        let create_md = Instruction {
            program_id: TOKEN_METADATA_ID,
            accounts: vec![
                AccountMeta::new(metadata, false),
                AccountMeta::new_readonly(mint, false),
                AccountMeta::new_readonly(owner, true),
                AccountMeta::new(owner, true),
                AccountMeta::new_readonly(owner, true),
                AccountMeta::new_readonly(system_program::ID, false),
                AccountMeta::new_readonly(RENT_SYSVAR, false),
            ],
            data: d,
        };
        // CreateMasterEditionV3 (17): max_supply = Some(0)
        let mut d = vec![17u8, 1];
        d.extend_from_slice(&0u64.to_le_bytes());
        let create_me = Instruction {
            program_id: TOKEN_METADATA_ID,
            accounts: vec![
                AccountMeta::new(edition, false),
                AccountMeta::new(mint, false),
                AccountMeta::new_readonly(owner, true),
                AccountMeta::new_readonly(owner, true),
                AccountMeta::new(owner, true),
                AccountMeta::new(metadata, false),
                AccountMeta::new_readonly(TOKEN_PROGRAM, false),
                AccountMeta::new_readonly(system_program::ID, false),
                AccountMeta::new_readonly(RENT_SYSVAR, false),
            ],
            data: d,
        };
        // ApproveCollectionAuthority (23) for the program's mint authority PDA
        let authority = mint_authority_pda();
        let approve = Instruction {
            program_id: TOKEN_METADATA_ID,
            accounts: vec![
                AccountMeta::new(collection_record_pda(&mint, &authority), false),
                AccountMeta::new_readonly(authority, false),
                AccountMeta::new(owner, true),
                AccountMeta::new(owner, true),
                AccountMeta::new_readonly(metadata, false),
                AccountMeta::new_readonly(mint, false),
                AccountMeta::new_readonly(system_program::ID, false),
                AccountMeta::new_readonly(RENT_SYSVAR, false),
            ],
            data: vec![23],
        };
        let owner_kp = self.collection_owner.insecure_clone();
        send(&mut self.svm, &[create_md, create_me, approve], &owner_kp, &[]).expect("create collection");
        self.collection_mint = mint;
    }

    pub fn initialize_ix(&self, signer: &Pubkey, depth: u32, buffer: u32, canopy: u32) -> Instruction {
        let programdata = pda(&[rod_pack_minter::ID.as_ref()], &anchor_lang::solana_program::bpf_loader_upgradeable::ID);
        Instruction::new_with_bytes(
            rod_pack_minter::ID,
            &rod_pack_minter::instruction::Initialize {
                params: rod_pack_minter::InitParams {
                    approver: self.approver.pubkey(),
                    tree_funder: self.funder.pubkey(),
                    tree_max_depth: depth,
                    tree_max_buffer: buffer,
                    tree_canopy_depth: canopy,
                    open_fee_lamports: OPEN_FEE,
                    retry_fee_lamports: RETRY_FEE,
                    paid_retries: PAID_RETRIES,
                    refund_after_secs: REFUND_AFTER,
                },
            }
            .data(),
            rod_pack_minter::accounts::Initialize {
                admin: *signer,
                config: config_pda(),
                mint_authority: mint_authority_pda(),
                program: rod_pack_minter::ID,
                program_data: programdata,
                booty_mint: self.booty_mint,
                booty_destination: self.shop_token,
                vault: vault_pda(&self.booty_mint),
                token_program: TOKEN_PROGRAM,
                system_program: system_program::ID,
            }
            .to_account_metas(None),
        )
    }

    pub fn initialize(&mut self, depth: u32, buffer: u32, canopy: u32) -> Result<TransactionMetadata, String> {
        let ix = self.initialize_ix(&self.admin.pubkey(), depth, buffer, canopy);
        let admin = self.admin.insecure_clone();
        send(&mut self.svm, &[ix], &admin, &[])
    }

    pub fn admin_ix(&self, data: Vec<u8>) -> Instruction {
        Instruction::new_with_bytes(
            rod_pack_minter::ID,
            &data,
            rod_pack_minter::accounts::AdminOnly { admin: self.admin.pubkey(), config: config_pda() }.to_account_metas(None),
        )
    }

    pub fn run_admin(&mut self, ix: Instruction) -> Result<TransactionMetadata, String> {
        let admin = self.admin.insecure_clone();
        send(&mut self.svm, &[ix], &admin, &[])
    }

    pub fn set_paused(&mut self, paused: bool) -> Result<TransactionMetadata, String> {
        let ix = self.admin_ix(rod_pack_minter::instruction::SetPaused { paused }.data());
        self.run_admin(ix)
    }

    pub fn first_crush_params(&self) -> rod_pack_minter::ExpansionParams {
        rod_pack_minter::ExpansionParams {
            name: "First Crush".into(),
            collection_mint: self.collection_mint,
            creator: self.creator,
            royalty_bps: 690,
            symbol: "".into(),
            uri_prefix: "https://f005.backblazeb2.com/file/cryptoidolz/cards/metadata/".into(),
            pack_size: 6,
            weights_regular: [70, 25, 5, 0],
            weights_guaranteed: [0, 25, 5, 0],
            sale_start: 0,
            sale_end: 0,
        }
    }

    /// Adds First Crush (expansion 1) with 66 test cards (22 per rarity), price 150 $BOOTY, unpaused.
    pub fn add_first_crush(&mut self) -> u16 {
        let id = 1u16;
        let ix = Instruction::new_with_bytes(
            rod_pack_minter::ID,
            &rod_pack_minter::instruction::AddExpansion { params: self.first_crush_params(), price: 150 * BOOTY }.data(),
            rod_pack_minter::accounts::AddExpansion {
                admin: self.admin.pubkey(),
                config: config_pda(),
                expansion: expansion_pda(id),
                card_table: cards_pda(id),
                system_program: system_program::ID,
            }
            .to_account_metas(None),
        );
        self.run_admin(ix).expect("add_expansion");
        let cards: Vec<rod_pack_minter::CardInput> = (0..66u16)
            .map(|i| rod_pack_minter::CardInput {
                rarity: (i / 22) as u8,
                name: format!("Test Card {i:02} with a long name"),
                uri_suffix: format!("CARD{i:02}XXXXXXXXXXXX.json"),
            })
            .collect();
        for (n, chunk) in cards.chunks(11).enumerate() {
            let ix = self.upsert_ix(&self.admin.pubkey(), id, (n * 11) as u16, chunk.to_vec());
            self.run_admin(ix).expect("upsert_cards");
        }
        let ix = self.expansion_admin_ix(id, rod_pack_minter::instruction::SetExpansionPaused { expansion_id: id, paused: false }.data());
        self.run_admin(ix).expect("unpause expansion");
        id
    }

    pub fn upsert_ix(&self, signer: &Pubkey, id: u16, start: u16, cards: Vec<rod_pack_minter::CardInput>) -> Instruction {
        Instruction::new_with_bytes(
            rod_pack_minter::ID,
            &rod_pack_minter::instruction::UpsertCards { expansion_id: id, start_index: start, cards }.data(),
            rod_pack_minter::accounts::UpsertCards {
                admin: *signer,
                config: config_pda(),
                expansion: expansion_pda(id),
                card_table: cards_pda(id),
                system_program: system_program::ID,
            }
            .to_account_metas(None),
        )
    }

    pub fn expansion_admin_ix(&self, id: u16, data: Vec<u8>) -> Instruction {
        Instruction::new_with_bytes(
            rod_pack_minter::ID,
            &data,
            rod_pack_minter::accounts::ExpansionAdmin { admin: self.admin.pubkey(), config: config_pda(), expansion: expansion_pda(id) }
                .to_account_metas(None),
        )
    }

    /// [create_account for a new tree keypair, create_tree], signed by `authority` and the tree keypair.
    pub fn create_tree(&mut self, authority: &Keypair) -> Result<Pubkey, String> {
        let config = self.config();
        let tree = Keypair::new();
        let size = bubblegum::merkle_tree_account_size(config.tree_max_depth, config.tree_max_buffer, config.tree_canopy_depth);
        let lamports = self.svm.minimum_balance_for_rent_exemption(size);
        let alloc = system_instruction::create_account(&authority.pubkey(), &tree.pubkey(), lamports, size as u64, &ACCOUNT_COMPRESSION_ID);
        let ix = Instruction::new_with_bytes(
            rod_pack_minter::ID,
            &rod_pack_minter::instruction::CreateTree {}.data(),
            rod_pack_minter::accounts::CreateTree {
                authority: authority.pubkey(),
                config: config_pda(),
                merkle_tree: tree.pubkey(),
                tree_config: bubblegum::tree_config_address(&tree.pubkey()),
                mint_authority: mint_authority_pda(),
                log_wrapper: NOOP_ID,
                compression_program: ACCOUNT_COMPRESSION_ID,
                bubblegum_program: BUBBLEGUM_ID,
                system_program: system_program::ID,
                event_authority: event_authority_pda(),
                program: rod_pack_minter::ID,
            }
            .to_account_metas(None),
        );
        let authority = authority.insecure_clone();
        send(&mut self.svm, &[alloc, ix], &authority, &[&tree]).map(|_| tree.pubkey())
    }

    pub fn rotate_ix(&self) -> Instruction {
        let config = self.config();
        Instruction::new_with_bytes(
            rod_pack_minter::ID,
            &rod_pack_minter::instruction::RotateTree {}.data(),
            rod_pack_minter::accounts::RotateTree {
                config: config_pda(),
                active_tree_config: bubblegum::tree_config_address(&config.active_tree),
                event_authority: event_authority_pda(),
                program: rod_pack_minter::ID,
            }
            .to_account_metas(None),
        )
    }

    pub fn mint_accounts(&self, tree: &Pubkey) -> rod_pack_minter::accounts::MintAccounts {
        rod_pack_minter::accounts::MintAccounts {
            mint_authority: mint_authority_pda(),
            tree_config: bubblegum::tree_config_address(tree),
            merkle_tree: *tree,
            collection_mint: self.collection_mint,
            collection_metadata: metadata_pda(&self.collection_mint),
            collection_edition: edition_pda(&self.collection_mint),
            collection_authority_record: collection_record_pda(&self.collection_mint, &mint_authority_pda()),
            bubblegum_signer: bubblegum::bubblegum_signer_address(),
            log_wrapper: NOOP_ID,
            compression_program: ACCOUNT_COMPRESSION_ID,
            token_metadata_program: TOKEN_METADATA_ID,
            bubblegum_program: BUBBLEGUM_ID,
            system_program: system_program::ID,
        }
    }

    pub fn new_order(&mut self) -> [u8; 16] {
        let mut o = [0u8; 16];
        o[..8].copy_from_slice(b"order-00");
        o[8..].copy_from_slice(&self.next_order.to_le_bytes());
        self.next_order += 1;
        o
    }

    pub fn buy_ix(&self, id: u16, order_id: [u8; 16], max_price: u64, approver: &Pubkey) -> Instruction {
        let c = self.config();
        let active_tc = bubblegum::tree_config_address(&c.active_tree);
        let standby_tc = if c.standby_tree == Pubkey::default() { active_tc } else { bubblegum::tree_config_address(&c.standby_tree) };
        Instruction::new_with_bytes(
            rod_pack_minter::ID,
            &rod_pack_minter::instruction::BuyPack { expansion_id: id, order_id, max_price }.data(),
            rod_pack_minter::accounts::BuyPack {
                player: self.player.pubkey(),
                approver: *approver,
                config: config_pda(),
                expansion: expansion_pda(id),
                card_table: cards_pda(id),
                pending_pack: pending_pda(&self.player.pubkey(), &order_id),
                player_token: self.player_token,
                vault: vault_pda(&self.booty_mint),
                booty_mint: self.booty_mint,
                token_program: TOKEN_PROGRAM,
                active_tree_config: active_tc,
                standby_tree_config: standby_tc,
                system_program: system_program::ID,
                event_authority: event_authority_pda(),
                program: rod_pack_minter::ID,
            }
            .to_account_metas(None),
        )
    }

    /// buy_pack signed by the player and the approver. Returns the order id.
    pub fn buy(&mut self, id: u16, max_price: u64) -> Result<[u8; 16], String> {
        let order = self.new_order();
        let ix = self.buy_ix(id, order, max_price, &self.approver.pubkey());
        let (player, approver) = (self.player.insecure_clone(), self.approver.insecure_clone());
        send(&mut self.svm, &[ix], &player, &[&approver]).map(|_| order)
    }

    pub fn open_ix(&self, opener: &Pubkey, order_id: &[u8; 16], player: &Pubkey, expansion_id: u16) -> Instruction {
        let tree = self.config().active_tree;
        Instruction::new_with_bytes(
            rod_pack_minter::ID,
            &rod_pack_minter::instruction::OpenPack {}.data(),
            rod_pack_minter::accounts::OpenPack {
                opener: *opener,
                config: config_pda(),
                pending_pack: pending_pda(player, order_id),
                player: *player,
                expansion: expansion_pda(expansion_id),
                card_table: cards_pda(expansion_id),
                vault: vault_pda(&self.booty_mint),
                booty_destination: self.shop_token,
                booty_mint: self.booty_mint,
                token_program: TOKEN_PROGRAM,
                mint: self.mint_accounts(&tree),
                slot_hashes: SLOT_HASHES_ID,
                event_authority: event_authority_pda(),
                program: rod_pack_minter::ID,
            }
            .to_account_metas(None),
        )
    }

    /// open_pack sent by the backend's opener key, with a compute limit.
    pub fn open(&mut self, order_id: &[u8; 16], expansion_id: u16) -> Result<TransactionMetadata, String> {
        let opener = self.opener.insecure_clone();
        self.open_by(&opener, order_id, expansion_id)
    }

    pub fn open_by(&mut self, opener: &Keypair, order_id: &[u8; 16], expansion_id: u16) -> Result<TransactionMetadata, String> {
        let ix = self.open_ix(&opener.pubkey(), order_id, &self.player.pubkey(), expansion_id);
        send(&mut self.svm, &[compute_limit_ix(1_400_000), ix], opener, &[])
    }

    /// Moves one slot forward (the purchase slot is complete) and opens.
    pub fn next_slot_and_open(&mut self, order_id: &[u8; 16], expansion_id: u16) -> Result<TransactionMetadata, String> {
        let s = current_slot(&self.svm);
        warp(&mut self.svm, s + 1);
        self.open(order_id, expansion_id)
    }

    /// buy_pack, then open_pack one slot later.
    pub fn buy_and_open(&mut self, id: u16, max_price: u64) -> Result<TransactionMetadata, String> {
        let order = self.buy(id, max_price)?;
        self.next_slot_and_open(&order, id)
    }

    pub fn seal_ix(&self, sealer: &Pubkey, order_id: &[u8; 16]) -> Instruction {
        Instruction::new_with_bytes(
            rod_pack_minter::ID,
            &rod_pack_minter::instruction::SealPack {}.data(),
            rod_pack_minter::accounts::SealPack {
                sealer: *sealer,
                pending_pack: pending_pda(&self.player.pubkey(), order_id),
                slot_hashes: SLOT_HASHES_ID,
                event_authority: event_authority_pda(),
                program: rod_pack_minter::ID,
            }
            .to_account_metas(None),
        )
    }

    pub fn redraw_ix(&self, caller: &Pubkey, order_id: &[u8; 16]) -> Instruction {
        Instruction::new_with_bytes(
            rod_pack_minter::ID,
            &rod_pack_minter::instruction::RedrawPack {}.data(),
            rod_pack_minter::accounts::RedrawPack {
                caller: *caller,
                config: config_pda(),
                pending_pack: pending_pda(&self.player.pubkey(), order_id),
                slot_hashes: SLOT_HASHES_ID,
                event_authority: event_authority_pda(),
                program: rod_pack_minter::ID,
            }
            .to_account_metas(None),
        )
    }

    /// The admin's refund of a pack of the test player.
    pub fn refund_ix(&self, signer: &Pubkey, order_id: &[u8; 16]) -> Instruction {
        Instruction::new_with_bytes(
            rod_pack_minter::ID,
            &rod_pack_minter::instruction::RefundPack {}.data(),
            rod_pack_minter::accounts::RefundPack {
                admin: *signer,
                config: config_pda(),
                pending_pack: pending_pda(&self.player.pubkey(), order_id),
                player: self.player.pubkey(),
                vault: vault_pda(&self.booty_mint),
                player_token: self.player_token,
                booty_mint: self.booty_mint,
                token_program: TOKEN_PROGRAM,
                event_authority: event_authority_pda(),
                program: rod_pack_minter::ID,
            }
            .to_account_metas(None),
        )
    }

    pub fn pending(&self, order_id: &[u8; 16]) -> Option<rod_pack_minter::PendingPack> {
        let a = self.svm.get_account(&pending_pda(&self.player.pubkey(), order_id))?;
        if a.data.is_empty() {
            return None;
        }
        Some(anchor_lang::AccountDeserialize::try_deserialize(&mut a.data.as_slice()).unwrap())
    }

    pub fn set_time(&mut self, unix_timestamp: i64) {
        let mut clock = self.svm.get_sysvar::<anchor_lang::prelude::Clock>();
        clock.unix_timestamp = unix_timestamp;
        self.svm.set_sysvar(&clock);
    }

    pub fn now(&self) -> i64 {
        self.svm.get_sysvar::<anchor_lang::prelude::Clock>().unix_timestamp
    }

    pub fn admin_mint_ix(&self, signer: &Pubkey, id: u16, card_id: u16, count: u8, recipient: &Pubkey) -> Instruction {
        let tree = self.config().active_tree;
        Instruction::new_with_bytes(
            rod_pack_minter::ID,
            &rod_pack_minter::instruction::AdminMint { expansion_id: id, card_id, count }.data(),
            rod_pack_minter::accounts::AdminMint {
                admin: *signer,
                config: config_pda(),
                expansion: expansion_pda(id),
                card_table: cards_pda(id),
                recipient: *recipient,
                mint: self.mint_accounts(&tree),
                event_authority: event_authority_pda(),
                program: rod_pack_minter::ID,
            }
            .to_account_metas(None),
        )
    }

    pub fn config(&self) -> rod_pack_minter::Config {
        let a = self.svm.get_account(&config_pda()).unwrap();
        anchor_lang::AccountDeserialize::try_deserialize(&mut a.data.as_slice()).unwrap()
    }

    pub fn expansion(&self, id: u16) -> rod_pack_minter::Expansion {
        let a = self.svm.get_account(&expansion_pda(id)).unwrap();
        anchor_lang::AccountDeserialize::try_deserialize(&mut a.data.as_slice()).unwrap()
    }

    pub fn num_minted(&self, tree: &Pubkey) -> u64 {
        let a = self.svm.get_account(&bubblegum::tree_config_address(tree)).unwrap();
        u64::from_le_bytes(a.data[80..88].try_into().unwrap())
    }

    /// Program ready to sell: First Crush added, one active tree, unpaused.
    pub fn ready() -> (Self, u16, Pubkey) {
        Self::ready_with(Self::new())
    }

    pub fn ready_with(mut env: Self) -> (Self, u16, Pubkey) {
        let id = env.add_first_crush();
        let admin = env.admin.insecure_clone();
        let tree = env.create_tree(&admin).expect("create_tree");
        env.set_paused(false).unwrap();
        (env, id, tree)
    }
}

/// Decodes the PackMinted event from the self-CPI in a transaction's inner instructions.
pub fn pack_event(meta: &TransactionMetadata) -> rod_pack_minter::PackMinted {
    use anchor_lang::{AnchorDeserialize, Discriminator};
    for inner in meta.inner_instructions.iter().flatten() {
        let data = &inner.instruction.data;
        // event CPI data: EVENT_IX_TAG (8 bytes) + event discriminator + borsh
        if data.len() > 16 && data[8..16] == rod_pack_minter::PackMinted::DISCRIMINATOR[..] {
            return rod_pack_minter::PackMinted::deserialize(&mut &data[16..]).unwrap();
        }
    }
    panic!("no PackMinted event");
}
