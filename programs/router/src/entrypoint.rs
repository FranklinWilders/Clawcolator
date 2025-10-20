//! Router program entrypoint

use pinocchio::{
    account_info::AccountInfo,
    entrypoint,
    msg,
    pubkey::Pubkey,
    ProgramResult,
};

use crate::instructions::{RouterInstruction, process_deposit, process_withdraw, process_initialize_registry, process_initialize_portfolio, process_initialize_escrow};
use crate::state::Vault;
use percolator_common::{PercolatorError, validate_owner, validate_writable, borrow_account_data_mut, InstructionReader};

entrypoint!(process_instruction);

pub fn process_instruction(
    program_id: &Pubkey,
    accounts: &[AccountInfo],
    instruction_data: &[u8],
) -> ProgramResult {
    // Check minimum instruction data length
    if instruction_data.is_empty() {
        msg!("Error: Instruction data is empty");
        return Err(PercolatorError::InvalidInstruction.into());
    }

    // Parse instruction discriminator
    let discriminator = instruction_data[0];
    let instruction = match discriminator {
        0 => RouterInstruction::Initialize,
        1 => RouterInstruction::Deposit,
        2 => RouterInstruction::Withdraw,
        3 => RouterInstruction::MultiReserve,
        4 => RouterInstruction::MultiCommit,
        5 => RouterInstruction::Liquidate,
        6 => RouterInstruction::InitializePortfolio,
        7 => RouterInstruction::InitializeEscrow,
        _ => {
            msg!("Error: Unknown instruction: {}", discriminator);
            return Err(PercolatorError::InvalidInstruction.into());
        }
    };

    // Dispatch to instruction handler
    match instruction {
        RouterInstruction::Initialize => {
            msg!("Instruction: Initialize");
            process_initialize_inner(program_id, accounts, &instruction_data[1..])
        }
        RouterInstruction::Deposit => {
            msg!("Instruction: Deposit");
            process_deposit_inner(program_id, accounts, &instruction_data[1..])
        }
        RouterInstruction::Withdraw => {
            msg!("Instruction: Withdraw");
            process_withdraw_inner(program_id, accounts, &instruction_data[1..])
        }
        RouterInstruction::MultiReserve => {
            msg!("Instruction: MultiReserve");
            process_multi_reserve_inner(program_id, accounts, &instruction_data[1..])
        }
        RouterInstruction::MultiCommit => {
            msg!("Instruction: MultiCommit");
            process_multi_commit_inner(program_id, accounts, &instruction_data[1..])
        }
        RouterInstruction::Liquidate => {
            msg!("Instruction: Liquidate");
            process_liquidate_inner(program_id, accounts, &instruction_data[1..])
        }
        RouterInstruction::InitializePortfolio => {
            msg!("Instruction: InitializePortfolio");
            process_initialize_portfolio_inner(program_id, accounts, &instruction_data[1..])
        }
        RouterInstruction::InitializeEscrow => {
            msg!("Instruction: InitializeEscrow");
            process_initialize_escrow_inner(program_id, accounts, &instruction_data[1..])
        }
    }
}

// Instruction processors with account validation

/// Process initialize instruction
///
/// Expected accounts:
/// 0. `[writable]` Registry account (PDA)
/// 1. `[signer]` Governance authority
///
/// Expected data layout (32 bytes):
/// - governance: Pubkey (32 bytes)
fn process_initialize_inner(program_id: &Pubkey, accounts: &[AccountInfo], data: &[u8]) -> ProgramResult {
    if accounts.len() < 2 {
        msg!("Error: Initialize instruction requires at least 2 accounts");
        return Err(PercolatorError::InvalidInstruction.into());
    }

    let registry_account = &accounts[0];
    let governance_account = &accounts[1];

    // Validate accounts
    validate_owner(registry_account, program_id)?;
    validate_writable(registry_account)?;

    // Parse instruction data - governance pubkey
    let mut reader = InstructionReader::new(data);
    let governance_bytes = reader.read_bytes::<32>()?;
    let governance = Pubkey::from(governance_bytes);

    // Verify governance signer matches instruction data
    if governance_account.key() != &governance {
        msg!("Error: Governance account does not match instruction data");
        return Err(PercolatorError::InvalidAccount.into());
    }

    // Call the initialization logic
    process_initialize_registry(program_id, registry_account, &governance)?;

    msg!("Router initialized successfully");
    Ok(())
}

