// Minimal IDL-driven client for rod_pack_minter: builds instructions from idl/rod_pack_minter.json (discriminators,
// account order and flags, Borsh args). Used by the devnet scripts; the backend will use a generated client later.
import { readFileSync } from 'node:fs';
import { fileURLToPath } from 'node:url';
import { PublicKey, TransactionInstruction } from '@solana/web3.js';

const here = fileURLToPath(new URL('.', import.meta.url));
export const IDL = JSON.parse(readFileSync(here + '../../idl/rod_pack_minter.json', 'utf8'));
export const PROGRAM_ID = new PublicKey(IDL.address);

export const BUBBLEGUM_ID = new PublicKey('BGUMAp9Gq7iTEuizy4pqaxsTyUCBK68MDfK752saRPUY');
export const COMPRESSION_ID = new PublicKey('cmtDvXumGCrqC1Age74AVPhSRVXJMd8PJS91L8KbNCK');
export const NOOP_ID = new PublicKey('noopb9bkMVfRPU8AsbpTUg8AQkHtKwMYZiFUjNRtMmV');
export const TOKEN_METADATA_ID = new PublicKey('metaqbxxUerdq28cj1RbAWkYQm3ybzjb6a8bt518x1s');
export const BPF_UPGRADEABLE_ID = new PublicKey('BPFLoaderUpgradeab1e11111111111111111111111');

const le = (n, bytes) => {
    const b = Buffer.alloc(bytes);
    let v = BigInt(n);
    if (v < 0n) v += 1n << BigInt(bytes * 8);
    for (let i = 0; i < bytes; i++) { b[i] = Number(v & 0xffn); v >>= 8n; }
    return b;
};
const SIZES = { u8: 1, i8: 1, u16: 2, i16: 2, u32: 4, i32: 4, u64: 8, i64: 8, u128: 16, i128: 16 };

function encode(type, value) {
    if (typeof type === 'string') {
        if (SIZES[type]) return le(value, SIZES[type]);
        if (type === 'bool') return Buffer.from([value ? 1 : 0]);
        if (type === 'pubkey') return new PublicKey(value).toBuffer();
        if (type === 'string') { const s = Buffer.from(value, 'utf8'); return Buffer.concat([le(s.length, 4), s]); }
        if (type === 'bytes') { const s = Buffer.from(value); return Buffer.concat([le(s.length, 4), s]); }
        throw new Error('unsupported type ' + type);
    }
    if (type.array) {
        const [inner, len] = type.array;
        if (value.length !== len) throw new Error(`array length ${value.length} != ${len}`);
        return Buffer.concat([...value].map((v) => encode(inner, v)));
    }
    if (type.vec) return Buffer.concat([le(value.length, 4), ...value.map((v) => encode(type.vec, v))]);
    if (type.option) return value == null ? Buffer.from([0]) : Buffer.concat([Buffer.from([1]), encode(type.option, value)]);
    if (type.defined) {
        const def = IDL.types.find((t) => t.name === (type.defined.name || type.defined));
        if (!def || def.type.kind !== 'struct') throw new Error('unsupported defined type ' + JSON.stringify(type));
        return Buffer.concat(def.type.fields.map((f) => encode(f.type, value[f.name])));
    }
    throw new Error('unsupported type ' + JSON.stringify(type));
}

function flatten(accounts, prefix = '') {
    const out = [];
    for (const a of accounts) {
        const name = prefix + a.name;
        if (a.accounts) out.push(...flatten(a.accounts, name + '.'));
        else out.push({ name, writable: !!a.writable, signer: !!a.signer, address: a.address });
    }
    return out;
}

/** Builds an instruction. `accounts` maps account names (groups as "mint.tree_config") to addresses. */
export function ix(name, args = {}, accounts = {}) {
    const def = IDL.instructions.find((i) => i.name === name);
    if (!def) throw new Error('unknown instruction ' + name);
    const data = Buffer.concat([Buffer.from(def.discriminator), ...def.args.map((a) => encode(a.type, args[a.name]))]);
    const keys = flatten(def.accounts).map((a) => {
        const address = accounts[a.name] ?? a.address;
        if (!address) throw new Error(`${name}: missing account ${a.name}`);
        return { pubkey: new PublicKey(address), isSigner: a.signer, isWritable: a.writable };
    });
    return new TransactionInstruction({ programId: PROGRAM_ID, keys, data });
}

