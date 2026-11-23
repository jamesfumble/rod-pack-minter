mod common;

use anchor_lang::{
    prelude::{Clock, Pubkey},
    solana_program::instruction::{AccountMeta, Instruction},
    solana_program::{system_instruction, system_program},
    InstructionData,
};
use common::*;
use rod_pack_minter::{bubblegum, constants::*};
use solana_keypair::Keypair;
use solana_signer::Signer;

#[test]
fn buy_pack_pays_and_mints_six_cards() {
    let (mut env, id, tree) = Env::ready();
    let before_player = token_amount(&env.svm, &env.player_token);
    let sol_before = env.svm.get_balance(&env.player.pubkey()).unwrap();
    let meta = env.buy(id, 150 * BOOTY).expect("buy_pack");
    let sol_spent = sol_before - env.svm.get_balance(&env.player.pubkey()).unwrap();
    println!("player SOL spent on the pack: {sol_spent} lamports (fees for 2 signatures = 10000)");
    assert_eq!(sol_spent, 10_000, "the player pays only the signature fees (no priority fee in this test)");
    println!("buy_pack (6 cards) compute units: {}", meta.compute_units_consumed);

    assert_eq!(token_amount(&env.svm, &env.player_token), before_player - 150 * BOOTY);
    assert_eq!(token_amount(&env.svm, &env.shop_token), 150 * BOOTY);
    assert_eq!(env.num_minted(&tree), 6);

    let ev = pack_event(&meta);
    assert_eq!(ev.expansion_id, id);
    assert_eq!(ev.player, env.player.pubkey());
    assert_eq!(ev.tree, tree);
    assert_eq!(ev.first_leaf_index, 0);
    assert_eq!(ev.card_ids.len(), 6);
    assert_eq!(ev.price, 150 * BOOTY);
    // card ids 0-21 Common, 22-43 Rare, 44-65 Epic: the last card is Rare or Epic
    assert!(ev.card_ids[5] >= 22, "guaranteed slot drew card {}", ev.card_ids[5]);
    println!("cards: {:?}", ev.card_ids);

    let c = env.config();
    assert_eq!((c.packs_sold, c.cards_minted), (1, 6));
    let e = env.expansion(id);
    assert_eq!((e.packs_sold, e.cards_minted), (1, 6));

    // A second pack continues in the same tree, with different cards (new slot hash and leaf count).
    set_slot_hash(&mut env.svm, 1001, [43u8; 32]);
    let meta2 = env.buy(id, 150 * BOOTY).expect("second buy_pack");
    let ev2 = pack_event(&meta2);
    assert_eq!(ev2.first_leaf_index, 6);
    assert_ne!(ev2.seed, ev.seed);
    assert_eq!(env.num_minted(&tree), 12);
}

#[test]
fn ten_card_pack_fits_in_compute() {
    let (mut env, id, _) = Env::ready();
    let mut p = env.first_crush_params();
    p.pack_size = MAX_PACK_SIZE;
    let ix = env.expansion_admin_ix(id, rod_pack_minter::instruction::UpdateExpansion { expansion_id: id, params: p }.data());
    env.run_admin(ix).unwrap();
    let meta = env.buy(id, 150 * BOOTY).expect("buy 10-card pack");
    println!("buy_pack (10 cards) compute units: {}", meta.compute_units_consumed);
    assert!(meta.compute_units_consumed < 1_200_000);
    assert_eq!(pack_event(&meta).card_ids.len(), 10);
}

#[test]
fn transaction_fits_with_a_lookup_table() {
    use solana_message::{v0, AddressLookupTableAccount, VersionedMessage};
    let (env, id, _) = Env::ready();
    let buy = env.buy_ix(id, *b"order-0000000001", 150 * BOOTY, &env.approver.pubkey());
    let rotate = env.rotate_ix();
    let mut price = vec![3u8];
    price.extend_from_slice(&250_000u64.to_le_bytes());
    let ixs = [compute_limit_ix(600_000), Instruction { program_id: COMPUTE_BUDGET_ID, accounts: vec![], data: price }, rotate, buy.clone()];
    // Everything except the two signers and the player's own token account goes into the lookup table.
    let mut table: Vec<Pubkey> = Vec::new();
    for ix in &ixs {
        for m in ix.accounts.iter().map(|m| m.pubkey).chain(std::iter::once(ix.program_id)) {
            if m != env.player.pubkey() && m != env.approver.pubkey() && m != env.player_token && !table.contains(&m) {
                table.push(m);
            }
        }
    }
    let alt = AddressLookupTableAccount { key: Pubkey::new_unique(), addresses: table.clone() };
    let msg = v0::Message::try_compile(&env.player.pubkey(), &ixs, &[alt], Default::default()).unwrap();
    let bytes = VersionedMessage::V0(msg).serialize().len() + 1 + 2 * 64;
    println!("v0 transaction with rotate + buy_pack: {bytes} bytes ({} accounts in the lookup table)", table.len());
    assert!(bytes <= 1232);
}

