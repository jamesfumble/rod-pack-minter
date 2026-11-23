// Devnet set-up and test commands for rod_pack_minter. TEST KEYS ONLY: they live in
// %USERPROFILE%\.config\rod-pack-minter\devnet\ (outside every repo) and are never printed. Public addresses of what
// this creates are written to deployments/devnet.json.
//
//   node scripts/devnet.mjs keys | fund | booty | collection | deploy <program-keypair-path> | init | expansion
//                           tree [admin|funder] | unpause | buy | admin-mint <cardId> <count> [recipient] | status
import { readFileSync, writeFileSync, existsSync, mkdirSync } from 'node:fs';
import { execFileSync } from 'node:child_process';
import { homedir } from 'node:os';
import { join, basename, dirname } from 'node:path';
import { fileURLToPath } from 'node:url';
import {
    Connection, Keypair, PublicKey, SystemProgram, Transaction, ComputeBudgetProgram, sendAndConfirmTransaction,
    TransactionInstruction, SYSVAR_RENT_PUBKEY, LAMPORTS_PER_SOL, TransactionMessage, VersionedTransaction, AddressLookupTableProgram,
} from '@solana/web3.js';
import {
    TOKEN_PROGRAM_ID, MINT_SIZE, createInitializeMint2Instruction, getAssociatedTokenAddressSync,
    createAssociatedTokenAccountIdempotentInstruction, createMintToInstruction, getMinimumBalanceForRentExemptMint,
} from '@solana/spl-token';
import * as c from './lib/client.mjs';

// RPM_NETWORK=surfnet + DEVNET_RPC_URL=http://127.0.0.1:18899 rehearses everything on a Surfpool forked from devnet.
const NETWORK = process.env.RPM_NETWORK || 'devnet';
const RPC = process.env.DEVNET_RPC_URL || 'https://api.devnet.solana.com';
const KEYS = join(homedir(), '.config', 'rod-pack-minter', 'devnet');
const ROOT = fileURLToPath(new URL('..', import.meta.url));
const STATE = join(ROOT, 'deployments', NETWORK + '.json');
const CARD_LIST = process.env.CARD_LIST || 'C:/Projects/first-crush-nft-collection/CardListStructureInfo.json';
const SOLANA = process.env.SOLANA_BIN || join(homedir(), '.local', 'share', 'solana', 'install', 'active_release', 'bin', process.platform === 'win32' ? 'solana.exe' : 'solana');
const BOOTY = 1_000_000_000n; // 9 decimals
const TREE = { depth: 14, buffer: 64, canopy: 8 };
const ROYALTY_CREATOR = 'GSqgkoPSWXgyovHvpqZgdW7SQ6P23dMH5dweWMNMLbGD';

const conn = new Connection(RPC, 'confirmed');
const key = (name) => Keypair.fromSecretKey(Uint8Array.from(JSON.parse(readFileSync(join(KEYS, name + '.json'), 'utf8'))));
const K = () => ({ admin: key('admin'), approver: key('approver'), funder: key('tree-funder'), owner: key('collection-owner'), player: key('player') });
const state = () => (existsSync(STATE) ? JSON.parse(readFileSync(STATE, 'utf8')) : {});
const save = (patch) => {
    mkdirSync(join(ROOT, 'deployments'), { recursive: true });
    const s = { ...state(), ...patch, network: NETWORK, programId: c.PROGRAM_ID.toBase58() };
    writeFileSync(STATE, JSON.stringify(s, null, 2) + '\n');
    return s;
};
const sol = async (pk) => (await conn.getBalance(new PublicKey(pk))) / LAMPORTS_PER_SOL;
const send = (ixs, payer, ...signers) => sendAndConfirmTransaction(conn, new Transaction().add(...ixs), [payer, ...signers], { commitment: 'confirmed' });
const explorer = (sig) => NETWORK === 'devnet' ? `https://explorer.solana.com/tx/${sig}?cluster=devnet` : sig;

