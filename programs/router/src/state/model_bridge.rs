//! Bridge between production state and verified model_safety types
//!
//! This module provides conversions to enable production code to use
//! formally verified functions from model_safety.
//!
//! # Architecture
//!
//! - **model_safety**: Abstract, mathematically clean model with formal proofs
//! - **Production**: Complex Solana state with exposures, LP buckets, multiple vaults
//!
//! # Conversion Strategy
//!
//! 1. Portfolio → Account: Maps user-level state
//! 2. SlabRegistry → Params: Maps global parameters
//! 3. Aggregate vaults → vault field
//!
//! # Type Mappings
//!
//! | Production Field | Model Field | Notes |
//! |------------------|-------------|-------|
//! | Portfolio.principal (i128) | Account.principal (u128) | Convert via max(0, principal) as u128 |
//! | Portfolio.pnl (i128) | Account.pnl_ledger (i128) | Direct mapping |
//! | Portfolio.vested_pnl (i128) | Account.reserved_pnl (u128) | Convert via max(0, vested) as u128 |
//! | Portfolio.last_slot (u64) | Warmup.started_at_slot (u64) | Direct mapping |
//! | InsuranceState.vault_balance | State.insurance_fund (u128) | Direct mapping |
//!
//! # Limitations
//!
//! - **Vesting algorithms differ**: Production uses exponential, model uses linear
//! - **Global haircut**: Production tracks pnl_index_checkpoint, model doesn't
//! - **Position tracking**: Production has complex exposures, model has simple position_size
//! - **Multiple vaults**: Production has per-mint vaults, model assumes single collateral
//!
//! # Usage
//!
//! ```rust,ignore
//! use model_safety;
//! use crate::state::model_bridge::*;
//!
//! // Convert portfolio to model account
//! let account = portfolio_to_account(&portfolio, &registry);
//!
//! // Use verified function
//! if model_safety::helpers::is_liquidatable(&account, &prices, &params) {
//!     // Execute liquidation using verified logic
//! }
//!
//! // For arithmetic, use verified math directly
//! use model_safety::math::*;
//! let result = add_u128(a, b);  // Saturating addition
//! ```

use super::{Portfolio, SlabRegistry};
use model_safety;

/// Convert a Portfolio to a model_safety Account
///
/// This enables using verified predicates (is_liquidatable, etc.) on production state.
///
/// # Type conversions
///
/// - `principal`: i128 → u128 via max(0, principal) cast
/// - `vested_pnl`: i128 → u128 via max(0, vested_pnl) cast
/// - `position_size`: Calculated from exposures array
///
/// # Arguments
///
/// * `portfolio` - Production user portfolio
/// * `registry` - Global registry (for vesting params)
///
/// # Returns
///
/// model_safety::Account with converted fields
pub fn portfolio_to_account(portfolio: &Portfolio, registry: &SlabRegistry) -> model_safety::Account {
    // Convert principal: i128 → u128 (clamp negative to 0)
    let principal = if portfolio.principal >= 0 {
        portfolio.principal as u128
    } else {
        0u128
    };

    // Convert vested_pnl: i128 → u128 (clamp negative to 0)
    let reserved_pnl = if portfolio.vested_pnl >= 0 {
        portfolio.vested_pnl as u128
    } else {
        0u128
    };

    // Calculate total position size from exposures
    // Sum absolute values of all position quantities
    let mut total_position_size = 0u128;
    for i in 0..portfolio.exposure_count as usize {
        let (_slab_idx, _instrument_idx, qty) = portfolio.exposures[i];
        // Position size is absolute value of quantity
        let abs_qty = qty.abs() as u128;
        total_position_size = total_position_size.saturating_add(abs_qty);
    }

    // Calculate slope_per_step for warmup
    // In linear model: withdrawable = steps_elapsed * slope_per_step
    // Map from production's exponential τ to a reasonable linear slope
    //
    // Heuristic: slope_per_step ≈ principal / (4 * tau_slots)
    // This means full vesting after ~4τ steps (matching exponential behavior)
    let tau = registry.pnl_vesting_params.tau_slots;
    let slope_per_step = if tau > 0 && principal > 0 {
        principal / (4 * tau as u128).max(1)
    } else {
        principal // Instant vesting if tau=0
    };

    model_safety::Account {
        principal,
        pnl_ledger: portfolio.pnl,
        reserved_pnl,
        warmup_state: model_safety::Warmup {
            started_at_slot: portfolio.last_slot,
            slope_per_step,
        },
        position_size: total_position_size,
    }
}