/// Process deposit instruction
///
/// Expected accounts:
/// 0. `[writable]` Vault account
/// 1. `[writable]` User token account
/// 2. `[signer]` User authority
/// 3. `[]` Token program
///
/// Expected data layout (16 bytes):
/// - amount: u128 (16 bytes)
fn process_deposit_inner(program_id: &Pubkey, accounts: &[AccountInfo], data: &[u8]) -> ProgramResult {
    if accounts.len() < 1 {
        msg!("Error: Deposit instruction requires at least 1 account");
        return Err(PercolatorError::InvalidInstruction.into());
    }

    let vault_account = &accounts[0];
    validate_owner(vault_account, program_id)?;
    validate_writable(vault_account)?;

    let vault = unsafe { borrow_account_data_mut::<Vault>(vault_account)? };

    // Parse instruction data
    let mut reader = InstructionReader::new(data);
    let amount = reader.read_u128()?;

    // Call the instruction handler
    process_deposit(vault, amount)?;

    msg!("Deposit processed successfully");
    Ok(())
}

/// Process withdraw instruction
///
/// Expected accounts:
/// 0. `[writable]` Vault account
/// 1. `[writable]` User token account
/// 2. `[signer]` User authority
/// 3. `[]` Token program
///
/// Expected data layout (16 bytes):
/// - amount: u128 (16 bytes)
fn process_withdraw_inner(program_id: &Pubkey, accounts: &[AccountInfo], data: &[u8]) -> ProgramResult {
    if accounts.len() < 1 {
        msg!("Error: Withdraw instruction requires at least 1 account");
        return Err(PercolatorError::InvalidInstruction.into());
    }

    let vault_account = &accounts[0];
    validate_owner(vault_account, program_id)?;
    validate_writable(vault_account)?;

    let vault = unsafe { borrow_account_data_mut::<Vault>(vault_account)? };

    // Parse instruction data
    let mut reader = InstructionReader::new(data);
    let amount = reader.read_u128()?;

    // Call the instruction handler
    process_withdraw(vault, amount)?;

    msg!("Withdraw processed successfully");
    Ok(())
}

/// Process multi-reserve instruction
///
/// Expected accounts:
/// 0. `[writable]` Portfolio account
/// 1. `[signer]` User authority
/// 2..N. `[writable]` Slab accounts
///
/// Expected data layout: TBD (Phase 4 work)
fn process_multi_reserve_inner(program_id: &Pubkey, accounts: &[AccountInfo], data: &[u8]) -> ProgramResult {
    if accounts.len() < 2 {
        msg!("Error: MultiReserve instruction requires at least 2 accounts");
        return Err(PercolatorError::InvalidInstruction.into());
    }

    let portfolio_account = &accounts[0];
    validate_owner(portfolio_account, program_id)?;
    validate_writable(portfolio_account)?;

    // TODO: Parse reserve parameters and coordinate multi-slab reserves
    // This is Phase 4 work - multi-slab coordination
    let _ = data;

    msg!("MultiReserve instruction validated - implementation pending");
    Ok(())
}

/// Process multi-commit instruction
///
/// Expected accounts:
/// 0. `[writable]` Portfolio account
/// 1. `[signer]` User authority
/// 2..N. `[writable]` Slab accounts
///
/// Expected data layout: TBD (Phase 4 work)
fn process_multi_commit_inner(program_id: &Pubkey, accounts: &[AccountInfo], data: &[u8]) -> ProgramResult {
    if accounts.len() < 2 {
        msg!("Error: MultiCommit instruction requires at least 2 accounts");
        return Err(PercolatorError::InvalidInstruction.into());
    }

    let portfolio_account = &accounts[0];
    validate_owner(portfolio_account, program_id)?;
    validate_writable(portfolio_account)?;

    // TODO: Parse commit parameters and coordinate multi-slab commits
    // This is Phase 4 work - atomic multi-slab execution
    let _ = data;

    msg!("MultiCommit instruction validated - implementation pending");
    Ok(())
}