const commands = {
    async keys() {
        for (const [n, k] of Object.entries(K())) console.log(n.padEnd(8), k.publicKey.toBase58(), (await sol(k.publicKey)).toFixed(4), 'SOL');
    },

    // Spread SOL from the admin to the other test keys.
    async fund() {
        const k = K();
        const want = { funder: 1.0, owner: 0.1, approver: 0.01, player: 0.1 };
        const ixs = [];
        for (const [n, amount] of Object.entries(want)) {
            const have = await sol(k[n].publicKey);
            if (have < amount) ixs.push(SystemProgram.transfer({ fromPubkey: k.admin.publicKey, toPubkey: k[n].publicKey, lamports: Math.round((amount - have) * LAMPORTS_PER_SOL) }));
        }
        if (ixs.length) console.log('fund', explorer(await send(ixs, k.admin)));
        await commands.keys();
    },

    // Test $BOOTY: mint authority = admin, the shop destination = admin's token account, the player gets 10,000.
    async booty() {
        const k = K();
        if (state().bootyMint) return console.log('booty exists', state().bootyMint);
        const mint = Keypair.generate();
        const shop = getAssociatedTokenAddressSync(mint.publicKey, k.admin.publicKey);
        const player = getAssociatedTokenAddressSync(mint.publicKey, k.player.publicKey);
        const sig = await send([
            SystemProgram.createAccount({ fromPubkey: k.admin.publicKey, newAccountPubkey: mint.publicKey, space: MINT_SIZE, lamports: await getMinimumBalanceForRentExemptMint(conn), programId: TOKEN_PROGRAM_ID }),
            createInitializeMint2Instruction(mint.publicKey, 9, k.admin.publicKey, null),
            createAssociatedTokenAccountIdempotentInstruction(k.admin.publicKey, shop, k.admin.publicKey, mint.publicKey),
            createAssociatedTokenAccountIdempotentInstruction(k.admin.publicKey, player, k.player.publicKey, mint.publicKey),
            createMintToInstruction(mint.publicKey, player, k.admin.publicKey, 10_000n * BOOTY),
        ], k.admin, mint);
        console.log('booty', explorer(sig));
        console.log(save({ bootyMint: mint.publicKey.toBase58(), bootyDestination: shop.toBase58(), playerBooty: player.toBase58() }));
    },

    // A sized Token Metadata collection NFT owned by the test collection owner, and the collection-authority record for
    // the program's mint authority (on mainnet CiAdmm... signs this one step for CARDZ...).
    async collection() {
        const k = K();
        const owner = k.owner.publicKey;
        let mintAddr = state().collectionMint;
        if (!mintAddr) {
            const mint = Keypair.generate();
            const holder = getAssociatedTokenAddressSync(mint.publicKey, owner);
            const metadata = c.metadataPda(mint.publicKey);
            const edition = c.editionPda(mint.publicKey);
            const str = (s) => { const b = Buffer.from(s); const l = Buffer.alloc(4); l.writeUInt32LE(b.length); return Buffer.concat([l, b]); };
            const mdData = Buffer.concat([Buffer.from([33]), str('Crypto Idolz Cards (devnet)'), str(''), str('https://f005.backblazeb2.com/file/cryptoidolz/cards/metadata/JGPY83YBVXNHWYDS1R.json'),
                Buffer.from([0, 0]), Buffer.from([0, 0, 0]), Buffer.from([1]), Buffer.from([1, 0]), Buffer.alloc(8)]);
            const createMd = new TransactionInstruction({ programId: c.TOKEN_METADATA_ID, data: mdData, keys: [
                { pubkey: metadata, isSigner: false, isWritable: true }, { pubkey: mint.publicKey, isSigner: false, isWritable: false },
                { pubkey: owner, isSigner: true, isWritable: false }, { pubkey: owner, isSigner: true, isWritable: true },
                { pubkey: owner, isSigner: true, isWritable: false }, { pubkey: SystemProgram.programId, isSigner: false, isWritable: false },
                { pubkey: SYSVAR_RENT_PUBKEY, isSigner: false, isWritable: false }] });
            const meData = Buffer.concat([Buffer.from([17, 1]), Buffer.alloc(8)]);
            const createMe = new TransactionInstruction({ programId: c.TOKEN_METADATA_ID, data: meData, keys: [
                { pubkey: edition, isSigner: false, isWritable: true }, { pubkey: mint.publicKey, isSigner: false, isWritable: true },
                { pubkey: owner, isSigner: true, isWritable: false }, { pubkey: owner, isSigner: true, isWritable: false },
                { pubkey: owner, isSigner: true, isWritable: true }, { pubkey: metadata, isSigner: false, isWritable: true },
                { pubkey: TOKEN_PROGRAM_ID, isSigner: false, isWritable: false }, { pubkey: SystemProgram.programId, isSigner: false, isWritable: false },
                { pubkey: SYSVAR_RENT_PUBKEY, isSigner: false, isWritable: false }] });
            const sig = await send([
                SystemProgram.createAccount({ fromPubkey: owner, newAccountPubkey: mint.publicKey, space: MINT_SIZE, lamports: await getMinimumBalanceForRentExemptMint(conn), programId: TOKEN_PROGRAM_ID }),
                createInitializeMint2Instruction(mint.publicKey, 0, owner, owner),
                createAssociatedTokenAccountIdempotentInstruction(owner, holder, owner, mint.publicKey),
                createMintToInstruction(mint.publicKey, holder, owner, 1n),
                createMd, createMe,
            ], k.owner, mint);
            console.log('collection', explorer(sig));
            mintAddr = mint.publicKey.toBase58();
            save({ collectionMint: mintAddr });
        }
        const authority = c.mintAuthorityPda();
        const record = c.collectionRecordPda(mintAddr, authority);
        if (await conn.getAccountInfo(record)) return console.log('collection authority record exists', record.toBase58());
        const approve = new TransactionInstruction({ programId: c.TOKEN_METADATA_ID, data: Buffer.from([23]), keys: [
            { pubkey: record, isSigner: false, isWritable: true }, { pubkey: authority, isSigner: false, isWritable: false },
            { pubkey: owner, isSigner: true, isWritable: true }, { pubkey: owner, isSigner: true, isWritable: true },
            { pubkey: c.metadataPda(mintAddr), isSigner: false, isWritable: false }, { pubkey: new PublicKey(mintAddr), isSigner: false, isWritable: false },
            { pubkey: SystemProgram.programId, isSigner: false, isWritable: false }, { pubkey: SYSVAR_RENT_PUBKEY, isSigner: false, isWritable: false }] });
        console.log('approve collection authority', explorer(await send([approve], k.owner)));
        console.log(save({ collectionAuthorityRecord: record.toBase58() }));
    },

    // First deploy. The program keypair is passed by path to the Solana CLI and never read here.
    async deploy(programKeypairPath) {
        if (!programKeypairPath) throw new Error('usage: deploy <program-keypair-path>');
        const so = join(ROOT, 'target', 'deploy', 'rod_pack_minter.so');
        const adminPath = join(KEYS, 'admin.json');
        // The Solana CLI rejects key paths that contain spaces ("unrecognized signer source"), so run it from the key's
        // folder and pass the bare file name.
        execFileSync(SOLANA, ['program', 'deploy', so, '--program-id', basename(programKeypairPath), '--upgrade-authority', adminPath,
            '--keypair', adminPath, '--url', RPC, '--with-compute-unit-price', '10000', '--max-sign-attempts', '30'], { stdio: 'inherit', cwd: dirname(programKeypairPath) });
        save({ deployedAt: new Date().toISOString() });
    },

    async init() {
        const k = K(); const s = state();
        if (await conn.getAccountInfo(c.configPda())) return console.log('already initialised');
        const i = c.ix('initialize', { approver: k.approver.publicKey, tree_funder: k.funder.publicKey, tree_max_depth: TREE.depth, tree_max_buffer: TREE.buffer, tree_canopy_depth: TREE.canopy }, {
            admin: k.admin.publicKey, config: c.configPda(), mint_authority: c.mintAuthorityPda(), program: c.PROGRAM_ID, program_data: c.programDataPda(),
            booty_mint: s.bootyMint, booty_destination: s.bootyDestination, system_program: SystemProgram.programId });
        console.log('initialize', explorer(await send([i], k.admin)));
        console.log(save({ config: c.configPda().toBase58(), mintAuthority: c.mintAuthorityPda().toBase58() }));
    },

    // First Crush from the card list: rarity Common 0, Rare 1, Epic 2; URI prefix shared, suffix per card.
    async expansion() {
        const k = K(); const s = state();
        const id = 1;
        const list = JSON.parse(readFileSync(CARD_LIST, 'utf8'));
        const RARITY = { Common: 0, Rare: 1, Epic: 2 };
        const prefix = 'https://f005.backblazeb2.com/file/cryptoidolz/cards/metadata/';
        const cards = list.map((e) => {
            if (!e.JsonURL.startsWith(prefix)) throw new Error('unexpected URI ' + e.JsonURL);
            if (!(e.Card.Rarity in RARITY)) throw new Error('unexpected rarity ' + e.Card.Rarity);
            return { rarity: RARITY[e.Card.Rarity], name: e.Card.Name, uri_suffix: e.JsonURL.slice(prefix.length) };
        });
        if (!(await conn.getAccountInfo(c.expansionPda(id)))) {
            const params = { name: 'First Crush', collection_mint: s.collectionMint, creator: ROYALTY_CREATOR, royalty_bps: 690, symbol: '', uri_prefix: prefix,
                pack_size: 6, weights_regular: [70, 25, 5, 0], weights_guaranteed: [0, 25, 5, 0], sale_start: 0, sale_end: 0 };
            const i = c.ix('add_expansion', { params, price: 150n * BOOTY }, { admin: k.admin.publicKey, config: c.configPda(), expansion: c.expansionPda(id), card_table: c.cardsPda(id), system_program: SystemProgram.programId });
            console.log('add_expansion', explorer(await send([i], k.admin)));
        }
        const table = c.decodeCardTable((await conn.getAccountInfo(c.cardsPda(id))).data);
        for (let start = table.cards.length; start < cards.length; start += 8) {
            const chunk = cards.slice(start, start + 8);
            const i = c.ix('upsert_cards', { expansion_id: id, start_index: start, cards: chunk }, { admin: k.admin.publicKey, config: c.configPda(), expansion: c.expansionPda(id), card_table: c.cardsPda(id), system_program: SystemProgram.programId });
            console.log(`upsert_cards ${start}-${start + chunk.length - 1}`, explorer(await send([i], k.admin)));
        }
        const after = c.decodeCardTable((await conn.getAccountInfo(c.cardsPda(id))).data);
        const same = after.cards.length === cards.length && after.cards.every((x, i) => x.name === cards[i].name && x.uriSuffix === cards[i].uri_suffix && x.rarity === cards[i].rarity);
        console.log(`card table: ${after.cards.length} cards, matches the card list: ${same}`);
        const e = c.decodeExpansion((await conn.getAccountInfo(c.expansionPda(id))).data);
        if (e.paused) {
            const i = c.ix('set_expansion_paused', { expansion_id: id, paused: false }, { admin: k.admin.publicKey, config: c.configPda(), expansion: c.expansionPda(id) });
            console.log('unpause expansion', explorer(await send([i], k.admin)));
        }
        save({ expansions: { first_crush: { id, expansion: c.expansionPda(id).toBase58(), cardTable: c.cardsPda(id).toBase58() } } });
    },

    async tree(who = 'funder') {
        const k = K();
        const signer = who === 'admin' ? k.admin : k.funder;
        const cfg = c.decodeConfig((await conn.getAccountInfo(c.configPda())).data);
        const tree = Keypair.generate();
        const size = c.merkleTreeSize(cfg.treeMaxDepth, cfg.treeMaxBuffer, cfg.treeCanopyDepth);
        const lamports = await conn.getMinimumBalanceForRentExemption(size);
        const alloc = SystemProgram.createAccount({ fromPubkey: signer.publicKey, newAccountPubkey: tree.publicKey, space: size, lamports, programId: c.COMPRESSION_ID });
        const i = c.ix('create_tree', {}, { authority: signer.publicKey, config: c.configPda(), merkle_tree: tree.publicKey, tree_config: c.treeConfigPda(tree.publicKey),
            mint_authority: c.mintAuthorityPda(), system_program: SystemProgram.programId, ...c.eventAccounts() });
        console.log(`create_tree (${size} bytes, ${lamports / LAMPORTS_PER_SOL} SOL)`, explorer(await send([alloc, i], signer, tree)));
        const trees = state().trees ?? [];
        save({ trees: [...trees, tree.publicKey.toBase58()] });
        await commands.status();
    },

    async unpause() {
        const k = K();
        const i = c.ix('set_paused', { paused: false }, { admin: k.admin.publicKey, config: c.configPda() });
        console.log('unpause', explorer(await send([i], k.admin)));
    },

    // One pack as the test player, co-signed by the approver: v0 transaction with a lookup table (as the backend will).
    async buy() {
        const k = K(); const s = state();
        const cfg = c.decodeConfig((await conn.getAccountInfo(c.configPda())).data);
        const id = 1;
        const orderId = Buffer.alloc(16); Buffer.from('devnet-' + Date.now().toString(36)).copy(orderId);
        const ixs = [ComputeBudgetProgram.setComputeUnitLimit({ units: 600_000 }), ComputeBudgetProgram.setComputeUnitPrice({ microLamports: 10_000 })];
        let tree = cfg.activeTree;
        const tc = c.treeConfigInfo((await conn.getAccountInfo(c.treeConfigPda(tree))).data);
        if (tc.capacity - tc.numMinted < 10n && !cfg.standbyTree.equals(PublicKey.default)) {
            ixs.push(c.ix('rotate_tree', {}, { config: c.configPda(), active_tree_config: c.treeConfigPda(tree), ...c.eventAccounts() }));
            tree = cfg.standbyTree;
        }
        const exp = c.decodeExpansion((await conn.getAccountInfo(c.expansionPda(id))).data);
        ixs.push(c.ix('buy_pack', { expansion_id: id, order_id: [...orderId], max_price: 150n * BOOTY }, {
            player: k.player.publicKey, approver: k.approver.publicKey, config: c.configPda(), expansion: c.expansionPda(id), card_table: c.cardsPda(id),
            player_token: s.playerBooty, booty_destination: cfg.bootyDestination, booty_mint: cfg.bootyMint, token_program: TOKEN_PROGRAM_ID,
            ...c.mintAccounts(tree, exp.collectionMint), ...c.eventAccounts() }));
        const lut = await ensureLookupTable(k.admin, ixs, [k.player.publicKey, k.approver.publicKey, new PublicKey(s.playerBooty)]);
        const { blockhash, lastValidBlockHeight } = await conn.getLatestBlockhash();
        const msg = new TransactionMessage({ payerKey: k.player.publicKey, recentBlockhash: blockhash, instructions: ixs }).compileToV0Message([lut]);
        const tx = new VersionedTransaction(msg);
        tx.sign([k.player]); // wallet first
        tx.sign([k.approver]); // then the backend approver, which sends it
        console.log('transaction size', tx.serialize().length, 'bytes');
        const sig = await conn.sendTransaction(tx, { maxRetries: 5 });
        await conn.confirmTransaction({ signature: sig, blockhash, lastValidBlockHeight }, 'confirmed');
        console.log('buy_pack', explorer(sig));
        await showPack(sig, exp);
    },

    async 'admin-mint'(cardId, count = '1', recipient) {
        const k = K();
        const cfg = c.decodeConfig((await conn.getAccountInfo(c.configPda())).data);
        const id = 1;
        const exp = c.decodeExpansion((await conn.getAccountInfo(c.expansionPda(id))).data);
        const i = c.ix('admin_mint', { expansion_id: id, card_id: Number(cardId), count: Number(count) }, {
            admin: k.admin.publicKey, config: c.configPda(), expansion: c.expansionPda(id), card_table: c.cardsPda(id),
            recipient: recipient || k.player.publicKey, ...c.mintAccounts(cfg.activeTree, exp.collectionMint), ...c.eventAccounts() });
        const sig = await send([ComputeBudgetProgram.setComputeUnitLimit({ units: 100_000 + 90_000 * Number(count) }), i], k.admin);
        console.log('admin_mint', explorer(sig));
    },

    async status() {
        const info = await conn.getAccountInfo(c.configPda());
        if (!info) return console.log('not initialised');
        const cfg = c.decodeConfig(info.data);
        console.log({ admin: cfg.admin.toBase58(), approver: cfg.approver.toBase58(), treeFunder: cfg.treeFunder.toBase58(), paused: cfg.paused,
            activeTree: cfg.activeTree.toBase58(), standbyTree: cfg.standbyTree.toBase58(), treesCreated: cfg.treesCreated, packsSold: cfg.packsSold.toString(), cardsMinted: cfg.cardsMinted.toString() });
        for (const t of [cfg.activeTree, cfg.standbyTree]) {
            if (t.equals(PublicKey.default)) continue;
            const tc = c.treeConfigInfo((await conn.getAccountInfo(c.treeConfigPda(t))).data);
            console.log(`tree ${t.toBase58()}: ${tc.numMinted}/${tc.capacity} minted, creator=delegate=mint authority: ${tc.creator.equals(c.mintAuthorityPda()) && tc.delegate.equals(c.mintAuthorityPda())}`);
        }
    },
};

