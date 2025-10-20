//! Initialize instruction - initialize slab state

use crate::pda::derive_slab_pda;
use crate::state::{SlabHeader, SlabState};
use percolator_common::*;
use pinocchio::{account_info::AccountInfo, msg, pubkey::Pubkey};

/// Process initialize instruction for slab
///
/// Initializes the 10MB slab state account with header and empty pools.
/// This is called once during slab deployment for each market.
///
/// # Arguments
/// * `program_id` - The slab program ID
/// * `slab_account` - The slab account to initialize (must be PDA)
/// * `market_id` - Unique market identifier (32 bytes)
/// * `lp_owner` - LP owner pubkey
/// * `router_id` - Router program ID
/// * `imr` - Initial margin ratio (basis points)
/// * `mmr` - Maintenance margin ratio (basis points)
/// * `maker_fee` - Maker fee (basis points, can be negative)
/// * `taker_fee` - Taker fee (basis points)
/// * `batch_ms` - Batch window duration (milliseconds)
pub fn process_initialize_slab(
    program_id: &Pubkey,
    slab_account: &AccountInfo,
    market_id: [u8; 32],
    lp_owner: Pubkey,
    router_id: Pubkey,
    imr: u64,
    mmr: u64,
    maker_fee: i64,
    taker_fee: u64,
    batch_ms: u64,
) -> Result<(), PercolatorError> {
    // Derive and verify slab PDA
    let (expected_pda, bump) = derive_slab_pda(&market_id, program_id);

    if slab_account.key() != &expected_pda {
        msg!("Error: Slab account is not the correct PDA");
        return Err(PercolatorError::InvalidAccount);
    }

    // Verify account size (10 MB)
    let data = slab_account.try_borrow_data()
        .map_err(|_| PercolatorError::InvalidAccount)?;

    if data.len() != core::mem::size_of::<SlabState>() {
        msg!("Error: Slab account has incorrect size");
        return Err(PercolatorError::InvalidAccount);
    }

    // Check if already initialized (magic bytes should not match)
    if data.len() >= 8 && &data[0..8] == SlabHeader::MAGIC {
        msg!("Error: Slab account already initialized");
        return Err(PercolatorError::InvalidAccount);
    }

    drop(data);

    // Initialize the slab state
    let slab = unsafe { borrow_account_data_mut::<SlabState>(slab_account)? };

    // Initialize header with parameters
    slab.header = SlabHeader::new(
        *program_id,
        lp_owner,
        router_id,
        imr,
        mmr,
        maker_fee,
        taker_fee,
        batch_ms,
        bump,
    );

    // Zero-initialize all arrays and pools
    // accounts array - manually zero-initialize
    for i in 0..MAX_ACCOUNTS {
        unsafe {
            core::ptr::write_bytes(&mut slab.accounts[i] as *mut AccountState, 0, 1);
        }
    }

    // instruments array - manually zero-initialize
    for i in 0..MAX_INSTRUMENTS {
        unsafe {
            core::ptr::write_bytes(&mut slab.instruments[i] as *mut Instrument, 0, 1);
        }
    }
    slab.instrument_count = 0;

    // DLP accounts
    for i in 0..MAX_DLP {
        slab.dlp_accounts[i] = 0;
    }

    // Initialize pools (sets all entries to default and free lists)
    slab.orders = crate::state::pools::Pool::new();
    slab.positions = crate::state::pools::Pool::new();
    slab.reservations = crate::state::pools::Pool::new();
    slab.slices = crate::state::pools::Pool::new();
    slab.aggressor_ledger = crate::state::pools::Pool::new();

    // Trade ring buffer - zero initialize
    for i in 0..MAX_TRADES {
        // Manually zero-initialize Trade structure
        unsafe {
            core::ptr::write_bytes(&mut slab.trades[i] as *mut Trade, 0, 1);
        }
    }
    slab.trade_head = 0;
    slab.trade_count = 0;

    msg!("Slab initialized successfully");
    Ok(())
}