export const pda = (seeds, program = PROGRAM_ID) => PublicKey.findProgramAddressSync(seeds, program)[0];
export const u16le = (n) => le(n, 2);
export const configPda = () => pda([Buffer.from('config')]);
export const expansionPda = (id) => pda([Buffer.from('expansion'), u16le(id)]);
export const cardsPda = (id) => pda([Buffer.from('cards'), u16le(id)]);
export const mintAuthorityPda = () => pda([Buffer.from('mint_authority')]);
export const vaultPda = (mint) => pda([Buffer.from('vault'), new PublicKey(mint).toBuffer()]);
export const pendingPda = (player, orderId) => pda([Buffer.from('pack'), new PublicKey(player).toBuffer(), Buffer.from(orderId)]);
export const eventAuthorityPda = () => pda([Buffer.from('__event_authority')]);
export const programDataPda = () => pda([PROGRAM_ID.toBuffer()], BPF_UPGRADEABLE_ID);
export const treeConfigPda = (tree) => pda([new PublicKey(tree).toBuffer()], BUBBLEGUM_ID);
export const bubblegumSignerPda = () => pda([Buffer.from('collection_cpi')], BUBBLEGUM_ID);
export const metadataPda = (mint) => pda([Buffer.from('metadata'), TOKEN_METADATA_ID.toBuffer(), new PublicKey(mint).toBuffer()], TOKEN_METADATA_ID);
export const editionPda = (mint) => pda([Buffer.from('metadata'), TOKEN_METADATA_ID.toBuffer(), new PublicKey(mint).toBuffer(), Buffer.from('edition')], TOKEN_METADATA_ID);
export const collectionRecordPda = (mint, authority) =>
    pda([Buffer.from('metadata'), TOKEN_METADATA_ID.toBuffer(), new PublicKey(mint).toBuffer(), Buffer.from('collection_authority'), new PublicKey(authority).toBuffer()], TOKEN_METADATA_ID);

export function merkleTreeSize(depth, buffer, canopy) {
    const path = 40 + 32 * depth;
    return 56 + 24 + buffer * path + path + ((1 << (canopy + 1)) - 2) * 32;
}

/** The mint account group for buy_pack / admin_mint. */
export function mintAccounts(tree, collectionMint) {
    const authority = mintAuthorityPda();
    return {
        'mint.mint_authority': authority,
        'mint.tree_config': treeConfigPda(tree),
        'mint.merkle_tree': tree,
        'mint.collection_mint': collectionMint,
        'mint.collection_metadata': metadataPda(collectionMint),
        'mint.collection_edition': editionPda(collectionMint),
        'mint.collection_authority_record': collectionRecordPda(collectionMint, authority),
        'mint.bubblegum_signer': bubblegumSignerPda(),
    };
}

export const eventAccounts = () => ({ event_authority: eventAuthorityPda(), program: PROGRAM_ID });

// ---- account decoding (Borsh, fixed layouts from state.rs) ----
class Reader {
    constructor(buf) { this.b = buf; this.o = 8; }
    u8() { return this.b[this.o++]; }
    bool() { return this.u8() === 1; }
    u16() { const v = this.b.readUInt16LE(this.o); this.o += 2; return v; }
    u32() { const v = this.b.readUInt32LE(this.o); this.o += 4; return v; }
    u64() { const v = this.b.readBigUInt64LE(this.o); this.o += 8; return v; }
    i64() { const v = this.b.readBigInt64LE(this.o); this.o += 8; return v; }
    key() { const v = new PublicKey(this.b.subarray(this.o, this.o + 32)); this.o += 32; return v; }
    str() { const n = this.u32(); const v = this.b.subarray(this.o, this.o + n).toString('utf8'); this.o += n; return v; }
}

export function decodeConfig(data) {
    const r = new Reader(data);
    return {
        admin: r.key(), pendingAdmin: r.key(), approver: r.key(), treeFunder: r.key(), paused: r.bool(),
        bootyMint: r.key(), bootyDestination: r.key(), activeTree: r.key(), standbyTree: r.key(),
        treeMaxDepth: r.u32(), treeMaxBuffer: r.u32(), treeCanopyDepth: r.u32(), treesCreated: r.u32(),
        nextExpansionId: r.u16(), packsSold: r.u64(), cardsMinted: r.u64(), bump: r.u8(), mintAuthorityBump: r.u8(),
        ...(data.length > 301 ? { vault: r.key(), openFee: r.u64(), retryFee: r.u64(), refundAfterSecs: r.i64(), pendingPacks: r.u64(), packsRefunded: r.u64(), reservedLeaves: r.u64(), paidRetries: r.u8() } : { v1: true }),
    };
}