#[test]
fn only_the_upgrade_authority_can_initialize() {
    let mut env = Env::setup();
    let intruder = Keypair::new();
    env.svm.airdrop(&intruder.pubkey(), 10_000_000_000).unwrap();
    let ix = env.initialize_ix(&intruder.pubkey(), TREE_DEPTH, TREE_BUFFER, TREE_CANOPY);
    assert_err(send(&mut env.svm, &[ix], &intruder, &[]), "Unauthorized");
    env.initialize(TREE_DEPTH, TREE_BUFFER, TREE_CANOPY).expect("upgrade authority initializes");
    let c = env.config();
    assert_eq!(c.admin, env.admin.pubkey());
    assert!(c.paused, "starts paused");
}

#[test]
fn buy_needs_the_approver() {
    let (mut env, id, _) = Env::ready();
    let fake = Keypair::new();
    let ix = env.buy_ix(id, *b"order-0000000001", 150 * BOOTY, &fake.pubkey());
    let player = env.player.insecure_clone();
    assert_err(send(&mut env.svm, &[ix], &player, &[&fake]), "WrongApprover");
    // A player cannot co-sign as the approver themselves either.
    let ix = env.buy_ix(id, *b"order-0000000001", 150 * BOOTY, &env.player.pubkey());
    assert_err(send(&mut env.svm, &[ix], &player, &[]), "WrongApprover");
}

#[test]
fn pause_stops_sales_and_admin_mints() {
    let (mut env, id, _) = Env::ready();
    env.set_paused(true).unwrap();
    assert_err(env.buy(id, 150 * BOOTY), "ProgramPaused");
    let ix = env.admin_mint_ix(&env.admin.pubkey(), id, 3, 1, &Pubkey::new_unique());
    assert_err(env.run_admin(ix), "ProgramPaused");
    env.set_paused(false).unwrap();

    let ix = env.expansion_admin_ix(id, rod_pack_minter::instruction::SetExpansionPaused { expansion_id: id, paused: true }.data());
    env.run_admin(ix).unwrap();
    assert_err(env.buy(id, 150 * BOOTY), "ExpansionPaused");
    let ix = env.admin_mint_ix(&env.admin.pubkey(), id, 3, 1, &Pubkey::new_unique());
    assert_err(env.run_admin(ix), "ExpansionPaused");
}

#[test]
fn price_limit_and_discount() {
    let (mut env, id, _) = Env::ready();
    assert_err(env.buy(id, 149 * BOOTY), "PriceAboveMax");

    let now = env.svm.get_sysvar::<Clock>().unix_timestamp;
    let ix = env.expansion_admin_ix(
        id,
        rod_pack_minter::instruction::SetPrice {
            expansion_id: id,
            price: 150 * BOOTY,
            discount_price: 100 * BOOTY,
            discount_start: now - 10,
            discount_end: now + 3600,
        }
        .data(),
    );
    env.run_admin(ix).unwrap();
    let meta = env.buy(id, 100 * BOOTY).expect("discounted buy");
    assert_eq!(pack_event(&meta).price, 100 * BOOTY);
    assert_eq!(token_amount(&env.svm, &env.shop_token), 100 * BOOTY);

    // A discount period in the past no longer applies.
    let ix = env.expansion_admin_ix(
        id,
        rod_pack_minter::instruction::SetPrice {
            expansion_id: id,
            price: 150 * BOOTY,
            discount_price: 100 * BOOTY,
            discount_start: now - 7200,
            discount_end: now - 3600,
        }
        .data(),
    );
    env.run_admin(ix).unwrap();
    assert_err(env.buy(id, 100 * BOOTY), "PriceAboveMax");
}

