# Devnet deployment

Program `RoD7T8KRzHoTPnbS15uibfKAaqV5bYzcVDW8X9zWmZN` was deployed to devnet on 2026-11-23 and tested with one pack
and one admin mint. Every address is in `deployments/devnet.json`. Test keys only: the devnet admin is a test key,
not `CiAdmm...`.

## What is on devnet

| Item | Address |
|---|---|
| Program (upgrade authority = devnet admin `ETpNiaa3z8TRF9QHEZd9mJ7CcRWSztzEeCZU9mMk327y`) | `RoD7T8KRzHoTPnbS15uibfKAaqV5bYzcVDW8X9zWmZN` |
| Config | `ERQ8ZiFJLnwcGBTuoR9ESSFWTuDY9dXw5WWVetdoPCSx` |
| Mint authority PDA (tree creator/delegate, collection authority) | `DBWpKB4FznUJegj4HYR6Q9BwwE8DSLHtB7gMRNU9gDfB` |
| Test collection "Crypto Idolz Cards (devnet)" (stands in for `CARDZ...`) | `GTieLvbNLMU1hhEZHUUtTjwiSoU1jf9zzMfKUj6DKFgg` |
| Collection authority record for the mint authority | `2GPzeCab2vrNzZjK9tBXVKGQtmVAoE9j23A5Lz6rBAvj` |
| Test $BOOTY mint (9 decimals, mint authority = devnet admin) | `DoLiFRNVuuGvKnAAU8WvRy2KZ1FKDCmjSyGdJw7yEBdA` |
| $BOOTY destination (stands in for the shop wallet) | `4YQFFaPwniikK9ZBm3ERuJxmi7QTqocaSb3tPBUdJtak` |
| First Crush expansion 1 / card table (66 cards, checked against the card list) | `FK9BfPbyzo5NzYZQ7qpqJ6j7zxBtayKyXyk7myFuZcSd` / `FjHZwedvmt79J2jeZ7Kc1LeXWRpQH2BP5o1Yhq81YGK4` |
| Active tree (14/64/8, 16,384 leaves) | `C3XUSVPDEZTu3fXzTBBvw6TGno4DJdjsWXxMEzkTMHeH` |
| Standby tree (made by the tree funder) | `5Up12nzgJCsCkZAUAyESPvZB1wWGu1gjnDDdnY4Bi6sj` |
| Lookup table for `buy_pack` | `A44typVio7FWNQ8UuQnrW3ApzpUAnJKtDPLXdakULx9C` |

Test keys (`%USERPROFILE%\.config\rod-pack-minter\devnet\`, never committed or printed): admin, approver
`3zurkJbmHrHUeak1x2p9S1Hj27oebCxKNuCAnKdJz7Hv`, tree funder `2d9qd3rZ4Upuubfo4HeLE7aCb68CDeakAXErcbde4aow`,
collection owner `4jBVfRCTLvRL2NaMy4vf5XFyT21H9ryDBFbeeyBp5zFg`, player `9fUJVnU5RWsCf1niJML7ueBifdE1tMxnYY2h6bdTzAuC`.

## First results

- `buy_pack`: 467-byte v0 transaction, 467,068 CU, 150 $BOOTY paid, 6 cards in leaves 0-5 of the active tree
  ([transaction](https://explorer.solana.com/tx/dqur9fPravTzq9sxoSmtGKrw9Jg6kv3gsQ7MhgMoR5CoNYYa2EVCH4B5oYjQZmWxFmHvpQyy54dCharD3CdSJAC?cluster=devnet)).
- `admin_mint`: 2 copies of card #50 to the test player.
- Rent (same on mainnet, checked 2026-11-23): tree 0.2451 SOL, program 2.2236 SOL.

## How it was set up (repeatable)

Rehearse on Surfpool first (`surfpool start --network devnet --no-deploy -p 18899`, then the same commands with
`RPM_NETWORK=surfnet` and `DEVNET_RPC_URL=http://127.0.0.1:18899`; SOL from `solana airdrop`).

```powershell
node scripts/devnet.mjs keys                  # addresses and balances (fund the admin with ~5 devnet SOL)
node scripts/devnet.mjs booty                 # test $BOOTY, shop account, 10,000 to the player
node scripts/devnet.mjs deploy "<program key path>"   # first deploy only; the key is passed by path
node scripts/devnet.mjs fund                  # spreads SOL to the other test keys
node scripts/devnet.mjs collection            # test collection + collection authority record
node scripts/devnet.mjs init                  # config (paused), approver, funder, tree size 14/64/8
node scripts/devnet.mjs expansion             # First Crush + 66 cards from CardListStructureInfo.json, unpaused
node scripts/devnet.mjs tree admin            # active tree
node scripts/devnet.mjs tree funder           # standby tree
node scripts/devnet.mjs unpause
node scripts/devnet.mjs buy                   # one pack as the test player (wallet signs first, approver second)
node scripts/devnet.mjs admin-mint 50 2       # 2 copies of card 50 to the player
node scripts/devnet.mjs status
```

Upgrades after a code change: `anchor build`, then
`solana program deploy target\deploy\rod_pack_minter.so --program-id RoD7T8KRzHoTPnbS15uibfKAaqV5bYzcVDW8X9zWmZN --upgrade-authority <devnet admin key> -k <devnet admin key> -u devnet`
(no program key file needed once the program exists).

## Gotchas found

- The Solana CLI rejects key paths with spaces ("unrecognized signer source"); `deploy` runs from the key's folder.
- The devnet faucet refuses airdrops from this machine; fund the admin from faucet.solana.com.
- Lookup table creation needs a slot the executing node knows: the public RPC's newest finalized slot can be "not a
  recent slot", so the script uses one 50 slots older.
- Token Metadata names are at most 32 characters.

## Not checked yet

- Helius DAS on devnet showing the cards as verified members of the test collection (needs the Helius key that the
  backend holds).
- The backend order flow, the admin site page and Unity against this deployment.
