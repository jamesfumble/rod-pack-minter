mod common;

use anchor_lang::{
    prelude::{Clock, Pubkey},
    solana_program::instruction::{AccountMeta, Instruction},
    solana_program::system_program,
    InstructionData, ToAccountMetas,
};
use common::*;
use rod_pack_minter::{
    bubblegum,
    constants::*,
    draw::{draw_pack, pack_seed},
};
use solana_keypair::Keypair;
use solana_signer::Signer;

/// The cards a pack must contain, recomputed off chain from public data (what anyone can verify).
fn expected_cards(env: &Env, slot_hash: &[u8; 32], order: &[u8; 16], purchase_slot: u64, id: u16) -> Vec<u16> {
    let seed = pack_seed(slot_hash, order, &env.player.pubkey().to_bytes(), purchase_slot, id);
    let e = env.expansion(id);
    // Test cards: ids 0-21 Common, 22-43 Rare, 44-65 Epic.
    let picks = draw_pack(&seed, e.pack_size, &e.weights_regular, &e.weights_guaranteed, &[22, 22, 22, 0]).unwrap();
    picks.into_iter().map(|(r, i)| r as u16 * 22 + i).collect()
}

#[test]
fn purchase_then_open_and_the_player_pays_every_fee() {
    let (mut env, id, tree) = Env::ready();
    let player = env.player.pubkey();
    let opener = env.opener.pubkey();
    let booty_before = token_amount(&env.svm, &env.player_token);
    let sol_before = env.svm.get_balance(&player).unwrap();

    let order = env.buy(id, 150 * BOOTY).expect("buy_pack");
    let purchase_slot = current_slot(&env.svm);
    let pending = env.pending(&order).expect("pending pack");
    assert_eq!((pending.price, pending.pack_size, pending.purchase_slot, pending.sealed), (150 * BOOTY, 6, purchase_slot, false));
    assert_eq!(token_amount(&env.svm, &env.player_token), booty_before - 150 * BOOTY);
    assert_eq!(token_amount(&env.svm, &vault_pda(&env.booty_mint)), 150 * BOOTY, "price held in the vault");
    assert_eq!(token_amount(&env.svm, &env.shop_token), 0);
    assert_eq!(env.num_minted(&tree), 0, "nothing minted yet");
    assert_eq!((env.config().pending_packs, env.config().reserved_leaves), (1, 6));

    // The cards do not exist yet: opening in the purchase slot fails.
    assert_err(env.open(&order, id), "TooEarlyToOpen");

    let opener_before = env.svm.get_balance(&opener).unwrap();
    let meta = env.next_slot_and_open(&order, id).expect("open_pack");
    println!("open_pack (6 cards) compute units: {}", meta.compute_units_consumed);

    assert_eq!(env.num_minted(&tree), 6);
    assert_eq!(token_amount(&env.svm, &vault_pda(&env.booty_mint)), 0);
    assert_eq!(token_amount(&env.svm, &env.shop_token), 150 * BOOTY, "price released to the shop");
    assert!(env.pending(&order).is_none(), "pending pack closed");
    let ev = pack_event(&meta);
    assert_eq!(ev.card_ids, expected_cards(&env, &slot_hash_of(purchase_slot), &order, purchase_slot, id));
    assert!(ev.card_ids[5] >= 22, "guaranteed slot drew card {}", ev.card_ids[5]);
    assert_eq!((ev.player, ev.tree, ev.first_leaf_index, ev.price), (player, tree, 0, 150 * BOOTY));
    let c = env.config();
    assert_eq!((c.packs_sold, c.cards_minted, c.pending_packs, c.reserved_leaves), (1, 6, 0, 0));

    // Fees: the player paid the buy transaction (2 signatures) and the open fee; the rent and the unused retry pool
    // came back.
    let player_spent = sol_before - env.svm.get_balance(&player).unwrap();
    assert_eq!(player_spent, 10_000 + OPEN_FEE);
    // The opener paid its 5,000-lamport fee and was repaid the deposit: it ends up ahead, not out of pocket.
    let opener_net = env.svm.get_balance(&opener).unwrap() as i64 - opener_before as i64;
    assert_eq!(opener_net, OPEN_FEE as i64 - 5_000);
    println!("player paid {player_spent} lamports in total; opener net +{opener_net}");
}

