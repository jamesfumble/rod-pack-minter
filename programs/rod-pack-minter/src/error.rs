use anchor_lang::prelude::*;

#[error_code]
pub enum ErrorCode {
    #[msg("Only the admin may do this")]
    Unauthorized,
    #[msg("Only the pending admin may accept")]
    NotPendingAdmin,
    #[msg("The program is paused")]
    ProgramPaused,
    #[msg("This expansion is paused")]
    ExpansionPaused,
    #[msg("This expansion is not on sale now")]
    OutsideSalePeriod,
    #[msg("The pack price is higher than the price the buyer accepted")]
    PriceAboveMax,
    #[msg("buy_pack must be a top-level instruction")]
    NotTopLevel,
    #[msg("Only compute-budget or Lighthouse instructions may follow buy_pack")]
    ForbiddenFollowingInstruction,
    #[msg("A Lighthouse instruction may not reference the pack accounts")]
    ForbiddenLighthouseAccount,
    #[msg("Wrong approver")]
    WrongApprover,
    #[msg("This is not the active tree")]
    NotActiveTree,
    #[msg("The active tree has no room for this pack; rotate to the standby tree first")]
    TreeFull,
    #[msg("There is no active tree")]
    NoActiveTree,
    #[msg("A standby tree already exists")]
    StandbyExists,
    #[msg("There is no standby tree to rotate to")]
    NoStandbyTree,
    #[msg("The active tree still has room")]
    TreeNotFull,
    #[msg("The tree account has the wrong owner or size")]
    BadTreeAccount,
    #[msg("Bad tree config account")]
    BadTreeConfig,
    #[msg("Wrong collection for this expansion")]
    WrongCollection,
    #[msg("Wrong $BOOTY mint or destination")]
    WrongPaymentAccount,
    #[msg("Pack size must be 1 to 10")]
    BadPackSize,
    #[msg("Weights must sum to more than 0, and only rarities that have cards may have weight")]
    BadWeights,
    #[msg("Royalty must be at most 10000 bps")]
    BadRoyalty,
    #[msg("Text too long")]
    TextTooLong,
    #[msg("Bad rarity")]
    BadRarity,
    #[msg("Card id out of range")]
    BadCardId,
    #[msg("Too many cards")]
    TooManyCards,
    #[msg("Card index must be at most the current card count")]
    BadCardIndex,
    #[msg("The expansion has no cards")]
    NoCards,
    #[msg("Discount period is invalid")]
    BadDiscount,
    #[msg("Admin mint count must be 1 to 6")]
    BadCount,
    #[msg("Bad SlotHashes sysvar")]
    BadSlotHashes,
    #[msg("Arithmetic overflow")]
    Overflow,
    #[msg("The pack can be opened from the slot after its purchase")]
    TooEarlyToOpen,
    #[msg("The pack is already sealed")]
    AlreadySealed,
    #[msg("The purchase slot's hash is no longer in SlotHashes; use the fallback (open after expiry)")]
    SlotHashUnavailable,
    #[msg("The refund period has not started")]
    TooEarlyToRefund,
    #[msg("Wrong player for this pack")]
    WrongPlayer,
    #[msg("Wrong vault")]
    WrongVault,
    #[msg("Fee too high")]
    FeeTooHigh,
    #[msg("Packs are pending; open or refund them first")]
    PacksPending,
    #[msg("The config is already migrated")]
    AlreadyMigrated,
    #[msg("Token mints with extensions are not supported")]
    MintHasExtensions,
    #[msg("Cannot change the collection after cards were minted")]
    CollectionLocked,
    #[msg("A discount needs a price above 0")]
    ZeroDiscount,
}