async function ensureLookupTable(authority, ixs, exclude) {
    const addresses = [];
    for (const i of ixs) for (const a of [i.programId, ...i.keys.map((m) => m.pubkey)]) {
        if (!exclude.some((e) => e.equals(a)) && !addresses.some((x) => x.equals(a))) addresses.push(a);
    }
    let s = state();
    if (s.lookupTable) {
        const lut = (await conn.getAddressLookupTable(new PublicKey(s.lookupTable))).value;
        const missing = addresses.filter((a) => !lut.state.addresses.some((x) => x.equals(a)));
        if (!missing.length) return lut;
        const ext = AddressLookupTableProgram.extendLookupTable({ lookupTable: lut.key, authority: authority.publicKey, payer: authority.publicKey, addresses: missing });
        console.log('extend lookup table', explorer(await send([ext], authority)));
    } else {
        const slot = await conn.getSlot('finalized');
        const [create, lutKey] = AddressLookupTableProgram.createLookupTable({ authority: authority.publicKey, payer: authority.publicKey, recentSlot: slot });
        const ext = AddressLookupTableProgram.extendLookupTable({ lookupTable: lutKey, authority: authority.publicKey, payer: authority.publicKey, addresses });
        console.log('create lookup table', explorer(await send([create, ext], authority)));
        save({ lookupTable: lutKey.toBase58() });
    }
    await new Promise((r) => setTimeout(r, 1500)); // a table is usable one slot after its last extension
    return (await conn.getAddressLookupTable(new PublicKey(state().lookupTable))).value;
}