#[test]
fn the_cards_do_not_depend_on_who_opens_or_when() {
    let (mut env, id, _) = Env::ready();
    let order = env.buy(id, 150 * BOOTY).unwrap();
    let purchase_slot = current_slot(&env.svm);
    // Opened 200 slots later by a stranger: same cards as recomputed from the purchase slot.
    warp(&mut env.svm, purchase_slot + 200);
    let stranger = Keypair::new();
    env.svm.airdrop(&stranger.pubkey(), 1_000_000_000).unwrap();
    let meta = env.open_by(&stranger, &order, id).expect("anyone may open");
    assert_eq!(pack_event(&meta).card_ids, expected_cards(&env, &slot_hash_of(purchase_slot), &order, purchase_slot, id));
}

#[test]
fn the_player_can_open_their_own_pack() {
    let (mut env, id, tree) = Env::ready();
    let order = env.buy(id, 150 * BOOTY).unwrap();
    let s = current_slot(&env.svm);
    warp(&mut env.svm, s + 1);
    let player = env.player.insecure_clone();
    env.open_by(&player, &order, id).expect("player opens");
    assert_eq!(env.num_minted(&tree), 6);
}

#[test]
fn a_sealed_pack_keeps_its_cards_after_slot_hashes_expire() {
    let (mut env, id, _) = Env::ready();
    let order = env.buy(id, 150 * BOOTY).unwrap();
    let purchase_slot = current_slot(&env.svm);
    warp(&mut env.svm, purchase_slot + 1);
    let sealer = Keypair::new();
    env.svm.airdrop(&sealer.pubkey(), 1_000_000_000).unwrap();
    let before = env.svm.get_balance(&sealer.pubkey()).unwrap();
    let ix = env.seal_ix(&sealer.pubkey(), &order);
    send(&mut env.svm, &[ix], &sealer, &[]).expect("seal_pack");
    assert_eq!(env.svm.get_balance(&sealer.pubkey()).unwrap() - before + 5_000, RETRY_FEE, "sealer repaid from the retry pool");
    let ix = env.seal_ix(&sealer.pubkey(), &order);
    assert_err(send(&mut env.svm, &[ix], &sealer, &[]), "AlreadySealed");

    warp(&mut env.svm, purchase_slot + 2000); // far past the 512-slot window
    let meta = env.open(&order, id).expect("open a sealed pack later");
    assert_eq!(pack_event(&meta).card_ids, expected_cards(&env, &slot_hash_of(purchase_slot), &order, purchase_slot, id));
}

#[test]
fn a_pack_nobody_opened_in_time_is_redrawn_not_refunded() {
    let (mut env, id, tree) = Env::ready();
    let order = env.buy(id, 150 * BOOTY).unwrap();
    let purchase_slot = current_slot(&env.svm);
    let caller = Keypair::new();
    env.svm.airdrop(&caller.pubkey(), 1_000_000_000).unwrap();

    // While the purchase slot's hash is available, the pack must be opened, not redrawn.
    warp(&mut env.svm, purchase_slot + 1);
    let ix = env.redraw_ix(&caller.pubkey(), &order);
    assert_err(send(&mut env.svm, &[ix], &caller, &[]), "PackStillOpenable");

    // An outage: nobody opened or sealed it for 600 slots. Opening and sealing are no longer possible.
    warp(&mut env.svm, purchase_slot + 600);
    assert_err(env.open(&order, id), "SlotHashUnavailable");
    let ix = env.seal_ix(&caller.pubkey(), &order);
    assert_err(send(&mut env.svm, &[ix], &caller, &[]), "SlotHashUnavailable");

    // Anyone (normally the backend) redraws it; the caller is paid from the player's deposit.
    let before = env.svm.get_balance(&caller.pubkey()).unwrap();
    let ix = env.redraw_ix(&caller.pubkey(), &order);
    send(&mut env.svm, &[ix], &caller, &[]).expect("redraw_pack");
    assert_eq!(env.svm.get_balance(&caller.pubkey()).unwrap() + 5_000 - before, RETRY_FEE);
    let new_slot = current_slot(&env.svm);
    let p = env.pending(&order).unwrap();
    assert_eq!((p.purchase_slot, p.redraws, p.sealed), (new_slot, 1, false));
    assert_eq!(token_amount(&env.svm, &vault_pda(&env.booty_mint)), 150 * BOOTY, "the price never left the vault");

    // The new slot's hash does not exist yet; one slot later the pack opens with cards from it.
    assert_err(env.open(&order, id), "TooEarlyToOpen");
    let meta = env.next_slot_and_open(&order, id).expect("open after redraw");
    assert_eq!(pack_event(&meta).card_ids, expected_cards(&env, &slot_hash_of(new_slot), &order, new_slot, id));
    assert_eq!(env.num_minted(&tree), 6);
    assert_eq!(token_amount(&env.svm, &env.shop_token), 150 * BOOTY);
}

