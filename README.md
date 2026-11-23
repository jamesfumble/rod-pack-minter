# rod-pack-minter

Solana program (Anchor 1.2.2) that sells Reign of Desire card packs. One `buy_pack` transaction takes the $BOOTY price
and mints the whole pack as Bubblegum v1 cNFTs into the expansion's Token Metadata collection. If any step fails,
nothing happens. Cards are drawn on chain when the transaction runs. Only the admin can choose a card (`admin_mint`).

Plan and decisions: `ci-cardgame/docs/plans/2026-11-22-first-crush-expansion.md`.

## Instructions

| Instruction | Signer | What it does |
|---|---|---|
| `initialize` | program upgrade authority | Creates the config (paused), sets the approver, tree funder, $BOOTY mint and destination, tree size |
| `set_paused`, `set_approver`, `set_tree_funder`, `set_tree_size`, `set_payment`, `propose_admin` / `accept_admin` | admin | Global settings |
| `add_expansion`, `update_expansion`, `set_price`, `set_expansion_paused`, `upsert_cards` | admin | Expansions (First Crush = 1); one discount period at a time |
| `create_tree` | admin or tree funder | Initialises a pre-allocated tree with the program's PDA as creator and delegate; becomes active or standby |
| `rotate_tree` | anyone | Promotes the standby tree once the active tree has fewer than 10 free leaves |
| `buy_pack(expansion_id, order_id, max_price)` | player + approver | Pays and mints `pack_size` drawn cards; must be top-level and followed only by compute-budget (or harmless Lighthouse) instructions |
| `admin_mint(expansion_id, card_id, count)` | admin | Mints a chosen card to any wallet, no payment |

Pause (global or per expansion) stops both `buy_pack` and `admin_mint`.

## Build and test on Windows (native, no WSL)

Tools: Rust (MSVC), Visual Studio Build Tools, Solana CLI 4.1.2 (Agave Windows build in
`%USERPROFILE%\.local\share\solana\install\active_release\bin`), Anchor CLI 1.2.2.

```powershell
.\scripts\setup-windows.ps1      # checks tools; applies the platform-tools v1.57 rustc workaround
.\scripts\fetch-fixtures.ps1     # Bubblegum, account compression, noop, Token Metadata binaries for tests
anchor build                     # target\deploy\rod_pack_minter.so + target\idl\rod_pack_minter.json
cargo test -p rod-pack-minter -- --nocapture
```

The tests run in LiteSVM with the real Metaplex programs. Measured: 6-card pack ~445k CU, 10-card pack ~675k CU,
`admin_mint` of 3 cards ~246k CU (callers must set a compute limit), v0 transaction with `rotate_tree` + `buy_pack`
482 bytes with a lookup table.

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