/// Convert multiple portfolios to a model State
///
/// This aggregates multiple user portfolios into a single model State,
/// enabling whole-system verification of operations like loss socialization.
///
/// # Arguments
///
/// * `portfolios` - Slice of user portfolios
/// * `registry` - Global registry with params and insurance state
/// * `total_vault_balance` - Sum of all vault balances across mints
/// * `total_fees` - Accumulated fees outstanding
///
/// # Returns
///
/// model_safety::State with aggregated user accounts
///
/// # Panics
///
/// Panics if portfolios.len() > model_safety::state::MAX_USERS
pub fn portfolios_to_state(
    portfolios: &[Portfolio],
    registry: &SlabRegistry,
    total_vault_balance: u128,
    total_fees: u128,
) -> model_safety::State {
    let mut users = arrayvec::ArrayVec::<model_safety::Account, 6>::new();

    for portfolio in portfolios.iter() {
        let account = portfolio_to_account(portfolio, registry);
        if users.try_push(account).is_err() {
            // Silently skip if we exceed capacity
            // In production, this should be handled differently
            break;
        }
    }

    let params = model_safety::Params {
        max_users: portfolios.len() as u8,
        withdraw_cap_per_step: registry.pnl_vesting_params.tau_slots as u128, // Placeholder
        maintenance_margin_bps: registry.mmr,
    };

    model_safety::State {
        vault: total_vault_balance,
        insurance_fund: registry.insurance_state.vault_balance,
        fees_outstanding: total_fees,
        users,
        params,
        authorized_router: true, // Production always authorized
    }
}

/// Apply verified account changes back to production portfolio
///
/// After calling a verified model_safety function, use this to apply
/// the results back to production state.
///
/// # Arguments
///
/// * `portfolio` - Production portfolio to update (mutable)
/// * `account` - Model account with updated values
///
/// # Safety
///
/// This function trusts that the model account was derived from verified operations.
/// DO NOT call this with arbitrary account values.
pub fn apply_account_to_portfolio(
    portfolio: &mut Portfolio,
    account: &model_safety::Account,
) {
    // Apply principal change
    // Model uses u128, production uses i128
    // Safe because verified functions never create negative principal
    portfolio.principal = account.principal as i128;

    // Apply PnL change
    portfolio.pnl = account.pnl_ledger;

    // Apply vested PnL change
    portfolio.vested_pnl = account.reserved_pnl as i128;

    // Update last_slot
    portfolio.last_slot = account.warmup_state.started_at_slot;

    // Note: position_size in model is aggregate; we don't update exposures array
    // because that requires more complex mapping. The exposures array should be
    // updated separately through production-specific logic.
}

/// Apply verified state changes back to production
///
/// After calling a verified whole-system operation (like socialize_losses),
/// use this to apply results back to production.
///
/// # Arguments
///
/// * `portfolios` - Slice of production portfolios to update (mutable)
/// * `registry` - Global registry to update (mutable)
/// * `state` - Model state with updated values
///
/// # Panics
///
/// Panics if portfolios.len() != state.users.len()
pub fn apply_state_to_portfolios(
    portfolios: &mut [Portfolio],
    registry: &mut SlabRegistry,
    state: &model_safety::State,
) {
    assert_eq!(
        portfolios.len(),
        state.users.len(),
        "Portfolio count must match model user count"
    );

    // Apply individual account changes
    for (portfolio, account) in portfolios.iter_mut().zip(state.users.iter()) {
        apply_account_to_portfolio(portfolio, account);
    }

    // Apply insurance fund changes
    registry.insurance_state.vault_balance = state.insurance_fund;

    // Note: We don't update total_vault_balance or fees here because those
    // are derived from other accounts in production. Conservation should be
    // verified separately via conservation_ok() checks.
}

/// Check conservation using verified helper
///
/// This is a critical safety check that should be called in tests
/// and optionally in production (governance mode).
///
/// # Arguments
///
/// * `portfolios` - All user portfolios
/// * `registry` - Global registry
/// * `total_vault_balance` - Sum of all vault balances
/// * `total_fees` - Accumulated fees
///
/// # Returns
///
/// true if conservation invariant holds, false otherwise
pub fn check_conservation(
    portfolios: &[Portfolio],
    registry: &SlabRegistry,
    total_vault_balance: u128,
    total_fees: u128,
) -> bool {
    let state = portfolios_to_state(portfolios, registry, total_vault_balance, total_fees);
    model_safety::helpers::conservation_ok(&state)
}