#[test]
fn the_retry_pool_pays_three_redraws_and_the_rest_returns_to_the_player() {
    let (mut env, id, _) = Env::ready();
    let sol_before = env.svm.get_balance(&env.player.pubkey()).unwrap();
    let order = env.buy(id, 150 * BOOTY).unwrap();
    assert_eq!(env.pending(&order).unwrap().retry_pool, RETRY_POOL);
    let caller = Keypair::new();
    env.svm.airdrop(&caller.pubkey(), 1_000_000_000).unwrap();
    // Four outages in a row: the first three redraws are paid from the pool, the fourth is not (pool empty).
    for round in 0..4u64 {
        let slot = env.pending(&order).unwrap().purchase_slot;
        warp(&mut env.svm, slot + 600);
        let before = env.svm.get_balance(&caller.pubkey()).unwrap();
        let ix = env.redraw_ix(&caller.pubkey(), &order);
        send(&mut env.svm, &[ix], &caller, &[]).expect("redraw");
        let paid = env.svm.get_balance(&caller.pubkey()).unwrap() + 5_000 - before;
        assert_eq!(paid, if round < 3 { RETRY_FEE } else { 0 }, "round {round}");
    }
    let p = env.pending(&order).unwrap();
    assert_eq!((p.redraws, p.retry_pool), (4, 0));
    env.next_slot_and_open(&order, id).expect("opens after four redraws");
    // The player paid the buy transaction, the open fee and the three paid redraws; nothing else.
    assert_eq!(sol_before - env.svm.get_balance(&env.player.pubkey()).unwrap(), 10_000 + OPEN_FEE + RETRY_POOL);
}

#[test]
fn a_sealed_pack_cannot_be_redrawn() {
    let (mut env, id, _) = Env::ready();
    let order = env.buy(id, 150 * BOOTY).unwrap();
    let s = current_slot(&env.svm);
    warp(&mut env.svm, s + 1);
    let opener = env.opener.insecure_clone();
    let ix = env.seal_ix(&opener.pubkey(), &order);
    send(&mut env.svm, &[ix], &opener, &[]).unwrap();
    warp(&mut env.svm, s + 2000);
    let ix = env.redraw_ix(&opener.pubkey(), &order);
    assert_err(send(&mut env.svm, &[ix], &opener, &[]), "AlreadySealed");
    env.open(&order, id).expect("the sealed pack still opens with its original cards");
}

#[test]
fn refund_is_admin_only_and_returns_price_and_rent() {
    let (mut env, id, _) = Env::ready();
    let booty_before = token_amount(&env.svm, &env.player_token);
    let sol_before = env.svm.get_balance(&env.player.pubkey()).unwrap();
    let order = env.buy(id, 150 * BOOTY).unwrap();

    // Too early, even for the admin.
    let ix = env.refund_ix(&env.admin.pubkey(), &order);
    assert_err(env.run_admin(ix), "TooEarlyToRefund");
    let now = env.now();
    env.set_time(now + REFUND_AFTER);

    // The player (or anyone else) cannot refund: they could otherwise return packs whose cards they dislike.
    let player = env.player.insecure_clone();
    let ix = env.refund_ix(&player.pubkey(), &order);
    assert_err(send(&mut env.svm, &[ix], &player, &[]), "Unauthorized");

    // The price must go to an account the player owns.
    let mut ix = env.refund_ix(&env.admin.pubkey(), &order);
    let other = Pubkey::new_unique();
    set_owned(&mut env.svm, other, TOKEN_PROGRAM, token_account_data(&env.booty_mint, &Pubkey::new_unique(), 0));
    for m in ix.accounts.iter_mut() {
        if m.pubkey == env.player_token {
            m.pubkey = other;
        }
    }
    assert!(env.run_admin(ix).is_err(), "refund to a stranger's account must fail");

    let admin_before = env.svm.get_balance(&env.admin.pubkey()).unwrap();
    let ix = env.refund_ix(&env.admin.pubkey(), &order);
    env.run_admin(ix).expect("admin refund");
    assert_eq!(token_amount(&env.svm, &env.player_token), booty_before, "price back");
    assert!(env.pending(&order).is_none());
    // The player paid the buy transaction, their own rejected refund attempt, and the open fee (now the admin's, for
    // sending the refund); the rent and the retry pool came back.
    assert_eq!(sol_before - env.svm.get_balance(&env.player.pubkey()).unwrap(), 10_000 + 5_000 + OPEN_FEE);
    assert_eq!(env.svm.get_balance(&env.admin.pubkey()).unwrap() + 5_000 - admin_before, OPEN_FEE);
    let c = env.config();
    assert_eq!((c.pending_packs, c.packs_refunded, c.reserved_leaves), (0, 1, 0));
    let s = current_slot(&env.svm);
    warp(&mut env.svm, s + 1);
    assert!(env.open(&order, id).is_err(), "a refunded pack cannot be opened");
}