#[test]
fn sale_period_is_enforced() {
    let (mut env, id, _) = Env::ready();
    let now = env.svm.get_sysvar::<Clock>().unix_timestamp;
    let mut p = env.first_crush_params();
    p.sale_start = now + 3600;
    let ix = env.expansion_admin_ix(id, rod_pack_minter::instruction::UpdateExpansion { expansion_id: id, params: p }.data());
    env.run_admin(ix).unwrap();
    assert_err(env.buy(id, 150 * BOOTY), "OutsideSalePeriod");
}

#[test]
fn nothing_may_follow_buy_pack_except_compute_budget() {
    let (mut env, id, tree) = Env::ready();
    let buy = env.buy_ix(id, *b"order-0000000001", 150 * BOOTY, &env.approver.pubkey());
    let (player, approver) = (env.player.insecure_clone(), env.approver.insecure_clone());

    // A trailing transfer (stand-in for a "check the cards and fail" instruction) is rejected.
    let tail = system_instruction::transfer(&player.pubkey(), &Pubkey::new_unique(), 1_000_000);
    assert_err(send(&mut env.svm, &[buy.clone(), tail.clone()], &player, &[&approver]), "ForbiddenFollowingInstruction");
    assert_eq!(env.num_minted(&tree), 0, "nothing minted");
    assert_eq!(token_amount(&env.svm, &env.shop_token), 0, "nothing paid");

    // Instructions before it are fine, and compute-budget instructions after it are fine.
    send(&mut env.svm, &[tail, buy, compute_limit_ix(1_400_000)], &player, &[&approver]).expect("allowed layout");
    assert_eq!(env.num_minted(&tree), 6);
}

#[test]
fn admin_mint_picks_a_card_and_only_the_admin_can() {
    let (mut env, id, tree) = Env::ready();
    let friend = Pubkey::new_unique();
    let ix = env.admin_mint_ix(&env.admin.pubkey(), id, 50, 3, &friend);
    // More than the default 200k CU per transaction: callers set a compute limit, as for buy_pack.
    let admin = env.admin.insecure_clone();
    let meta = send(&mut env.svm, &[compute_limit_ix(600_000), ix], &admin, &[]).expect("admin_mint");
    println!("admin_mint (3 cards) compute units: {}", meta.compute_units_consumed);
    assert_eq!(env.num_minted(&tree), 3);

    let intruder = Keypair::new();
    env.svm.airdrop(&intruder.pubkey(), 10_000_000_000).unwrap();
    let ix = env.admin_mint_ix(&intruder.pubkey(), id, 50, 1, &intruder.pubkey());
    assert_err(send(&mut env.svm, &[ix], &intruder, &[]), "Unauthorized");
    // The approver key cannot either.
    let approver = env.approver.insecure_clone();
    let ix = env.admin_mint_ix(&approver.pubkey(), id, 50, 1, &approver.pubkey());
    assert_err(send(&mut env.svm, &[ix], &approver, &[]), "Unauthorized");
    assert_eq!(env.num_minted(&tree), 3);
}

#[test]
fn only_the_admin_changes_settings_and_cards() {
    let (mut env, id, _) = Env::ready();
    let intruder = Keypair::new();
    env.svm.airdrop(&intruder.pubkey(), 10_000_000_000).unwrap();
    let card = rod_pack_minter::CardInput { rarity: 2, name: "Fake".into(), uri_suffix: "fake.json".into() };
    let ix = env.upsert_ix(&intruder.pubkey(), id, 0, vec![card]);
    assert_err(send(&mut env.svm, &[ix], &intruder, &[]), "Unauthorized");

    let ix = Instruction::new_with_bytes(
        rod_pack_minter::ID,
        &rod_pack_minter::instruction::SetApprover { approver: intruder.pubkey() }.data(),
        anchor_lang::ToAccountMetas::to_account_metas(
            &rod_pack_minter::accounts::AdminOnly { admin: intruder.pubkey(), config: config_pda() },
            None,
        ),
    );
    assert_err(send(&mut env.svm, &[ix], &intruder, &[]), "Unauthorized");
    assert_eq!(env.config().approver, env.approver.pubkey());
}

