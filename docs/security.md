# Security notes

Four independent reviews were run (2026-11-23, 2026-11-24 three times). This file lists what the design guarantees, the
residual risks that were accepted, and the rules the backend must follow.

## Guarantees (enforced on chain)

- **Fair draw.** A pack's cards come from sha256(hash of the purchase slot | order_id | player | purchase slot |
  expansion id). The purchase slot's hash exists only after that slot is complete, when the purchase and its payment
  can no longer be undone.
  - The player, the approver, any opener and the admin cannot choose or re-roll the cards.
  - The opener and the opening time do not affect the seed.
  - Weights and card counts are frozen in the PendingPack at purchase.
  - Card rarities cannot change once in a table, and cards are only appended.
  - While packs are pending, existing card entries (name, URI) and the expansion's metadata settings (URI prefix,
    symbol, creator, royalty, collection) cannot change, so the admin cannot swap what a paid pack mints. Minted
    cards stay mutable through the collection's update authority, as before.
- **No refund option.** Only the admin can refund, as a support tool. The cards of a paid pack are public from the
  next slot, so a refund the player could trigger would let them return bad packs.
- **Outages: redraw, not refund.** A pack whose slot hash left SlotHashes (~512 slots) before anyone sealed or
  opened it gets a new purchase slot from `redraw_pack`. Only the backend approver (automatically) or the admin may
  call it, so a player or a colluding validator cannot time the new draw slot.
  The new slot's hash does not exist yet when the redraw is sent, so the redraw is as fair as a new purchase. The
  price never leaves the vault. There is no fallback hash known in advance. A separate `seal_pack` is paid from the
  retry pool only from 150 slots after purchase, so bots gain nothing by sealing every pack at once.
- **Mintable when sold.** `buy_pack` reserves tree leaves against the active and standby trees' free leaves minus the
  9 a tree can lose at rotation, and requires weights that can draw a card, so a paid pack can always be opened.
  `admin_mint` cannot use reserved leaves.
- **Only the admin picks a card** (`admin_mint`). Trees are created with the program PDA as creator and delegate,
  `public = false`. Nobody can mint into them outside the program.
- **Money.**
  - Each currency's vault can only pay a pack's recorded price, once: on open to the shop ($BOOTY) or to the four
    $LUST destinations plus the burned rest ($LUST), or to the player's own account on an admin refund.
  - `set_payment` waits for zero pending packs.
  - `set_destination` can repair a broken shop account at any time, and the vault is never allowed as the
    destination.
  - Only plain mints are accepted (82 bytes, no Token-2022 extensions), for $BOOTY and $LUST.
  - Two currencies, each with its own vault; $LUST and $BOOTY must be different mints. A $LUST price is paid out to exactly the four configured destinations
    (checked against `LustConfig`), with the remainder burned; the split cannot exceed 100%.
  - Prices are mirrored from the shop catalog with Phantom-signed `set_price` / `set_sale_period`; the player's
    `max_price` caps what they pay. Price 0 means not for sale in that currency.
- **The player pays every fee.**
  - The PendingPack holds the player's deposit: `open_fee` for whoever opens (or the admin, for a refund) and a retry
    pool of `retry_fee x paid_retries` that pays each seal or redraw while it lasts.
  - The rent and the unused pool go back to the player.
  - Measured: player net 30,000 lamports per pack (devnet fees); opener net +9k to +15k.
  - `redraw_pack` is only accepted when opening has become impossible (unsealed and the purchase slot hash has left
    SlotHashes); otherwise it fails with `PackStillOpenable` / `AlreadySealed`.
- **Set-up.** Only the upgrade authority can `initialize`. Every admin instruction checks `has_one = admin`. The
  admin change is two-step.

## Accepted residual risks

- **Outage re-roll.** The cards of a paid pack are computable from the next slot. If nobody seals or opens it for
  ~3.4 minutes, it is redrawn, so a player whose pack is bad gains from such an outage. The backend opens
  immediately, and the game client opens as a backup, so this needs a real outage of both.
- **Blocking the seal on purpose (unconfirmed).** A player could try to keep their own PendingPack from being sealed
  for ~512 blocks by filling its per-account compute limit with high-priority transactions, to force a redraw. That
  would cost on the order of several thousand max-compute transactions outbidding the backend for ~3.4 minutes, far
  more than a pack's value. Mitigation: the backend sends `open_pack` in the first slot after purchase with a
  competitive priority fee, and watches for repeated failures.
- **Block leader.** A colluding leader of the purchase slot could try block contents to influence that slot's hash.
- **Forks.** If a purchase lands on a fork that loses, a player who saw that fork's hash could try to keep the
  purchase off the winning fork. The player cannot cause forks.
- **Freeze authorities.** If the $BOOTY or $LUST mint has a freeze authority, its holder could freeze a vault and
  block opens and refunds. This is trust in the token issuer (the project itself).
- **Payout accounts.** The shop account and the four $LUST destinations must accept transfers, or every `open_pack`
  of that currency fails (until the admin replaces them with `set_destination` / `set_lust_payment`). Use only
  token accounts the project controls (plain SPL Token accounts). The admin can also change the $LUST split or
  destinations while packs are pending; they apply when those packs open (admin trust, like `set_destination`).
- **Account layouts.** PendingPack and Expansion have 32 spare bytes for future fields. A layout change without
  them breaks existing accounts (as happened on devnet before mainnet, where expansions 1 and 2 are unreadable).
- **Admin key** (`CiAdmm...` on mainnet). It controls the program: prices, pause, admin mints, upgrades, refunds. It
  already controls the card collection.
- **Approver key** (backend). Its holder can sell packs at the on-chain price, but cannot choose cards.

## Backend rules

- **Order ids.** Generate `order_id` on the server only (random 16 bytes), never from client input.
- **Signing.** Co-sign `buy_pack` only for a transaction the server built, with the expected `max_price`. Send it
  yourself.
- **Opening.** Send `open_pack` as soon as the purchase slot is complete (with a competitive priority fee), and retry
  on failure. If an open fails for any reason other than the seal itself (e.g. a payout account rejects the
  transfer), send `seal_pack` at once so the cards stay fixed while the cause is repaired. Mark the payment mint
  writable only for $LUST packs. If opening must wait, send `seal_pack` within ~3 minutes. A pending pack that fails with
  `SlotHashUnavailable` gets `redraw_pack` (signed by the approver), then `open_pack` one slot later. The game client should also send `open_pack` itself if the backend has not opened the
  pack after a few seconds.
- **Reading events.** Take PackBought / PackMinted only from:
  - a successful, finalized transaction;
  - inner instructions of program `RoD7T8...`;
  - whose first account is the `__event_authority` PDA, as signer;
  - whose data starts with the event-CPI tag and the event discriminator.

  Never read events from `Program data:` logs. Match `(player, order_id)`, `currency` and `price` to your order. Recompute the cards from
  `seed` if in doubt.
- **Trees.** Keep a standby tree: create one whenever `standby_tree` is empty (tree funder key).