#[test]
fn admin_edits_after_purchase_do_not_change_a_paid_pack() {
    let (mut env, id, _) = Env::ready();
    let order = env.buy(id, 150 * BOOTY).unwrap();
    let purchase_slot = current_slot(&env.svm);
    let expected = expected_cards(&env, &slot_hash_of(purchase_slot), &order, purchase_slot, id);
    // New weights and an extra Common card after the purchase.
    let mut p = env.first_crush_params();
    p.weights_regular = [1, 1, 98, 0];
    let ix = env.expansion_admin_ix(id, rod_pack_minter::instruction::UpdateExpansion { expansion_id: id, params: p }.data());
    env.run_admin(ix).unwrap();
    let card = rod_pack_minter::CardInput { rarity: 0, name: "Late Card".into(), uri_suffix: "late.json".into() };
    let ix = env.upsert_ix(&env.admin.pubkey(), id, 66, vec![card]);
    env.run_admin(ix).unwrap();
    let meta = env.next_slot_and_open(&order, id).unwrap();
    assert_eq!(pack_event(&meta).card_ids, expected, "drawn with the weights and cards frozen at purchase");
}

#[test]
fn a_cards_rarity_is_locked_and_undrawable_packs_are_not_sold() {
    let (mut env, id, _) = Env::ready();
    let card = rod_pack_minter::CardInput { rarity: 2, name: "Pool Palapa".into(), uri_suffix: "x.json".into() };
    let ix = env.upsert_ix(&env.admin.pubkey(), id, 0, vec![card]);
    assert_err(env.run_admin(ix), "RarityLocked");
    // Renaming keeps the rarity: allowed (metadata is mutable for future mints).
    let card = rod_pack_minter::CardInput { rarity: 0, name: "Renamed".into(), uri_suffix: "x.json".into() };
    let ix = env.upsert_ix(&env.admin.pubkey(), id, 0, vec![card]);
    env.run_admin(ix).unwrap();
    // Weight only on a rarity without cards: nothing could be drawn, so nothing is sold.
    let mut p = env.first_crush_params();
    p.weights_regular = [0, 0, 0, 10];
    let ix = env.expansion_admin_ix(id, rod_pack_minter::instruction::UpdateExpansion { expansion_id: id, params: p }.data());
    env.run_admin(ix).unwrap();
    assert_err(env.buy(id, 150 * BOOTY), "BadWeights");
}

#[test]
fn the_shop_account_can_be_replaced_but_never_with_the_vault() {
    let (mut env, id, _) = Env::ready();
    let order = env.buy(id, 150 * BOOTY).unwrap();
    let set = |env: &Env, dest: Pubkey| {
        Instruction::new_with_bytes(
            rod_pack_minter::ID,
            &rod_pack_minter::instruction::SetDestination {}.data(),
            rod_pack_minter::accounts::SetDestination { admin: env.admin.pubkey(), config: config_pda(), booty_destination: dest }.to_account_metas(None),
        )
    };
    let ix = set(&env, vault_pda(&env.booty_mint));
    assert_err(env.run_admin(ix), "VaultNotAllowed");
    let new_shop = Pubkey::new_unique();
    set_owned(&mut env.svm, new_shop, TOKEN_PROGRAM, token_account_data(&env.booty_mint, &Pubkey::new_unique(), 0));
    let ix = set(&env, new_shop);
    env.run_admin(ix).expect("replace while a pack is pending");
    env.shop_token = new_shop;
    env.next_slot_and_open(&order, id).unwrap();
    assert_eq!(token_amount(&env.svm, &new_shop), 150 * BOOTY);
}

