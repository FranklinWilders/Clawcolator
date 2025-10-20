/// Router instruction handlers

pub mod deposit;
pub mod withdraw;
pub mod initialize;
pub mod initialize_portfolio;
pub mod initialize_escrow;
pub mod multi_reserve;
pub mod multi_commit;
pub mod liquidate;

pub use deposit::*;
pub use withdraw::*;
pub use initialize::*;
pub use initialize_portfolio::*;
pub use initialize_escrow::*;
pub use multi_reserve::*;
pub use multi_commit::*;
pub use liquidate::*;

/// Instruction discriminator
#[repr(u8)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RouterInstruction {
    /// Initialize router
    Initialize = 0,
    /// Deposit collateral
    Deposit = 1,
    /// Withdraw collateral
    Withdraw = 2,
    /// Multi-slab reserve orchestration
    MultiReserve = 3,
    /// Multi-slab commit orchestration
    MultiCommit = 4,
    /// Liquidation coordinator
    Liquidate = 5,
    /// Initialize portfolio
    InitializePortfolio = 6,
    /// Initialize escrow
    InitializeEscrow = 7,
}

// Note: Instruction dispatching is handled in entrypoint.rs
// The functions in this module are called from the entrypoint after
// account deserialization and validation.
