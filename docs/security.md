# Security notes

Two independent reviews were run (2026-11-23 and 2026-11-24). This file lists what the design guarantees, the
residual risks that were accepted, and the rules the backend must follow.

## Guarantees (enforced on chain)

- **Fair draw.** A pack's cards come from sha256(hash of the purchase slot | order_id | player | purchase slot |
  expansion id). The purchase slot's hash exists only after that slot is complete, when the purchase and its payment
  can no longer be undone.
  - The player, the approver, any opener and the admin cannot choose or re-roll the cards.
  - The opener and the opening time do not affect the seed.
  - Weights and card counts are frozen in the PendingPack at purchase.
  - Card rarities cannot change once in a table, and cards are only appended.
- **No refund option.** Only the admin can refund, as a support tool. The cards of a paid pack are public from the
  next slot, so a refund the player could trigger would let them return bad packs.
- **Outages: redraw, not refund.** A pack whose slot hash left SlotHashes (~512 slots) before anyone sealed or
  opened it gets a new purchase slot from `redraw_pack` (anyone may call it; the backend does it automatically).
  The new slot's hash does not exist yet when the redraw is sent, so the redraw is as fair as a new purchase. The
  price never leaves the vault. There is no fallback hash known in advance. Bots and the backend are paid
  (`seal_fee`, `open_fee`) to seal or open early.
- **Mintable when sold.** `buy_pack` reserves tree leaves (`reserved_leaves` against active + standby free leaves)
  and requires weights that can draw a card, so a paid pack can always be opened.
- **Only the admin picks a card** (`admin_mint`). Trees are created with the program PDA as creator and delegate,
  `public = false`. Nobody can mint into them outside the program.
- **Money.**
  - The vault can only pay a pack's recorded price, once: to the shop on open, or to the player's own account on an
    admin refund.
  - `set_payment` waits for zero pending packs.
  - `set_destination` can repair a broken shop account at any time, and the vault is never allowed as the
    destination.
  - Only plain mints are accepted (82 bytes, no Token-2022 extensions).
- **The player pays every fee.**
  - The PendingPack holds the player's deposit (`open_fee + seal_fee`). It is paid to whoever opens or seals (or to
    the admin for a refund).
  - The rent goes back to the player.
  - Measured: player net 35-36k lamports per pack; opener net +14-20k.
- **Set-up.** Only the upgrade authority can `initialize`. Every admin instruction checks `has_one = admin`. The
  admin change is two-step.

## Accepted residual risks

- **Outage re-roll.** During an outage of every opener (more than ~3.4 minutes), a player who computed that their
  pack's cards are bad could avoid sealing it, so that it is redrawn. A refund and a new purchase would give the same
  edge. The game client opening the pack itself as a backup keeps such outages rare.
- **Block leader.** A colluding leader of the purchase slot could try block contents to influence that slot's hash.
- **Forks.** If a purchase lands on a fork that loses, a player who saw that fork's hash could try to keep the
  purchase off the winning fork. The player cannot cause forks.
- **$BOOTY freeze authority.** If the $BOOTY mint has a freeze authority, its holder could freeze the vault and
  block opens and refunds. This is trust in the token issuer (the project itself).
- **Admin key** (`CiAdmm...` on mainnet). It controls the program: prices, pause, admin mints, upgrades, refunds. It
  already controls the card collection.
- **Approver key** (backend). Its holder can sell packs at the on-chain price, but cannot choose cards.

## Backend rules

- **Order ids.** Generate `order_id` on the server only (random 16 bytes), never from client input.
- **Signing.** Co-sign `buy_pack` only for a transaction the server built, with the expected `max_price`. Send it
  yourself.
- **Opening.** Send `open_pack` as soon as the purchase slot is complete, and retry on failure. If opening must wait,
  send `seal_pack` within ~3 minutes. A pending pack that fails with `SlotHashUnavailable` gets `redraw_pack`, then
  `open_pack` one slot later. The game client should also send `open_pack` itself if the backend has not opened the
  pack after a few seconds.
- **Reading events.** Take PackBought / PackMinted only from:
  - a successful, finalized transaction;
  - inner instructions of program `RoD7T8...`;
  - whose first account is the `__event_authority` PDA, as signer;
  - whose data starts with the event-CPI tag and the event discriminator.

  Never read events from `Program data:` logs. Match `(player, order_id)` to your order. Recompute the cards from
  `seed` if in doubt.
- **Trees.** Keep a standby tree: create one whenever `standby_tree` is empty (tree funder key).