#[test]
fn a_pack_opens_once() {
    let (mut env, id, tree) = Env::ready();
    let order = env.buy(id, 150 * BOOTY).unwrap();
    env.next_slot_and_open(&order, id).unwrap();
    assert!(env.open(&order, id).is_err());
    assert_eq!(env.num_minted(&tree), 6);
}

#[test]
fn an_order_id_cannot_be_reused() {
    let (mut env, id, _) = Env::ready();
    let order = env.new_order();
    let ix = env.buy_ix(id, order, 150 * BOOTY, &env.approver.pubkey());
    let (player, approver) = (env.player.insecure_clone(), env.approver.insecure_clone());
    send(&mut env.svm, &[ix.clone()], &player, &[&approver]).unwrap();
    assert_err(send(&mut env.svm, &[ix], &player, &[&approver]), "already in use");
}

#[test]
fn cards_go_to_the_buyer_only() {
    let (mut env, id, _) = Env::ready();
    let order = env.buy(id, 150 * BOOTY).unwrap();
    let s = current_slot(&env.svm);
    warp(&mut env.svm, s + 1);
    let mut ix = env.open_ix(&env.opener.pubkey(), &order, &env.player.pubkey(), id);
    let thief = Pubkey::new_unique();
    for m in ix.accounts.iter_mut() {
        if m.pubkey == env.player.pubkey() {
            m.pubkey = thief;
        }
    }
    let opener = env.opener.insecure_clone();
    assert_err(send(&mut env.svm, &[compute_limit_ix(1_400_000), ix], &opener, &[]), "WrongPlayer");
}

#[test]
fn ten_card_pack_fits_in_compute() {
    let (mut env, id, _) = Env::ready();
    let mut p = env.first_crush_params();
    p.pack_size = MAX_PACK_SIZE;
    let ix = env.expansion_admin_ix(id, rod_pack_minter::instruction::UpdateExpansion { expansion_id: id, params: p }.data());
    env.run_admin(ix).unwrap();
    let meta = env.buy_and_open(id, 150 * BOOTY).expect("10-card pack");
    println!("open_pack (10 cards) compute units: {}", meta.compute_units_consumed);
    assert!(meta.compute_units_consumed < 1_200_000);
    assert_eq!(pack_event(&meta).card_ids.len(), 10);
}

#[test]
fn a_full_size_card_table_fits_in_the_default_heap() {
    // MAX_CARDS (400) cards with maximum-length text (name 32, URI 128 + 72), opened without a heap request.
    let (mut env, _, _) = Env::ready();
    let mut p = env.first_crush_params();
    p.name = "Max Size".into();
    p.uri_prefix = "h".repeat(MAX_URI_PREFIX_LEN);
    p.pack_size = MAX_PACK_SIZE;
    let id = 2u16;
    let ix = Instruction::new_with_bytes(
        rod_pack_minter::ID,
        &rod_pack_minter::instruction::AddExpansion { params: p, price: BOOTY }.data(),
        rod_pack_minter::accounts::AddExpansion {
            admin: env.admin.pubkey(),
            config: config_pda(),
            expansion: expansion_pda(id),
            card_table: cards_pda(id),
            system_program: system_program::ID,
        }
        .to_account_metas(None),
    );
    env.run_admin(ix).unwrap();
    for start in (0..MAX_CARDS).step_by(5) {
        let cards: Vec<rod_pack_minter::CardInput> = (start..start + 5)
            .map(|i| rod_pack_minter::CardInput { rarity: (i % 3) as u8, name: format!("{i:0>32}"), uri_suffix: format!("{i:0>72}") })
            .collect();
        let ix = env.upsert_ix(&env.admin.pubkey(), id, start as u16, cards);
        env.run_admin(ix).expect("upsert");
    }
    let card = rod_pack_minter::CardInput { rarity: 0, name: "one too many".into(), uri_suffix: "x".into() };
    let ix = env.upsert_ix(&env.admin.pubkey(), id, MAX_CARDS as u16, vec![card]);
    assert_err(env.run_admin(ix), "TooManyCards");
    let ix = env.expansion_admin_ix(id, rod_pack_minter::instruction::SetExpansionPaused { expansion_id: id, paused: false }.data());
    env.run_admin(ix).unwrap();
    let meta = env.buy_and_open(id, BOOTY).expect("open with a full card table");
    println!("open_pack (10 cards, 400-card table, max text) compute units: {}", meta.compute_units_consumed);
}