async function showPack(sig, exp) {
    const tx = await conn.getTransaction(sig, { commitment: 'confirmed', maxSupportedTransactionVersion: 0 });
    const table = c.decodeCardTable((await conn.getAccountInfo(c.cardsPda(1))).data);
    console.log('compute units', tx.meta.computeUnitsConsumed);
    for (const ev of c.decodeEvents(tx)) {
        if (ev.name !== 'PackMinted') continue;
        const p = c.decodePackMinted(ev.data);
        const R = ['Common', 'Rare', 'Epic', 'Legendary'];
        console.log(`pack: tree ${p.tree.toBase58()}, leaves ${p.firstLeafIndex}..${p.firstLeafIndex + BigInt(p.cardIds.length) - 1n}, price ${p.price / BOOTY} BOOTY`);
        p.cardIds.forEach((id, i) => {
            const card = table.cards[id];
            console.log(`  ${i + 1}. #${id} ${card.name} (${R[card.rarity]})  asset ${c.assetId(p.tree, p.firstLeafIndex + BigInt(i)).toBase58()}`);
        });
    }
}

const [cmd, ...args] = process.argv.slice(2);
if (!commands[cmd]) { console.log('commands:', Object.keys(commands).join(', ')); process.exit(1); }
commands[cmd](...args).catch((e) => { console.error(e.logs ? e.logs.join('\n') : ''); console.error(e.message || e); process.exit(1); });