export function decodePendingPack(data) {
    const r = new Reader(data);
    return {
        player: r.key(), expansionId: r.u16(), orderId: Buffer.from(r.b.subarray(r.o, (r.o += 16))), price: r.u64(), packSize: r.u8(),
        purchaseSlot: r.u64(), purchasedAt: r.i64(), sealed: r.bool(), slotHash: Buffer.from(r.b.subarray(r.o, (r.o += 32))), openFee: r.u64(), retryFee: r.u64(),
        bump: r.u8(), weightsRegular: [r.u16(), r.u16(), r.u16(), r.u16()], weightsGuaranteed: [r.u16(), r.u16(), r.u16(), r.u16()],
        rarityCounts: [r.u16(), r.u16(), r.u16(), r.u16()], redraws: r.u8(), retryPool: r.u64(),
    };
}

export function decodeExpansion(data) {
    const r = new Reader(data);
    return {
        id: r.u16(), paused: r.bool(), name: r.str(), collectionMint: r.key(), creator: r.key(), royaltyBps: r.u16(),
        symbol: r.str(), uriPrefix: r.str(), packSize: r.u8(),
        weightsRegular: [r.u16(), r.u16(), r.u16(), r.u16()], weightsGuaranteed: [r.u16(), r.u16(), r.u16(), r.u16()],
        price: r.u64(), discountPrice: r.u64(), discountStart: r.i64(), discountEnd: r.i64(), saleStart: r.i64(), saleEnd: r.i64(),
        packsSold: r.u64(), cardsMinted: r.u64(),
    };
}

// Header (disc 8 | expansion_id u16 | bump u8 | count u16), then fixed 107-byte entries:
// rarity u8 | name_len u8 | name [32] | suffix_len u8 | uri_suffix [72].
export const CARD_HEADER_LEN = 13;
export const CARD_ENTRY_LEN = 107;
export function decodeCardTable(data) {
    const expansionId = data.readUInt16LE(8);
    const count = data.readUInt16LE(11);
    const cards = [];
    for (let i = 0; i < count; i++) {
        const o = CARD_HEADER_LEN + i * CARD_ENTRY_LEN;
        if (o + CARD_ENTRY_LEN > data.length) break; // an old-layout table (devnet before the upgrade)
        const e = data.subarray(o, o + CARD_ENTRY_LEN);
        cards.push({ rarity: e[0], name: e.subarray(2, 2 + e[1]).toString('utf8'), uriSuffix: e.subarray(35, 35 + e[34]).toString('utf8') });
    }
    return { expansionId, count, cards };
}

export function treeConfigInfo(data) {
    return { creator: new PublicKey(data.subarray(8, 40)), delegate: new PublicKey(data.subarray(40, 72)), capacity: data.readBigUInt64LE(72), numMinted: data.readBigUInt64LE(80) };
}

/** PackMinted / AdminMinted events from the self-CPI inner instructions of a confirmed transaction. */
export function decodeEvents(tx) {
    const out = [];
    const keys = tx.transaction.message.staticAccountKeys ?? tx.transaction.message.accountKeys;
    for (const inner of tx.meta?.innerInstructions ?? []) {
        for (const ins of inner.instructions) {
            const prog = keys[ins.programIdIndex];
            if (!prog || !prog.equals(PROGRAM_ID)) continue;
            const data = Buffer.from(require58(ins.data));
            for (const ev of IDL.events ?? []) {
                if (data.length > 16 && Buffer.from(ev.discriminator).equals(data.subarray(8, 16))) out.push({ name: ev.name, data: data.subarray(16) });
            }
        }
    }
    return out;
}

import bs58pkg from 'bs58';
const require58 = (s) => bs58pkg.decode(s);

export function decodePackMinted(buf) {
    let o = 0;
    const u16 = () => { const v = buf.readUInt16LE(o); o += 2; return v; };
    const u32 = () => { const v = buf.readUInt32LE(o); o += 4; return v; };
    const u64 = () => { const v = buf.readBigUInt64LE(o); o += 8; return v; };
    const key = () => { const v = new PublicKey(buf.subarray(o, o + 32)); o += 32; return v; };
    const expansionId = u16();
    const orderId = buf.subarray(o, o + 16).toString('hex'); o += 16;
    const player = key(); const tree = key(); const firstLeafIndex = u64();
    const n = u32(); const cardIds = []; for (let i = 0; i < n; i++) cardIds.push(u16());
    const price = u64(); const seed = buf.subarray(o, o + 32).toString('hex');
    return { expansionId, orderId, player, tree, firstLeafIndex, cardIds, price, seed };
}

/** Bubblegum asset id of a leaf. */
export const assetId = (tree, leafIndex) => pda([Buffer.from('asset'), new PublicKey(tree).toBuffer(), le(leafIndex, 8)], BUBBLEGUM_ID);