#[test]
fn transactions_fit_with_a_lookup_table() {
    use solana_message::{v0, AddressLookupTableAccount, VersionedMessage};
    let (mut env, id, _) = Env::ready();
    let order = env.new_order();
    let buy = env.buy_ix(id, order, 150 * BOOTY, &env.approver.pubkey());
    let open = env.open_ix(&env.opener.pubkey(), &order, &env.player.pubkey(), id);
    let mut price = vec![3u8];
    price.extend_from_slice(&250_000u64.to_le_bytes());
    let cb = [compute_limit_ix(600_000), Instruction { program_id: COMPUTE_BUDGET_ID, accounts: vec![], data: price }];
    let signers = [env.player.pubkey(), env.approver.pubkey(), env.opener.pubkey(), env.player_token];
    for (name, payer, ixs) in [
        ("buy_pack", env.player.pubkey(), vec![cb[0].clone(), cb[1].clone(), buy]),
        ("rotate_tree + open_pack", env.opener.pubkey(), vec![cb[0].clone(), cb[1].clone(), env.rotate_ix(), open]),
    ] {
        let mut table: Vec<Pubkey> = Vec::new();
        for ix in &ixs {
            for m in ix.accounts.iter().map(|m| m.pubkey).chain(std::iter::once(ix.program_id)) {
                if !signers.contains(&m) && !table.contains(&m) {
                    table.push(m);
                }
            }
        }
        let alt = AddressLookupTableAccount { key: Pubkey::new_unique(), addresses: table.clone() };
        let msg = v0::Message::try_compile(&payer, &ixs, &[alt], Default::default()).unwrap();
        let sigs = msg.header.num_required_signatures as usize;
        let bytes = VersionedMessage::V0(msg).serialize().len() + 1 + sigs * 64;
        println!("{name}: {bytes} bytes");
        assert!(bytes <= 1232);
    }
    let _ = &mut env;
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
    assert_eq!(c.vault, vault_pda(&env.booty_mint));
}

#[test]
fn buy_needs_the_approver() {
    let (mut env, id, _) = Env::ready();
    let fake = Keypair::new();
    let order = env.new_order();
    let ix = env.buy_ix(id, order, 150 * BOOTY, &fake.pubkey());
    let player = env.player.insecure_clone();
    assert_err(send(&mut env.svm, &[ix], &player, &[&fake]), "WrongApprover");
    let ix = env.buy_ix(id, order, 150 * BOOTY, &env.player.pubkey());
    assert_err(send(&mut env.svm, &[ix], &player, &[]), "WrongApprover");
}

