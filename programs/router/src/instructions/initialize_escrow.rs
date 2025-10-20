//! Initialize escrow instruction

use crate::pda::derive_escrow_pda;
use crate::state::Escrow;
use percolator_common::*;
use pinocchio::{account_info::AccountInfo, msg, pubkey::Pubkey};

/// Process initialize escrow instruction
///
/// Initializes an escrow account for (user, slab, mint) triplet.
///
/// # Arguments
/// * `program_id` - The router program ID
/// * `escrow_account` - The escrow account to initialize (must be PDA)
/// * `user` - The user pubkey
/// * `slab` - The slab program pubkey
/// * `mint` - The mint pubkey
pub fn process_initialize_escrow(
    program_id: &Pubkey,
    escrow_account: &AccountInfo,
    user: &Pubkey,
    slab: &Pubkey,
    mint: &Pubkey,
) -> Result<(), PercolatorError> {
    // Derive and verify escrow PDA
    let (expected_pda, bump) = derive_escrow_pda(user, slab, mint, program_id);

    if escrow_account.key() != &expected_pda {
        msg!("Error: Escrow account is not the correct PDA");
        return Err(PercolatorError::InvalidAccount);
    }

    // Verify account size
    let data = escrow_account.try_borrow_data()
        .map_err(|_| PercolatorError::InvalidAccount)?;

    if data.len() != Escrow::LEN {
        msg!("Error: Escrow account has incorrect size");
        return Err(PercolatorError::InvalidAccount);
    }

    // Check if already initialized (first bytes should be zero)
    if data.len() >= 32 && data[0] != 0 {
        msg!("Error: Escrow account may already be initialized");
        return Err(PercolatorError::InvalidAccount);
    }

    drop(data);

    // Initialize the escrow
    let escrow = unsafe { borrow_account_data_mut::<Escrow>(escrow_account)? };

    *escrow = Escrow {
        router_id: *program_id,
        slab_id: *slab,
        user: *user,
        mint: *mint,
        balance: 0,
        nonce: 0,
        frozen: false,
        bump,
        _padding: [0; 6],
    };

    msg!("Escrow initialized successfully");
    Ok(())
}