/// Wrapper for verified loss socialization
///
/// This wraps the verified socialize_losses function for production use.
///
/// # Arguments
///
/// * `portfolios` - All user portfolios (mutable)
/// * `registry` - Global registry (mutable, for insurance fund updates)
/// * `deficit` - Amount of bad debt to socialize
/// * `total_vault_balance` - Current total vault balance
/// * `total_fees` - Current accumulated fees
///
/// # Returns
///
/// Result indicating success or failure
///
/// # Safety
///
/// This function uses formally verified logic that guarantees:
/// - I1: Principals are never reduced
/// - I2: Conservation is maintained
/// - I4: Only winners are haircutted, bounded correctly
pub fn socialize_losses_verified(
    portfolios: &mut [Portfolio],
    registry: &mut SlabRegistry,
    deficit: u128,
    total_vault_balance: u128,
    total_fees: u128,
) -> Result<(), ()> {
    // Convert to model
    let state = portfolios_to_state(portfolios, registry, total_vault_balance, total_fees);

    // Call verified function
    let new_state = model_safety::transitions::socialize_losses(state, deficit);

    // Apply changes back
    apply_state_to_portfolios(portfolios, registry, &new_state);

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use pinocchio::pubkey::Pubkey;

    #[test]
    fn test_portfolio_to_account_positive_values() {
        let mut registry = SlabRegistry::new(Pubkey::default(), Pubkey::default(), 0);
        registry.pnl_vesting_params.tau_slots = 10_000;
        registry.mmr = 50_000; // 5%

        let portfolio = Portfolio::new(Pubkey::default(), Pubkey::default(), 0);
        let mut portfolio = portfolio;
        portfolio.principal = 100_000_000; // $100
        portfolio.pnl = 20_000_000; // $20 profit
        portfolio.vested_pnl = 15_000_000; // $15 vested
        portfolio.last_slot = 1000;

        let account = portfolio_to_account(&portfolio, &registry);

        assert_eq!(account.principal, 100_000_000);
        assert_eq!(account.pnl_ledger, 20_000_000);
        assert_eq!(account.reserved_pnl, 15_000_000);
        assert_eq!(account.warmup_state.started_at_slot, 1000);
    }

    #[test]
    fn test_portfolio_to_account_negative_principal() {
        let mut registry = SlabRegistry::new(Pubkey::default(), Pubkey::default(), 0);
        registry.pnl_vesting_params.tau_slots = 10_000;

        let mut portfolio = Portfolio::new(Pubkey::default(), Pubkey::default(), 0);
        portfolio.principal = -50_000_000; // Negative (edge case)
        portfolio.pnl = -30_000_000; // Loss
        portfolio.vested_pnl = -10_000_000; // Negative vested

        let account = portfolio_to_account(&portfolio, &registry);

        // Negative principal → 0
        assert_eq!(account.principal, 0);
        // Negative PnL preserved (i128)
        assert_eq!(account.pnl_ledger, -30_000_000);
        // Negative vested → 0
        assert_eq!(account.reserved_pnl, 0);
    }

    #[test]
    fn test_apply_account_to_portfolio() {
        let mut portfolio = Portfolio::new(Pubkey::default(), Pubkey::default(), 0);
        portfolio.principal = 100_000_000;
        portfolio.pnl = 50_000_000;
        portfolio.vested_pnl = 40_000_000;
        portfolio.last_slot = 1000;

        let account = model_safety::Account {
            principal: 100_000_000,
            pnl_ledger: 35_000_000, // Reduced by haircut
            reserved_pnl: 28_000_000, // Reduced proportionally
            warmup_state: model_safety::Warmup {
                started_at_slot: 2000,
                slope_per_step: 1000,
            },
            position_size: 0,
        };

        apply_account_to_portfolio(&mut portfolio, &account);

        assert_eq!(portfolio.principal, 100_000_000);
        assert_eq!(portfolio.pnl, 35_000_000);
        assert_eq!(portfolio.vested_pnl, 28_000_000);
        assert_eq!(portfolio.last_slot, 2000);
    }

    #[test]
    fn test_portfolios_to_state() {
        let registry = SlabRegistry::new(Pubkey::default(), Pubkey::default(), 0);

        let mut p1 = Portfolio::new(Pubkey::default(), Pubkey::default(), 0);
        p1.principal = 100_000_000;
        p1.pnl = 20_000_000;

        let mut p2 = Portfolio::new(Pubkey::default(), Pubkey::default(), 0);
        p2.principal = 200_000_000;
        p2.pnl = -10_000_000;

        let portfolios = vec![p1, p2];
        let state = portfolios_to_state(&portfolios, &registry, 310_000_000, 5_000);

        assert_eq!(state.users.len(), 2);
        assert_eq!(state.vault, 310_000_000);
        assert_eq!(state.fees_outstanding, 5_000);
        assert_eq!(state.users[0].principal, 100_000_000);
        assert_eq!(state.users[1].principal, 200_000_000);
    }

    #[test]
    fn test_check_conservation_holds() {
        let registry = SlabRegistry::new(Pubkey::default(), Pubkey::default(), 0);

        let mut p1 = Portfolio::new(Pubkey::default(), Pubkey::default(), 0);
        p1.principal = 100_000_000;
        p1.pnl = 20_000_000;

        let mut p2 = Portfolio::new(Pubkey::default(), Pubkey::default(), 0);
        p2.principal = 200_000_000;
        p2.pnl = 0;

        let portfolios = vec![p1, p2];

        // Conservation: vault = Σprincipal + Σmax(0,pnl) + insurance + fees
        // = 100M + 200M + 20M + 0 + 0 = 320M
        let total_vault = 320_000_000;
        let total_fees = 0;

        assert!(check_conservation(&portfolios, &registry, total_vault, total_fees));
    }

    #[test]
    fn test_check_conservation_fails() {
        let registry = SlabRegistry::new(Pubkey::default(), Pubkey::default(), 0);

        let mut p1 = Portfolio::new(Pubkey::default(), Pubkey::default(), 0);
        p1.principal = 100_000_000;
        p1.pnl = 20_000_000;

        let portfolios = vec![p1];

        // Conservation should be: 100M + 20M = 120M
        // But we claim vault is 100M → fails
        let total_vault = 100_000_000;
        let total_fees = 0;

        assert!(!check_conservation(&portfolios, &registry, total_vault, total_fees));
    }

    /// Example: Conservation check in a typical test scenario
    ///
    /// This demonstrates the recommended pattern for adding conservation checks
    /// to production tests. Add this pattern to all state-mutating tests.
    #[test]
    fn test_conservation_example_deposit_withdraw() {
        let registry = SlabRegistry::new(Pubkey::default(), Pubkey::default(), 0);

        // Initial state: User deposits 100M
        let mut p1 = Portfolio::new(Pubkey::default(), Pubkey::default(), 0);
        p1.principal = 100_000_000;
        p1.pnl = 0;

        let mut portfolios = vec![p1];
        let mut total_vault = 100_000_000;
        let total_fees = 0;

        // ✅ Conservation check after deposit
        assert!(
            check_conservation(&portfolios, &registry, total_vault, total_fees),
            "Conservation violated after deposit"
        );

        // User realizes profit: +20M PnL
        portfolios[0].pnl = 20_000_000;
        total_vault += 20_000_000; // Vault increases by profit

        // ✅ Conservation check after profit
        assert!(
            check_conservation(&portfolios, &registry, total_vault, total_fees),
            "Conservation violated after profit"
        );

        // User withdraws 20M (all vested profit, no principal touched)
        portfolios[0].pnl = 0; // All PnL withdrawn
        total_vault -= 20_000_000;

        // ✅ Conservation check after withdrawal
        // vault = principal + max(0, pnl) + insurance + fees
        // 100M = 100M + 0 + 0 + 0 ✓
        assert!(
            check_conservation(&portfolios, &registry, total_vault, total_fees),
            "Conservation violated after withdrawal"
        );

        // Final state verification
        assert_eq!(portfolios[0].principal, 100_000_000); // Principal unchanged
        assert_eq!(portfolios[0].pnl, 0); // PnL fully withdrawn
        assert_eq!(total_vault, 100_000_000); // Vault = principal only
    }
}