#[test]
fn pause_stops_sales_and_admin_mints_but_paid_packs_still_open() {
    let (mut env, id, tree) = Env::ready();
    let paid = env.buy(id, 150 * BOOTY).unwrap();
    env.set_paused(true).unwrap();
    assert_err(env.buy(id, 150 * BOOTY), "ProgramPaused");
    let ix = env.admin_mint_ix(&env.admin.pubkey(), id, 3, 1, &Pubkey::new_unique());
    assert_err(env.run_admin(ix), "ProgramPaused");
    env.next_slot_and_open(&paid, id).expect("a paid pack opens while paused");
    assert_eq!(env.num_minted(&tree), 6);
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
    let set = |env: &Env, discount_price: u64, start: i64, end: i64| {
        env.expansion_admin_ix(
            id,
            rod_pack_minter::instruction::SetPrice { expansion_id: id, price: 150 * BOOTY, discount_price, discount_start: start, discount_end: end }.data(),
        )
    };
    let ix = set(&env, 0, now - 10, now + 3600);
    assert_err(env.run_admin(ix), "ZeroDiscount");
    let ix = set(&env, 100 * BOOTY, now - 10, now + 3600);
    env.run_admin(ix).unwrap();
    let order = env.buy(id, 100 * BOOTY).expect("discounted buy");
    assert_eq!(env.pending(&order).unwrap().price, 100 * BOOTY);
    let ix = set(&env, 100 * BOOTY, now - 7200, now - 3600);
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
fn admin_mint_picks_a_card_and_only_the_admin_can() {
    let (mut env, id, tree) = Env::ready();
    let friend = Pubkey::new_unique();
    let ix = env.admin_mint_ix(&env.admin.pubkey(), id, 50, 3, &friend);
    let admin = env.admin.insecure_clone();
    let meta = send(&mut env.svm, &[compute_limit_ix(600_000), ix], &admin, &[]).expect("admin_mint");
    println!("admin_mint (3 cards) compute units: {}", meta.compute_units_consumed);
    assert_eq!(env.num_minted(&tree), 3);

    for k in [Keypair::new(), env.approver.insecure_clone(), env.opener.insecure_clone()] {
        env.svm.airdrop(&k.pubkey(), 10_000_000_000).unwrap();
        let ix = env.admin_mint_ix(&k.pubkey(), id, 50, 1, &k.pubkey());
        assert_err(send(&mut env.svm, &[compute_limit_ix(600_000), ix], &k, &[]), "Unauthorized");
    }
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
    for data in [
        rod_pack_minter::instruction::SetApprover { approver: intruder.pubkey() }.data(),
        rod_pack_minter::instruction::SetFees { open_fee_lamports: 0, retry_fee_lamports: 0, paid_retries: 0, refund_after_secs: 600 }.data(),
        rod_pack_minter::instruction::SetPaused { paused: true }.data(),
    ] {
        let ix = Instruction::new_with_bytes(
            rod_pack_minter::ID,
            &data,
            rod_pack_minter::accounts::AdminOnly { admin: intruder.pubkey(), config: config_pda() }.to_account_metas(None),
        );
        assert_err(send(&mut env.svm, &[ix], &intruder, &[]), "Unauthorized");
    }
    assert_eq!(env.config().approver, env.approver.pubkey());
}

#[test]
fn admin_safeguards() {
    let (mut env, id, _) = Env::ready();
    // Tree sizes outside what account compression supports.
    for (d, b, c) in [(2, 8, 0), (31, 64, 8), (14, 63, 8), (14, 64, 14)] {
        let ix = env.admin_ix(rod_pack_minter::instruction::SetTreeSize { max_depth: d, max_buffer: b, canopy_depth: c }.data());
        assert_err(env.run_admin(ix), "BadTreeAccount");
    }
    // Fees capped, refund delay at least 10 minutes.
    let ix = env.admin_ix(rod_pack_minter::instruction::SetFees { open_fee_lamports: MAX_FEE_LAMPORTS + 1, retry_fee_lamports: 0, paid_retries: 0, refund_after_secs: 600 }.data());
    assert_err(env.run_admin(ix), "FeeTooHigh");
    let ix = env.admin_ix(rod_pack_minter::instruction::SetFees { open_fee_lamports: 0, retry_fee_lamports: 0, paid_retries: MAX_PAID_RETRIES + 1, refund_after_secs: 600 }.data());
    assert_err(env.run_admin(ix), "FeeTooHigh");
    let ix = env.admin_ix(rod_pack_minter::instruction::SetFees { open_fee_lamports: 0, retry_fee_lamports: 0, paid_retries: 0, refund_after_secs: 60 }.data());
    assert_err(env.run_admin(ix), "TooEarlyToRefund");
    // The collection is locked once cards exist.
    env.buy_and_open(id, 150 * BOOTY).unwrap();
    let mut p = env.first_crush_params();
    p.collection_mint = Pubkey::new_unique();
    let ix = env.expansion_admin_ix(id, rod_pack_minter::instruction::UpdateExpansion { expansion_id: id, params: p }.data());
    assert_err(env.run_admin(ix), "CollectionLocked");
}

#[test]
fn payment_switch_waits_for_pending_packs() {
    let (mut env, id, _) = Env::ready();
    let order = env.buy(id, 150 * BOOTY).unwrap();
    let set_payment = |env: &Env| {
        Instruction::new_with_bytes(
            rod_pack_minter::ID,
            &rod_pack_minter::instruction::SetPayment {}.data(),
            rod_pack_minter::accounts::SetPayment {
                admin: env.admin.pubkey(),
                config: config_pda(),
                booty_mint: env.booty_mint,
                booty_destination: env.shop_token,
                vault: vault_pda(&env.booty_mint),
            }
            .to_account_metas(None),
        )
    };
    let ix = set_payment(&env);
    assert_err(env.run_admin(ix), "PacksPending");
    env.next_slot_and_open(&order, id).unwrap();
    let ix = set_payment(&env);
    env.run_admin(ix).expect("no pending packs");
}

#[test]
fn migrate_config_extends_a_v1_config() {
    let (mut env, _, _) = Env::ready();
    // Turn the config back into a v1 account (the layout devnet was first initialised with).
    let mut acc = env.svm.get_account(&config_pda()).unwrap();
    acc.data.truncate(rod_pack_minter::Config::V1_LEN);
    env.svm.set_account(config_pda(), acc).unwrap();
    let migrate = |env: &Env, admin: &Pubkey| {
        Instruction::new_with_bytes(
            rod_pack_minter::ID,
            &rod_pack_minter::instruction::MigrateConfig { open_fee_lamports: 7, retry_fee_lamports: 8, paid_retries: 2, refund_after_secs: 3600 }.data(),
            rod_pack_minter::accounts::MigrateConfig { admin: *admin, config: config_pda(), system_program: system_program::ID }.to_account_metas(None),
        )
    };
    let intruder = Keypair::new();
    env.svm.airdrop(&intruder.pubkey(), 1_000_000_000).unwrap();
    let ix = migrate(&env, &intruder.pubkey());
    assert_err(send(&mut env.svm, &[ix], &intruder, &[]), "Unauthorized");
    let ix = migrate(&env, &env.admin.pubkey());
    env.run_admin(ix).expect("migrate");
    let c = env.config();
    assert_eq!((c.open_fee_lamports, c.retry_fee_lamports, c.paid_retries, c.refund_after_secs, c.vault), (7, 8, 2, 3600, Pubkey::default()));
    assert_eq!(c.admin, env.admin.pubkey(), "v1 fields kept");
    let ix = migrate(&env, &env.admin.pubkey());
    assert_err(env.run_admin(ix), "AlreadyMigrated");
}

#[test]
fn trees_are_made_by_admin_or_funder_and_rotate_when_full() {
    // Depth 3 = 8 leaves: one 6-card pack fits, a second does not.
    let (mut env, id, first) = Env::ready_with(Env::with_tree_size(3, 8, 0));
    env.buy_and_open(id, 150 * BOOTY).expect("first pack");
    // Only 2 leaves left and no standby tree: the next pack is not sold at all (nothing is paid).
    let booty = token_amount(&env.svm, &env.player_token);
    assert_err(env.buy(id, 150 * BOOTY), "TreeFull");
    assert_eq!(token_amount(&env.svm, &env.player_token), booty);

    let rotate = env.rotate_ix();
    let admin = env.admin.insecure_clone();
    assert_err(send(&mut env.svm, &[rotate], &admin, &[]), "NoStandbyTree");
    let intruder = Keypair::new();
    env.svm.airdrop(&intruder.pubkey(), 10_000_000_000).unwrap();
    assert_err(env.create_tree(&intruder), "Unauthorized");
    let funder = env.funder.insecure_clone();
    let standby = env.create_tree(&funder).expect("funder creates the standby tree");
    assert_err(env.create_tree(&funder), "StandbyExists");
    // With the standby tree's room counted, the pack is sold.
    let order = env.buy(id, 150 * BOOTY).expect("sold once there is room");
    let s = current_slot(&env.svm);
    warp(&mut env.svm, s + 1);

    // The backend puts rotate_tree in front of open_pack.
    let rotate = env.rotate_ix();
    let mut open = env.open_ix(&env.opener.pubkey(), &order, &env.player.pubkey(), id);
    for m in open.accounts.iter_mut() {
        if m.pubkey == first {
            m.pubkey = standby;
        } else if m.pubkey == bubblegum::tree_config_address(&first) {
            m.pubkey = bubblegum::tree_config_address(&standby);
        }
    }
    let opener = env.opener.insecure_clone();
    send(&mut env.svm, &[compute_limit_ix(1_400_000), rotate, open], &opener, &[]).expect("rotate + open");
    let c = env.config();
    assert_eq!((c.active_tree, c.standby_tree, c.trees_created), (standby, Pubkey::default(), 2));
    assert_eq!(env.num_minted(&standby), 6);
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
    // Even the collection's update authority cannot use the tree: its creator and delegate is the program's PDA.
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
    assert!(send(&mut env.svm, &[ix], &owner, &[]).is_err(), "direct mint must fail");
    assert_eq!(env.num_minted(&tree), 0);
}
