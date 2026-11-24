# rod-pack-minter

Solana program (Anchor 1.2.2) that sells Reign of Desire card packs as Bubblegum v1 cNFTs in the expansion's Token
Metadata collection. A purchase has two steps:

1. `buy_pack` (player + backend approver): the $BOOTY price moves into the program's vault and a PendingPack records
   the purchase slot. Tree room is reserved and the draw inputs (weights, card counts) are frozen.
2. `open_pack` (anyone, normally the backend a moment later): the cards are drawn from the hash of the purchase slot,
   which only exists once the purchase is irreversible, then minted to the player. The price goes to the shop.

Nobody (player, backend, opener or admin) can choose or re-roll a paid pack's cards. Only `admin_mint` (admin-signed)
picks a card. The player pays every fee: `buy_pack` takes a deposit (an open fee for whoever opens the pack, plus a retry pool for
up to `paid_retries` seals/redraws, returned unused at opening).
Failed transactions need no refund: a failed `buy_pack` takes nothing, and a failed `open_pack` is simply retried
(the cards are already fixed). If nobody opens a pack while its slot hash is in SlotHashes (~3.4 minutes, i.e. an
outage), anyone (the backend automatically) calls `redraw_pack`: the pack gets a new purchase slot and then opens
normally, so the price never leaves the vault and nobody handles refunds.

Plan and decisions: `ci-cardgame/docs/plans/2026-11-22-first-crush-expansion.md`. Security notes: `docs/security.md`.

## Instructions

| Instruction | Signer | What it does |
|---|---|---|
| `initialize` | program upgrade authority | Config (paused), vault, approver, tree funder, $BOOTY mint and destination, tree size, fees |
| `migrate_config`, `init_vault` | admin | One-off upgrade of a v1 config (devnet) |
| `set_paused`, `set_approver`, `set_tree_funder`, `set_tree_size`, `set_fees`, `set_payment`, `set_destination`, `propose_admin` / `accept_admin` | admin | Global settings |
| `add_expansion`, `update_expansion`, `set_price`, `set_expansion_paused`, `upsert_cards` | admin | Expansions; one discount period at a time; cards are appended or renamed, never removed or re-rarified |
| `create_tree` | admin or tree funder | Initialises a pre-allocated tree with the program's PDA as creator and delegate; becomes active or standby |
| `rotate_tree` | anyone | Promotes the standby tree once the active tree has fewer than 10 free leaves |
| `buy_pack(expansion_id, order_id, max_price)` | player + approver | Step 1: price to the vault, PendingPack, fee deposit |
| `open_pack` | anyone | Step 2: seal, draw, mint, price to the shop, deposit to the opener, rent back to the player |
| `seal_pack` | anyone | Stores the purchase slot's hash early (when opening must wait); paid from the deposit |
| `redraw_pack` | anyone | After an outage: gives an unopened, unsealed pack whose slot hash expired a new purchase slot |
| `refund_pack` | admin | Support tool only: price back to the player for an unopened pack (after `refund_after_secs`) |
| `admin_mint(expansion_id, card_id, count)` | admin | Mints a chosen card to any wallet, no payment |

Pause (global or per expansion) stops `buy_pack` and `admin_mint`; already paid packs can still be opened.

## Build and test on Windows (native, no WSL)

Tools: Rust (MSVC), Visual Studio Build Tools, Solana CLI 4.1.2 (Agave Windows build in
`%USERPROFILE%\.local\share\solana\install\active_release\bin`), Anchor CLI 1.2.2.

```powershell
.\scripts\setup-windows.ps1      # checks tools; applies the platform-tools v1.57 rustc workaround
.\scripts\fetch-fixtures.ps1     # Bubblegum, account compression, noop, Token Metadata binaries for tests
anchor build                     # target\deploy\rod_pack_minter.so + target\idl\rod_pack_minter.json
cargo test -p rod-pack-minter -- --nocapture
```

The tests (34) run in LiteSVM with the real Metaplex programs. Measured: `buy_pack` ~30-46k CU, `open_pack` of 6 cards
~400-440k CU, 10 cards ~650-770k CU (also with a full 400-card table), `admin_mint` of 3 cards ~200k CU (callers must set
a compute limit). With a lookup table: `buy_pack` 444-476 bytes, `rotate_tree` + `open_pack` 361-377 bytes.

`idl/rod_pack_minter.json` is the committed copy of the IDL for the backend and admin site; refresh it after changes.

## Keys

No key file is ever committed (`.gitignore` blocks `*keypair*.json`, `.env`, `target/`). Scripts take key file paths
as arguments and never print keys.

Program id: `RoD7T8KRzHoTPnbS15uibfKAaqV5bYzcVDW8X9zWmZN`, the same on devnet and mainnet. Its key file is kept
offline by James and is needed only for the first deploy on each network (`--program-id <path>`); after that the
upgrade authority signs upgrades.

`anchor build` writes a random placeholder `target/deploy/rod_pack_minter-keypair.json` when the real key file is not
there and prints "Program ID mismatch". That is harmless for building and testing. **Never run `anchor keys sync`**: it
would replace the program id with the placeholder. Deploy with `solana program deploy ... --program-id <real key path>`.