/// Process liquidate instruction
///
/// Expected accounts:
/// 0. `[writable]` Portfolio account
/// 1. `[signer]` Liquidator
/// 2. `[writable]` Liquidatee account
/// 3..N. `[writable]` Slab accounts
///
/// Expected data layout: TBD (Phase 4 work)
fn process_liquidate_inner(program_id: &Pubkey, accounts: &[AccountInfo], data: &[u8]) -> ProgramResult {
    if accounts.len() < 3 {
        msg!("Error: Liquidate instruction requires at least 3 accounts");
        return Err(PercolatorError::InvalidInstruction.into());
    }

    let portfolio_account = &accounts[0];
    validate_owner(portfolio_account, program_id)?;
    validate_writable(portfolio_account)?;

    // TODO: Parse liquidation parameters and coordinate liquidation
    // This is Phase 4 work - cross-slab liquidation
    let _ = data;

    msg!("Liquidate instruction validated - implementation pending");
    Ok(())
}

/// Process initialize portfolio instruction
///
/// Expected accounts:
/// 0. `[writable]` Portfolio account (PDA)
/// 1. `[signer]` User
///
/// Expected data layout (32 bytes):
/// - user: Pubkey (32 bytes)
fn process_initialize_portfolio_inner(program_id: &Pubkey, accounts: &[AccountInfo], data: &[u8]) -> ProgramResult {
    if accounts.len() < 2 {
        msg!("Error: InitializePortfolio instruction requires at least 2 accounts");
        return Err(PercolatorError::InvalidInstruction.into());
    }

    let portfolio_account = &accounts[0];
    let user_account = &accounts[1];

    // Validate accounts
    validate_owner(portfolio_account, program_id)?;
    validate_writable(portfolio_account)?;

    // Parse instruction data - user pubkey
    let mut reader = InstructionReader::new(data);
    let user_bytes = reader.read_bytes::<32>()?;
    let user = Pubkey::from(user_bytes);

    // Verify user signer matches instruction data
    if user_account.key() != &user {
        msg!("Error: User account does not match instruction data");
        return Err(PercolatorError::InvalidAccount.into());
    }

    // Call the initialization logic
    process_initialize_portfolio(program_id, portfolio_account, &user)?;

    msg!("Portfolio initialized successfully");
    Ok(())
}

/// Process initialize escrow instruction
///
/// Expected accounts:
/// 0. `[writable]` Escrow account (PDA)
/// 1. `[signer]` User
///
/// Expected data layout (96 bytes):
/// - user: Pubkey (32 bytes)
/// - slab: Pubkey (32 bytes)
/// - mint: Pubkey (32 bytes)
fn process_initialize_escrow_inner(program_id: &Pubkey, accounts: &[AccountInfo], data: &[u8]) -> ProgramResult {
    if accounts.len() < 2 {
        msg!("Error: InitializeEscrow instruction requires at least 2 accounts");
        return Err(PercolatorError::InvalidInstruction.into());
    }

    let escrow_account = &accounts[0];
    let user_account = &accounts[1];

    // Validate accounts
    validate_owner(escrow_account, program_id)?;
    validate_writable(escrow_account)?;

    // Parse instruction data
    let mut reader = InstructionReader::new(data);
    let user_bytes = reader.read_bytes::<32>()?;
    let slab_bytes = reader.read_bytes::<32>()?;
    let mint_bytes = reader.read_bytes::<32>()?;

    let user = Pubkey::from(user_bytes);
    let slab = Pubkey::from(slab_bytes);
    let mint = Pubkey::from(mint_bytes);

    // Verify user signer matches instruction data
    if user_account.key() != &user {
        msg!("Error: User account does not match instruction data");
        return Err(PercolatorError::InvalidAccount.into());
    }

    // Call the initialization logic
    process_initialize_escrow(program_id, escrow_account, &user, &slab, &mint)?;

    msg!("Escrow initialized successfully");
    Ok(())
}