#[test]
fn trees_are_made_by_admin_or_funder_and_rotate_when_full() {
    // Depth 3 = 8 leaves: one 6-card pack fits, a second does not.
    let (mut env, id, first) = Env::ready_with(Env::with_tree_size(3, 8, 0));
    env.buy(id, 150 * BOOTY).expect("first pack");
    set_slot_hash(&mut env.svm, 1001, [9u8; 32]);
    assert_err(env.buy(id, 150 * BOOTY), "TreeFull");
    assert_eq!(token_amount(&env.svm, &env.shop_token), 150 * BOOTY, "the failed pack charged nothing");

    let rotate = env.rotate_ix();
    let admin = env.admin.insecure_clone();
    assert_err(send(&mut env.svm, &[rotate], &admin, &[]), "NoStandbyTree");

    let intruder = Keypair::new();
    env.svm.airdrop(&intruder.pubkey(), 10_000_000_000).unwrap();
    assert_err(env.create_tree(&intruder), "Unauthorized");

    let funder = env.funder.insecure_clone();
    let standby = env.create_tree(&funder).expect("funder creates the standby tree");
    assert_eq!(env.config().standby_tree, standby);
    assert_err(env.create_tree(&funder), "StandbyExists");

    // Anyone may rotate once the active tree is too full; the backend puts it in front of buy_pack.
    let rotate = env.rotate_ix();
    let player = env.player.insecure_clone();
    let approver = env.approver.insecure_clone();
    let mut buy = env.buy_ix(id, *b"order-0000000002", 150 * BOOTY, &env.approver.pubkey());
    // buy_ix read the old active tree; point it at the standby tree, which rotate_tree promotes first.
    for m in buy.accounts.iter_mut() {
        if m.pubkey == first {
            m.pubkey = standby;
        } else if m.pubkey == bubblegum::tree_config_address(&first) {
            m.pubkey = bubblegum::tree_config_address(&standby);
        }
    }
    send(&mut env.svm, &[compute_limit_ix(1_400_000), rotate, buy], &player, &[&approver]).expect("rotate + buy");
    let c = env.config();
    assert_eq!(c.active_tree, standby);
    assert_eq!(c.standby_tree, Pubkey::default());
    assert_eq!(env.num_minted(&standby), 6);
    assert_eq!(c.trees_created, 2);
}

#[test]
fn rotate_refuses_while_the_active_tree_has_room() {
    let (mut env, _, _) = Env::ready();
    let funder = env.funder.insecure_clone();
    env.create_tree(&funder).unwrap();
    let rotate = env.rotate_ix();
    assert_err(send(&mut env.svm, &[rotate], &funder, &[]), "TreeNotFull");
}

#[test]
fn nobody_can_mint_into_a_program_tree_directly() {
    let (mut env, _, tree) = Env::ready();
    // Even the collection's update authority (who can verify cards into the collection) cannot use the tree:
    // its creator and delegate is the program's PDA.
    let owner = env.collection_owner.insecure_clone();
    let mut data = vec![153, 18, 178, 47, 197, 158, 86, 15];
    data.extend(bubblegum::metadata_args("Fake Epic", "", "https://x/fake.json", 0, &env.collection_mint, &owner.pubkey()));
    let ix = Instruction {
        program_id: BUBBLEGUM_ID,
        accounts: vec![
            AccountMeta::new(bubblegum::tree_config_address(&tree), false),
            AccountMeta::new_readonly(owner.pubkey(), false),
            AccountMeta::new_readonly(owner.pubkey(), false),
            AccountMeta::new(tree, false),
            AccountMeta::new_readonly(owner.pubkey(), true),
            AccountMeta::new_readonly(owner.pubkey(), true),
            AccountMeta::new_readonly(owner.pubkey(), true),
            AccountMeta::new_readonly(BUBBLEGUM_ID, false),
            AccountMeta::new_readonly(env.collection_mint, false),
            AccountMeta::new(metadata_pda(&env.collection_mint), false),
            AccountMeta::new_readonly(edition_pda(&env.collection_mint), false),
            AccountMeta::new_readonly(bubblegum::bubblegum_signer_address(), false),
            AccountMeta::new_readonly(NOOP_ID, false),
            AccountMeta::new_readonly(ACCOUNT_COMPRESSION_ID, false),
            AccountMeta::new_readonly(TOKEN_METADATA_ID, false),
            AccountMeta::new_readonly(system_program::ID, false),
        ],
        data,
    };
    let res = send(&mut env.svm, &[ix], &owner, &[]);
    assert!(res.is_err(), "direct mint must fail");
    println!("direct Bubblegum mint rejected: {}", res.unwrap_err().lines().find(|l| l.contains("Error")).unwrap_or(""));
    assert_eq!(env.num_minted(&tree), 0);
}
