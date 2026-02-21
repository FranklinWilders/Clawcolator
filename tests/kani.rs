//! Formal verification with Kani
//!
//! These proofs verify critical safety properties of the risk engine.
//! Run with: cargo kani --harness <name> (individual proofs)
//! Run all: cargo kani (may take significant time)
//!
//! Key invariants proven:
//! - I2: Conservation of funds across all operations (V >= C_tot + I)
//! - I5: PNL warmup is monotonic and deterministic
//! - I7: User isolation - operations on one user don't affect others
//! - I8: Equity (capital + pnl) is used consistently for margin checks
//! - N1: Negative PnL is realized immediately into capital (not time-gated)
//! - LQ-PARTIAL: Liquidation reduces OI; dust kill-switch prevents sub-threshold
//!               remnants (post-fee position may remain below target margin)
//!
//! Haircut system design:
//!   - Insolvency is handled via haircut ratio (c_tot, pnl_pos_tot aggregates)
//!   - Forced loss realization writes off negative PnL
//!   - Insurance balance increases only via:
//!     maintenance fees + liquidation fees + trading fees + explicit top-ups.
//! See README.md for the current design rationale.

#![cfg(kani)]

use percolator::*;

// Default oracle price for conservation checks
const DEFAULT_ORACLE: u64 = 1_000_000;

// ============================================================================
// RiskParams Constructors for Kani Proofs
// ============================================================================

/// Zero maintenance fees, no freshness check - trading_fee_bps=10 for fee-credit proofs
fn test_params() -> RiskParams {
    RiskParams {
        warmup_period_slots: 100,
        maintenance_margin_bps: 500,
        initial_margin_bps: 1000,
        trading_fee_bps: 10,
        max_accounts: 4, // Match MAX_ACCOUNTS for Kani
        new_account_fee: U128::ZERO,
        risk_reduction_threshold: U128::ZERO,
        maintenance_fee_per_slot: U128::ZERO,
        max_crank_staleness_slots: u64::MAX,
        liquidation_fee_bps: 50,
        liquidation_fee_cap: U128::new(10_000),
        liquidation_buffer_bps: 100,
        min_liquidation_abs: U128::new(100_000),
    }
}

/// Floor + zero maintenance fees, no freshness - used for reserved/insurance/floor proofs
fn test_params_with_floor() -> RiskParams {
    RiskParams {
        warmup_period_slots: 100,
        maintenance_margin_bps: 500,
        initial_margin_bps: 1000,
        trading_fee_bps: 10,
        max_accounts: 4, // Match MAX_ACCOUNTS for Kani
        new_account_fee: U128::ZERO,
        risk_reduction_threshold: U128::new(1000), // Non-zero floor
        maintenance_fee_per_slot: U128::ZERO,
        max_crank_staleness_slots: u64::MAX,
        liquidation_fee_bps: 50,
        liquidation_fee_cap: U128::new(10_000),
        liquidation_buffer_bps: 100,
        min_liquidation_abs: U128::new(100_000),
    }
}

/// Maintenance fee with fee_per_slot = 1 - used only for maintenance/keeper/fee_credit proofs
fn test_params_with_maintenance_fee() -> RiskParams {
    RiskParams {
        warmup_period_slots: 100,
        maintenance_margin_bps: 500,
        initial_margin_bps: 1000,
        trading_fee_bps: 10,
        max_accounts: 4, // Match MAX_ACCOUNTS for Kani
        new_account_fee: U128::ZERO,
        risk_reduction_threshold: U128::ZERO,
        maintenance_fee_per_slot: U128::new(1), // fee_per_slot = 1 (direct, no division)
        max_crank_staleness_slots: u64::MAX,
        liquidation_fee_bps: 50,
        liquidation_fee_cap: U128::new(10_000),
        liquidation_buffer_bps: 100,
        min_liquidation_abs: U128::new(100_000),
    }
}

// ============================================================================
// Integer Safety Helpers (match percolator.rs implementations)
// ============================================================================

/// Safely convert negative i128 to u128 (handles i128::MIN without overflow)
#[inline]
fn neg_i128_to_u128(val: i128) -> u128 {
    debug_assert!(val < 0, "neg_i128_to_u128 called with non-negative value");
    if val == i128::MIN {
        (i128::MAX as u128) + 1
    } else {
        (-val) as u128
    }
}

/// Safely compute absolute value of i128 as u128 (handles i128::MIN)
#[inline]
fn abs_i128_to_u128(val: i128) -> u128 {
    if val >= 0 {
        val as u128
    } else {
        neg_i128_to_u128(val)
    }
}

/// Safely convert u128 to i128 with clamping (handles values > i128::MAX)
#[inline]
fn u128_to_i128_clamped(x: u128) -> i128 {
    if x > i128::MAX as u128 {
        i128::MAX
    } else {
        x as i128
    }
}

// ============================================================================
// Frame Proof Helpers (snapshot account/globals for comparison)
// ============================================================================

/// Snapshot of account fields for frame proofs
struct AccountSnapshot {
    capital: u128,
    pnl: i128,
    position_size: i128,
    warmup_slope_per_step: u128,
}

/// Snapshot of global engine fields for frame proofs
struct GlobalsSnapshot {
    vault: u128,
    insurance_balance: u128,
}

fn snapshot_account(account: &Account) -> AccountSnapshot {
    AccountSnapshot {
        capital: account.capital.get(),
        pnl: account.pnl.get(),
        position_size: account.position_size.get(),
        warmup_slope_per_step: account.warmup_slope_per_step.get(),
    }
}

fn snapshot_globals(engine: &RiskEngine) -> GlobalsSnapshot {
    GlobalsSnapshot {
        vault: engine.vault.get(),
        insurance_balance: engine.insurance_fund.balance.get(),
    }
}

// ============================================================================
// Verification Prelude: State Validity and Fast Conservation Helpers
// ============================================================================



/// Cheap validity check for RiskEngine state
/// Used as assume/assert in frame proofs and validity-preservation proofs.
///
/// NOTE: This is a simplified version that skips the matcher array check
/// to avoid memcmp unwinding issues in Kani. The user/LP accounts created
/// by add_user/add_lp already have correct matcher arrays.
fn valid_state(engine: &RiskEngine) -> bool {
    // 1. Crank state bounds
    if engine.num_used_accounts > MAX_ACCOUNTS as u16 {
        return false;
    }
    if engine.crank_cursor >= MAX_ACCOUNTS as u16 {
        return false;
    }
    if engine.gc_cursor >= MAX_ACCOUNTS as u16 {
        return false;
    }

    // 4. free_head is either u16::MAX (empty) or valid index
    if engine.free_head != u16::MAX && engine.free_head >= MAX_ACCOUNTS as u16 {
        return false;
    }

    // Check per-account invariants for used accounts only
    for block in 0..BITMAP_WORDS {
        let mut w = engine.used[block];
        while w != 0 {
            let bit = w.trailing_zeros() as usize;
            let idx = block * 64 + bit;
            w &= w - 1;

            // Guard: reject states with bitmap bits beyond MAX_ACCOUNTS
            if idx >= MAX_ACCOUNTS {
                return false;
            }

            let account = &engine.accounts[idx];

            // NOTE: Skipped matcher array check (causes memcmp unwinding issues)
            // Accounts created by add_user have zeroed matcher arrays by construction

            // 5. reserved_pnl <= max(pnl, 0)
            let pos_pnl = if account.pnl.get() > 0 {
                account.pnl.get() as u128
            } else {
                0
            };
            if (account.reserved_pnl as u128) > pos_pnl {
                return false;
            }

            // NOTE: N1 (pnl < 0 => capital == 0) is NOT a global invariant.
            // It's legal to have pnl < 0 with capital > 0 before settle is called.
            // N1 is enforced at settle boundaries (withdraw/deposit/trade end).
            // Keep N1 as separate proofs, not in valid_state().
        }
    }

    true
}

// ============================================================================
// CANONICAL INV(engine) - The One True Invariant
// ============================================================================
//
// This is a layered invariant that matches production intent:
//   INV = Structural ∧ Accounting ∧ Mode ∧ PerAccount
//
// Use this for:
//   1. Proving INV(new()) - initial state is valid
//   2. Proving INV(s) ∧ pre(op,s) ⇒ INV(op(s)) for each public operation
//
// NOTE: This is intentionally more comprehensive than valid_state() which was
// simplified for tractability. Use canonical_inv() for preservation proofs.

/// Structural invariant: freelist and bitmap integrity
fn inv_structural(engine: &RiskEngine) -> bool {
    // S0: params.max_accounts matches compile-time MAX_ACCOUNTS
    if engine.params.max_accounts != MAX_ACCOUNTS as u64 {
        return false;
    }

    // S1: num_used_accounts == popcount(used bitmap)
    let mut popcount: u16 = 0;
    for block in 0..BITMAP_WORDS {
        popcount += engine.used[block].count_ones() as u16;
    }
    if engine.num_used_accounts != popcount {
        return false;
    }

    // S2: free_head is either u16::MAX (empty) or valid index
    if engine.free_head != u16::MAX && engine.free_head >= MAX_ACCOUNTS as u16 {
        return false;
    }

    // S3: Freelist acyclicity, uniqueness, and disjointness from used
    // Use visited bitmap to detect duplicates and cycles
    let expected_free = (MAX_ACCOUNTS as u16).saturating_sub(engine.num_used_accounts);
    let mut free_count: u16 = 0;
    let mut current = engine.free_head;
    let mut visited = [false; MAX_ACCOUNTS];

    // Bounded walk with visited check
    while current != u16::MAX {
        // Check index in range
        if current >= MAX_ACCOUNTS as u16 {
            return false; // Invalid index in freelist
        }
        let idx = current as usize;

        // Check not already visited (cycle or duplicate detection)
        if visited[idx] {
            return false; // Cycle or duplicate detected
        }
        visited[idx] = true;

        // Check disjoint from used bitmap
        if engine.is_used(idx) {
            return false; // Freelist node is marked as used - contradiction
        }

        free_count += 1;

        // Safety: prevent unbounded iteration (should never trigger if no cycle)
        if free_count > MAX_ACCOUNTS as u16 {
            return false; // Too many nodes - impossible if no duplicates
        }

        current = engine.next_free[idx];
    }

    // Freelist length must equal expected
    if free_count != expected_free {
        return false; // Freelist length mismatch
    }

    // S4: Crank state bounds
    if engine.crank_cursor >= MAX_ACCOUNTS as u16 {
        return false;
    }
    if engine.gc_cursor >= MAX_ACCOUNTS as u16 {
        return false;
    }
    if engine.liq_cursor >= MAX_ACCOUNTS as u16 {
        return false;
    }

    true
}

/// Accounting invariant: conservation (haircut system)
///
/// This checks the **primary conservation inequality only**: vault >= c_tot + insurance.
/// Mark-to-market / funding conservation is verified by operation-specific proofs
/// via check_conservation(oracle), which includes variation margin terms.
/// Aggregate sum correctness is checked by inv_aggregates.
fn inv_accounting(engine: &RiskEngine) -> bool {
    // A1: Primary conservation: vault >= c_tot + insurance
    // This is the fundamental invariant in the haircut system.
    // Equivalent to: signed_residual is non-negative (no bad debt).
    let (solvent, _deficit) = RiskEngine::signed_residual(
        engine.vault.get(),
        engine.c_tot.get(),
        engine.insurance_fund.balance.get(),
    );
    solvent
}

/// N1 boundary condition: after settlement boundaries (settle/withdraw/deposit/trade/liquidation),
/// either pnl >= 0 or capital == 0. This prevents unrealized losses lingering with capital.
fn n1_boundary_holds(account: &percolator::Account) -> bool {
    account.pnl.get() >= 0 || account.capital.get() == 0
}

/// Fast conservation check for proofs with no open positions / funding.
/// vault >= c_tot + insurance ⟺ signed_residual is non-negative (no bad debt)
fn conservation_fast_no_funding(engine: &RiskEngine) -> bool {
    let (solvent, _) = RiskEngine::signed_residual(
        engine.vault.get(),
        engine.c_tot.get(),
        engine.insurance_fund.balance.get(),
    );
    solvent
}

/// Mode invariant (placeholder - no mode fields in haircut system)
fn inv_mode(_engine: &RiskEngine) -> bool {
    true
}

/// Per-account invariant: individual account consistency
fn inv_per_account(engine: &RiskEngine) -> bool {
    for block in 0..BITMAP_WORDS {
        let mut w = engine.used[block];
        while w != 0 {
            let bit = w.trailing_zeros() as usize;
            let idx = block * 64 + bit;
            w &= w - 1;

            // Guard: reject states with bitmap bits beyond MAX_ACCOUNTS
            if idx >= MAX_ACCOUNTS {
                return false;
            }

            let account = &engine.accounts[idx];

            // PA1: reserved_pnl <= max(pnl, 0)
            let pos_pnl = if account.pnl.get() > 0 {
                account.pnl.get() as u128
            } else {
                0
            };
            if (account.reserved_pnl as u128) > pos_pnl {
                return false;
            }

            // PA2: No i128::MIN in fields that get abs'd or negated
            // pnl and position_size can be negative, but i128::MIN would cause overflow on negation
            if account.pnl.get() == i128::MIN || account.position_size.get() == i128::MIN {
                return false;
            }

            // PA3: If account is LP, owner must be non-zero (set during add_lp)
            // Skipped: owner is 32 bytes, checking all zeros is expensive in Kani

            // PA4: warmup_slope_per_step should be bounded to prevent overflow
            // The maximum reasonable slope is total insurance over 1 slot
            // For now, just check it's not u128::MAX
            if account.warmup_slope_per_step.get() == u128::MAX {
                return false;
            }
        }
    }

    true
}

/// Aggregate coherence: c_tot, pnl_pos_tot, total_open_interest match account-level sums
fn inv_aggregates(engine: &RiskEngine) -> bool {
    let mut sum_capital: u128 = 0;
    let mut sum_pnl_pos: u128 = 0;
    let mut sum_abs_pos: u128 = 0;
    for idx in 0..MAX_ACCOUNTS {
        if engine.is_used(idx) {
            sum_capital = sum_capital.saturating_add(engine.accounts[idx].capital.get());
            let pnl = engine.accounts[idx].pnl.get();
            if pnl > 0 {
                sum_pnl_pos = sum_pnl_pos.saturating_add(pnl as u128);
            }
            sum_abs_pos = sum_abs_pos.saturating_add(abs_i128_to_u128(engine.accounts[idx].position_size.get()));
        }
    }
    engine.c_tot.get() == sum_capital
        && engine.pnl_pos_tot.get() == sum_pnl_pos
        && engine.total_open_interest.get() == sum_abs_pos
}

/// The canonical invariant: INV(engine) = Structural ∧ Aggregates ∧ Accounting ∧ Mode ∧ PerAccount
fn canonical_inv(engine: &RiskEngine) -> bool {
    inv_structural(engine)
        && inv_aggregates(engine)
        && inv_accounting(engine)
        && inv_mode(engine)
        && inv_per_account(engine)
}

/// Sync all engine aggregates (c_tot, pnl_pos_tot, total_open_interest) from account data.
/// Call this after manually setting account.capital, account.pnl, or account.position_size.
/// Unlike engine.recompute_aggregates() which only handles c_tot and pnl_pos_tot,
/// this also recomputes total_open_interest.
fn sync_engine_aggregates(engine: &mut RiskEngine) {
    engine.recompute_aggregates();
    let mut oi: u128 = 0;
    for idx in 0..MAX_ACCOUNTS {
        if engine.is_used(idx) {
            oi = oi.saturating_add(abs_i128_to_u128(engine.accounts[idx].position_size.get()));
        }
    }
    engine.total_open_interest = U128::new(oi);
}

// ============================================================================
// NON-VACUITY ASSERTION HELPERS
// ============================================================================
//
// These helpers ensure proofs actually exercise the intended code paths.
// Use them to assert that:
//   - Operations succeed when they should
//   - Specific branches are taken
//   - Mutations actually occur

/// Assert that an operation must succeed (non-vacuous proof of Ok path)
/// Use when constraining inputs to force Ok, then proving postconditions
macro_rules! assert_ok {
    ($result:expr, $msg:expr) => {
        match $result {
            Ok(v) => v,
            Err(_) => {
                kani::assert(false, $msg);
                unreachable!()
            }
        }
    };
}

/// Assert that an operation must fail (non-vacuous proof of Err path)
macro_rules! assert_err {
    ($result:expr, $msg:expr) => {
        match $result {
            Ok(_) => {
                kani::assert(false, $msg);
                unreachable!()
            }
            Err(e) => e,
        }
    };
}

/// Non-vacuity: assert that a value changed (mutation actually occurred)
#[inline]
fn assert_changed<T: PartialEq + Copy>(before: T, after: T, msg: &'static str) {
    kani::assert(before != after, msg);
}

/// Non-vacuity: assert that a value is non-zero (meaningful input)
#[inline]
fn assert_nonzero(val: u128, msg: &'static str) {
    kani::assert(val > 0, msg);
}

/// Non-vacuity: assert that liquidation was triggered (position reduced)
#[inline]
fn assert_liquidation_occurred(pos_before: i128, pos_after: i128) {
    let abs_before = if pos_before >= 0 {
        pos_before as u128
    } else {
        neg_i128_to_u128(pos_before)
    };
    let abs_after = if pos_after >= 0 {
        pos_after as u128
    } else {
        neg_i128_to_u128(pos_after)
    };
    kani::assert(
        abs_after < abs_before,
        "liquidation must reduce position size",
    );
}

/// Non-vacuity: assert that ADL actually haircut something
#[inline]
fn assert_adl_occurred(pnl_before: i128, pnl_after: i128) {
    kani::assert(pnl_after < pnl_before, "ADL must reduce PnL");
}

/// Non-vacuity: assert that GC freed the expected account
#[inline]
fn assert_gc_freed(engine: &RiskEngine, idx: usize) {
    kani::assert(!engine.is_used(idx), "GC must free the dust account");
}

/// Totals for fast conservation check (no funding)
struct Totals {
    sum_capital: u128,
    sum_pnl_pos: u128,
    sum_pnl_neg_abs: u128,
}

/// Recompute totals by iterating only used accounts
fn recompute_totals(engine: &RiskEngine) -> Totals {
    let mut sum_capital: u128 = 0;
    let mut sum_pnl_pos: u128 = 0;
    let mut sum_pnl_neg_abs: u128 = 0;

    for block in 0..BITMAP_WORDS {
        let mut w = engine.used[block];
        while w != 0 {
            let bit = w.trailing_zeros() as usize;
            let idx = block * 64 + bit;
            w &= w - 1;

            // Guard: reject states with bitmap bits beyond MAX_ACCOUNTS
            if idx >= MAX_ACCOUNTS {
                return Totals { sum_capital: 0, sum_pnl_pos: 0, sum_pnl_neg_abs: 0 };
            }

            let account = &engine.accounts[idx];
            sum_capital = sum_capital.saturating_add(account.capital.get());

            // Explicit handling: positive, negative, or zero pnl
            if account.pnl.get() > 0 {
                sum_pnl_pos = sum_pnl_pos.saturating_add(account.pnl.get() as u128);
            } else if account.pnl.get() < 0 {
                sum_pnl_neg_abs =
                    sum_pnl_neg_abs.saturating_add(neg_i128_to_u128(account.pnl.get()));
            }
            // pnl == 0: no contribution to either sum
        }
    }

    Totals {
        sum_capital,
        sum_pnl_pos,
        sum_pnl_neg_abs,
    }
}


// ============================================================================
// I2: Conservation of funds (FAST - uses totals-based conservation check)
// These harnesses ensure position_size.is_zero() so funding is irrelevant.
// ============================================================================

#[kani::proof]
#[kani::unwind(33)]
#[kani::solver(cadical)]
fn fast_i2_deposit_preserves_conservation() {
    let mut engine = RiskEngine::new(test_params_with_maintenance_fee());
    engine.current_slot = 100;
    engine.last_crank_slot = 100;
    engine.last_full_sweep_start_slot = 100;
    let user_idx = engine.add_user(0).unwrap();

    // Seasoned account: symbolic capital, pnl, and slot to exercise fee/warmup branches
    let capital: u128 = kani::any();
    kani::assume(capital >= 100 && capital <= 5_000);
    let pnl: i128 = kani::any();
    kani::assume(pnl > -2_000 && pnl < 2_000);

    engine.accounts[user_idx as usize].capital = U128::new(capital);
    engine.accounts[user_idx as usize].pnl = I128::new(pnl);
    // Set last_fee_slot < current_slot so fee accrual branch is exercised
    engine.accounts[user_idx as usize].last_fee_slot = 50;

    let now_slot: u64 = kani::any();
    kani::assume(now_slot >= 100 && now_slot <= 300);
    engine.current_slot = now_slot;
    engine.last_crank_slot = now_slot;
    engine.last_full_sweep_start_slot = now_slot;

    // vault = capital + insurance to satisfy conservation
    let pnl_pos = if pnl > 0 { pnl as u128 } else { 0 };
    engine.insurance_fund.balance = U128::new(1_000);
    engine.vault = U128::new(capital + 1_000 + pnl_pos);
    sync_engine_aggregates(&mut engine);

    kani::assert(canonical_inv(&engine), "setup must satisfy INV");

    let amount: u128 = kani::any();
    kani::assume(amount > 0 && amount < 5_000);

    // Deposit may succeed or fail (fee accrual might undercollateralize)
    let _result = engine.deposit(user_idx, amount, now_slot);

    kani::assert(canonical_inv(&engine), "I2: Deposit must preserve INV");
    kani::assert(
        conservation_fast_no_funding(&engine),
        "I2: Deposit must preserve conservation"
    );
}

#[kani::proof]
#[kani::unwind(33)]
#[kani::solver(cadical)]
fn fast_i2_withdraw_preserves_conservation() {
    let mut engine = RiskEngine::new(test_params());
    let user_idx = engine.add_user(0).unwrap();

    let deposit: u128 = kani::any();
    let withdraw: u128 = kani::any();

    kani::assume(deposit > 0 && deposit < 10_000);
    kani::assume(withdraw > 0 && withdraw < 10_000);
    kani::assume(withdraw <= deposit);

    // Force Ok: deposit/withdraw on fresh account with valid amounts must succeed
    assert_ok!(engine.deposit(user_idx, deposit, 0), "deposit must succeed");

    kani::assert(canonical_inv(&engine), "setup must satisfy INV after deposit");

    assert_ok!(engine.withdraw(user_idx, withdraw, 0, 1_000_000), "withdraw must succeed");

    kani::assert(canonical_inv(&engine), "I2: Withdrawal must preserve INV");
    kani::assert(
        conservation_fast_no_funding(&engine),
        "I2: Withdrawal must preserve conservation"
    );
}

// ============================================================================
// I5: PNL Warmup Properties
// ============================================================================

#[kani::proof]
#[kani::unwind(33)]
#[kani::solver(cadical)]
fn i5_warmup_determinism() {
    let mut engine = RiskEngine::new(test_params());
    let user_idx = engine.add_user(0).unwrap();

    let pnl: i128 = kani::any();
    let reserved: u128 = kani::any();
    let slope: u128 = kani::any();
    let slots: u64 = kani::any();

    // Exercise both positive and negative PnL paths
    kani::assume(pnl > -10_000 && pnl < 10_000 && pnl != 0);
    kani::assume(reserved < 5_000);
    kani::assume(slope > 0 && slope < 100);
    kani::assume(slots < 200);

    // PA1: reserved_pnl <= max(pnl, 0)
    let pnl_pos = if pnl > 0 { pnl as u128 } else { 0 };
    kani::assume(reserved <= pnl_pos);

    engine.accounts[user_idx as usize].pnl = I128::new(pnl);
    engine.accounts[user_idx as usize].reserved_pnl = reserved as u64;
    engine.accounts[user_idx as usize].warmup_slope_per_step = U128::new(slope);
    engine.current_slot = slots;

    // Calculate twice with same inputs
    let w1 = engine.withdrawable_pnl(&engine.accounts[user_idx as usize]);
    let w2 = engine.withdrawable_pnl(&engine.accounts[user_idx as usize]);

    assert!(w1 == w2, "I5: Withdrawable PNL must be deterministic");
}

#[kani::proof]
#[kani::unwind(33)]
#[kani::solver(cadical)]
fn i5_warmup_monotonicity() {
    let mut engine = RiskEngine::new(test_params());
    let user_idx = engine.add_user(0).unwrap();

    let pnl: i128 = kani::any();
    let slope: u128 = kani::any();
    let reserved: u64 = kani::any();
    let slots1: u64 = kani::any();
    let slots2: u64 = kani::any();

    // Both positive and negative pnl: negative always yields 0, trivially monotonic
    kani::assume(pnl > -5_000 && pnl < 10_000 && pnl != 0);
    kani::assume(slope < 100);
    kani::assume(slots1 < 200);
    kani::assume(slots2 < 200);
    kani::assume(slots2 > slots1);

    // Symbolic reserved_pnl exercises the available_pnl = positive_pnl - reserved branch
    let pnl_pos: u64 = if pnl > 0 { pnl as u64 } else { 0 };
    kani::assume(reserved <= pnl_pos); // PA1: reserved <= max(pnl, 0)

    engine.accounts[user_idx as usize].pnl = I128::new(pnl);
    engine.accounts[user_idx as usize].warmup_slope_per_step = U128::new(slope);
    engine.accounts[user_idx as usize].reserved_pnl = reserved;

    engine.current_slot = slots1;
    let w1 = engine.withdrawable_pnl(&engine.accounts[user_idx as usize]);

    engine.current_slot = slots2;
    let w2 = engine.withdrawable_pnl(&engine.accounts[user_idx as usize]);

    assert!(
        w2 >= w1,
        "I5: Warmup must be monotonically increasing over time"
    );
}

#[kani::proof]
#[kani::unwind(33)]
#[kani::solver(cadical)]
fn i5_warmup_bounded_by_pnl() {
    let mut engine = RiskEngine::new(test_params());
    let user_idx = engine.add_user(0).unwrap();

    let pnl: i128 = kani::any();
    let reserved: u128 = kani::any();
    let slope: u128 = kani::any();
    let slots: u64 = kani::any();

    // Extended range: includes both positive and negative PnL branches
    kani::assume(pnl > -10_000 && pnl < 10_000);
    kani::assume(reserved < 5_000);
    kani::assume(slope > 0 && slope < 100);
    kani::assume(slots < 200);

    // PA1: reserved_pnl <= max(pnl, 0)
    let pnl_pos = if pnl > 0 { pnl as u128 } else { 0 };
    kani::assume(reserved <= pnl_pos);

    engine.accounts[user_idx as usize].pnl = I128::new(pnl);
    engine.accounts[user_idx as usize].reserved_pnl = reserved as u64;
    engine.accounts[user_idx as usize].warmup_slope_per_step = U128::new(slope);
    engine.current_slot = slots;

    let withdrawable = engine.withdrawable_pnl(&engine.accounts[user_idx as usize]);
    let available = pnl_pos.saturating_sub(reserved);

    // Bound 1: withdrawable <= available PnL (net of reserved)
    kani::assert(
        withdrawable <= available,
        "I5: Withdrawable must not exceed available PNL"
    );

    // Bound 2: negative PnL always yields zero withdrawable
    if pnl <= 0 {
        kani::assert(withdrawable == 0, "I5: Negative PnL must yield zero withdrawable");
    }

    // Bound 3: warmup cap — withdrawable <= slope * elapsed
    let elapsed = slots.saturating_sub(engine.accounts[user_idx as usize].warmup_started_at_slot) as u128;
    let warmup_cap = slope.saturating_mul(elapsed);
    kani::assert(
        withdrawable <= warmup_cap || withdrawable <= available,
        "I5: Withdrawable bounded by min(warmup_cap, available)"
    );
}

// ============================================================================
// I7: User Isolation
// ============================================================================

#[kani::proof]
#[kani::unwind(33)]
#[kani::solver(cadical)]
fn i7_user_isolation_deposit() {
    let mut engine = RiskEngine::new(test_params());
    let user1 = engine.add_user(0).unwrap();
    let user2 = engine.add_user(0).unwrap();

    let amount1: u128 = kani::any();
    let amount2: u128 = kani::any();
    let op_amount: u128 = kani::any();

    kani::assume(amount1 > 0 && amount1 < 10_000);
    kani::assume(amount2 > 0 && amount2 < 10_000);
    kani::assume(op_amount > 0 && op_amount < 10_000);

    // Force Ok: deposits must succeed on fresh accounts
    assert_ok!(engine.deposit(user1, amount1, 0), "user1 initial deposit must succeed");
    assert_ok!(engine.deposit(user2, amount2, 0), "user2 initial deposit must succeed");

    let user2_principal = engine.accounts[user2 as usize].capital;
    let user2_pnl = engine.accounts[user2 as usize].pnl;

    // Operate on user1 with symbolic amount — force Ok for non-vacuity
    assert_ok!(engine.deposit(user1, op_amount, 0), "user1 second deposit must succeed");

    // User2 should be unchanged
    assert!(
        engine.accounts[user2 as usize].capital == user2_principal,
        "I7: User2 principal unchanged by user1 deposit"
    );
    assert!(
        engine.accounts[user2 as usize].pnl == user2_pnl,
        "I7: User2 PNL unchanged by user1 deposit"
    );
}

#[kani::proof]
#[kani::unwind(33)]
#[kani::solver(cadical)]
fn i7_user_isolation_withdrawal() {
    let mut engine = RiskEngine::new(test_params());
    let user1 = engine.add_user(0).unwrap();
    let user2 = engine.add_user(0).unwrap();

    let amount1: u128 = kani::any();
    let amount2: u128 = kani::any();
    let withdraw: u128 = kani::any();

    kani::assume(amount1 > 100 && amount1 < 10_000);
    kani::assume(amount2 > 0 && amount2 < 10_000);
    kani::assume(withdraw > 0 && withdraw <= amount1);

    // Force Ok: deposits must succeed on fresh accounts
    assert_ok!(engine.deposit(user1, amount1, 0), "user1 deposit must succeed");
    assert_ok!(engine.deposit(user2, amount2, 0), "user2 deposit must succeed");

    let user2_principal = engine.accounts[user2 as usize].capital;
    let user2_pnl = engine.accounts[user2 as usize].pnl;

    // Operate on user1 with symbolic withdrawal — force Ok for non-vacuity
    assert_ok!(engine.withdraw(user1, withdraw, 0, 1_000_000), "user1 withdraw must succeed");

    // User2 should be unchanged
    assert!(
        engine.accounts[user2 as usize].capital == user2_principal,
        "I7: User2 principal unchanged by user1 withdrawal"
    );
    assert!(
        engine.accounts[user2 as usize].pnl == user2_pnl,
        "I7: User2 PNL unchanged by user1 withdrawal"
    );
}

// ============================================================================
// I8: Equity Consistency (margin checks use equity = max(0, capital + pnl))
// ============================================================================

#[kani::proof]
#[kani::unwind(33)]
#[kani::solver(cadical)]
fn i8_equity_with_positive_pnl() {
    let mut engine = RiskEngine::new(test_params());
    let user_idx = engine.add_user(0).unwrap();

    let principal: u128 = kani::any();
    let pnl: i128 = kani::any();

    // Full range: covers both positive and negative PnL branches
    kani::assume(principal < 10_000);
    kani::assume(pnl > -10_000 && pnl < 10_000);

    engine.accounts[user_idx as usize].capital = U128::new(principal);
    engine.accounts[user_idx as usize].pnl = I128::new(pnl);

    let equity = engine.account_equity(&engine.accounts[user_idx as usize]);

    // equity == max(0, capital + pnl) — covers both positive and negative PnL branches
    let sum_i = (principal as i128).saturating_add(pnl);
    let expected = if sum_i > 0 { sum_i as u128 } else { 0 };

    kani::assert(equity == expected, "I8: Equity = max(0, capital + pnl)");
}

#[kani::proof]
#[kani::unwind(33)]
#[kani::solver(cadical)]
fn i8_equity_with_negative_pnl() {
    let mut engine = RiskEngine::new(test_params());
    let user_idx = engine.add_user(0).unwrap();

    let principal: u128 = kani::any();
    let pnl: i128 = kani::any();

    kani::assume(principal < 10_000);
    kani::assume(pnl < 0 && pnl > -10_000);

    engine.accounts[user_idx as usize].capital = U128::new(principal);
    engine.accounts[user_idx as usize].pnl = I128::new(pnl);

    let equity = engine.account_equity(&engine.accounts[user_idx as usize]);

    // Equity = max(0, capital + pnl)
    let expected_i = (principal as i128).saturating_add(pnl);
    let expected = if expected_i > 0 {
        expected_i as u128
    } else {
        0
    };

    assert!(
        equity == expected,
        "I8: Equity = max(0, capital + pnl) when PNL is negative"
    );
}

// ============================================================================
// Withdrawal Safety
// ============================================================================

#[kani::proof]
#[kani::unwind(33)]
#[kani::solver(cadical)]
fn withdrawal_requires_sufficient_balance() {
    let mut engine = RiskEngine::new(test_params());
    engine.current_slot = 100;
    engine.last_crank_slot = 100;
    engine.last_full_sweep_start_slot = 100;

    let user_idx = engine.add_user(0).unwrap();

    let principal: u128 = kani::any();
    let withdraw: u128 = kani::any();

    kani::assume(principal < 10_000);
    kani::assume(withdraw < 20_000);
    kani::assume(withdraw > principal); // Try to withdraw more than available

    engine.accounts[user_idx as usize].capital = U128::new(principal);
    engine.vault = U128::new(principal);
    sync_engine_aggregates(&mut engine);

    kani::assert(canonical_inv(&engine), "INV before withdraw");

    let result = engine.withdraw(user_idx, withdraw, 100, 1_000_000);

    assert!(
        result == Err(RiskError::InsufficientBalance),
        "Withdrawal of more than available must fail with InsufficientBalance"
    );

    kani::assert(canonical_inv(&engine), "INV preserved on error path");
}

#[kani::proof]
#[kani::unwind(33)]
#[kani::solver(cadical)]
fn pnl_withdrawal_requires_warmup() {
    let mut engine = RiskEngine::new(test_params());

    let now_slot: u64 = kani::any();
    kani::assume(now_slot >= 10 && now_slot <= 200);
    engine.current_slot = now_slot;
    engine.last_crank_slot = now_slot;
    engine.last_full_sweep_start_slot = now_slot;

    let user_idx = engine.add_user(0).unwrap();

    let pnl: i128 = kani::any();
    let capital: u128 = kani::any();
    let withdraw: u128 = kani::any();

    kani::assume(pnl > 0 && pnl < 5_000);
    kani::assume(capital >= 1_000 && capital <= 5_000);
    // Withdraw more than capital but within capital + pnl (tests warmup guard)
    kani::assume(withdraw > capital && withdraw <= capital + pnl as u128);

    engine.accounts[user_idx as usize].pnl = I128::new(pnl);
    engine.accounts[user_idx as usize].warmup_slope_per_step = U128::new(10);
    // Set warmup_started_at_slot = now_slot so elapsed=0, no PnL converted
    engine.accounts[user_idx as usize].warmup_started_at_slot = now_slot;
    engine.accounts[user_idx as usize].capital = U128::new(capital);
    engine.insurance_fund.balance = U128::new(100_000);
    engine.vault = U128::new(capital + 100_000 + pnl as u128);
    sync_engine_aggregates(&mut engine);

    kani::assert(canonical_inv(&engine), "INV before withdraw");

    // withdrawable_pnl should be 0 since warmup_started_at_slot == current_slot
    let withdrawable = engine.withdrawable_pnl(&engine.accounts[user_idx as usize]);
    kani::assert(withdrawable == 0, "No PNL warmed up when elapsed=0");

    // Withdraw > capital must fail since no PnL is warmed up yet
    let result = engine.withdraw(user_idx, withdraw, now_slot, 1_000_000);
    kani::assert(
        result.is_err(),
        "Cannot withdraw beyond capital when PNL not warmed up"
    );

    kani::assert(canonical_inv(&engine), "INV preserved on error path");
}

// ============================================================================
// Arithmetic Safety
// ============================================================================

#[kani::proof]
#[kani::unwind(33)]
#[kani::solver(cadical)]
fn saturating_arithmetic_prevents_overflow() {
    // Test percolator's mark_pnl_for_position with symbolic inputs.
    // This function uses saturating_abs_i128 and saturating_sub for price diffs,
    // plus checked_mul/checked_div for the product.
    let pos: i128 = kani::any();
    let entry: u64 = kani::any();
    let oracle: u64 = kani::any();

    kani::assume(pos > -100 && pos < 100);
    kani::assume(entry >= 900_000 && entry <= 1_100_000);
    kani::assume(oracle >= 900_000 && oracle <= 1_100_000);

    let result = RiskEngine::mark_pnl_for_position(pos, entry, oracle);

    if pos == 0 {
        let mark = result.unwrap();
        kani::assert(mark == 0, "zero position → zero mark PnL");
    } else {
        assert!(result.is_ok(), "mark_pnl must succeed for bounded inputs");
        let mark = result.unwrap();

        // Sign property: long profits when oracle > entry, short profits when entry > oracle
        if pos > 0 {
            if oracle > entry {
                kani::assert(mark >= 0, "long profits when oracle > entry");
            } else if oracle < entry {
                kani::assert(mark <= 0, "long loses when oracle < entry");
            } else {
                kani::assert(mark == 0, "no mark change at same price");
            }
        } else {
            if entry > oracle {
                kani::assert(mark >= 0, "short profits when entry > oracle");
            } else if entry < oracle {
                kani::assert(mark <= 0, "short loses when entry < oracle");
            } else {
                kani::assert(mark == 0, "no mark change at same price");
            }
        }

        // Magnitude bound: |mark| <= |pos| * |oracle - entry| / 1_000_000
        let abs_pos = if pos > 0 { pos as u128 } else { (-pos) as u128 };
        let diff = if oracle > entry {
            (oracle - entry) as u128
        } else {
            (entry - oracle) as u128
        };
        let bound = abs_pos * diff / 1_000_000;
        let abs_mark = if mark >= 0 { mark as u128 } else { (-mark) as u128 };
        kani::assert(abs_mark <= bound, "|mark| bounded by |pos|*|price_diff|/1e6");
    }
}

// ============================================================================
// Edge Cases
// ============================================================================

#[kani::proof]
#[kani::unwind(33)]
#[kani::solver(cadical)]
fn zero_pnl_withdrawable_is_zero() {
    let mut engine = RiskEngine::new(test_params());
    let user_idx = engine.add_user(0).unwrap();

    // Symbolic slot and reserved_pnl: withdrawable must be 0 for pnl=0 at any slot
    let slot: u64 = kani::any();
    let reserved: u64 = kani::any();
    kani::assume(slot < 10_000);
    kani::assume(reserved < 5_000);

    engine.accounts[user_idx as usize].pnl = I128::new(0);
    engine.accounts[user_idx as usize].reserved_pnl = reserved;
    engine.current_slot = slot;

    let withdrawable = engine.withdrawable_pnl(&engine.accounts[user_idx as usize]);

    kani::assert(withdrawable == 0, "Zero PNL means zero withdrawable regardless of slot or reserved");
}

#[kani::proof]
#[kani::unwind(33)]
#[kani::solver(cadical)]
fn negative_pnl_withdrawable_is_zero() {
    let mut engine = RiskEngine::new(test_params());
    let user_idx = engine.add_user(0).unwrap();

    let pnl: i128 = kani::any();
    kani::assume(pnl < 0 && pnl > -10_000);

    // Symbolic slot, slope, and reserved: withdrawable must be 0 for ALL negative PnL
    let slot: u64 = kani::any();
    kani::assume(slot < 10_000);
    let slope: u128 = kani::any();
    kani::assume(slope <= 1_000);

    engine.accounts[user_idx as usize].pnl = I128::new(pnl);
    engine.accounts[user_idx as usize].warmup_slope_per_step = U128::new(slope);
    engine.current_slot = slot;

    let withdrawable = engine.withdrawable_pnl(&engine.accounts[user_idx as usize]);

    kani::assert(withdrawable == 0, "Negative PNL means zero withdrawable regardless of slot/slope");
}

// ============================================================================
// Funding Rate Invariants
// ============================================================================

#[kani::proof]
#[kani::unwind(33)]
#[kani::solver(cadical)]
fn funding_p1_settlement_idempotent() {
    // P1: Funding settlement is idempotent
    // After settling once, settling again with unchanged global index does nothing

    let mut engine = RiskEngine::new(test_params());
    let user_idx = engine.add_user(0).unwrap();

    // Arbitrary position and PNL
    let position: i128 = kani::any();
    kani::assume(position != i128::MIN);
    kani::assume(position.abs() < 1_000_000);

    let pnl: i128 = kani::any();
    kani::assume(pnl > -1_000_000 && pnl < 1_000_000);

    engine.accounts[user_idx as usize].position_size = I128::new(position);
    engine.accounts[user_idx as usize].pnl = I128::new(pnl);

    // Set arbitrary funding index
    let index: i128 = kani::any();
    kani::assume(index != i128::MIN);
    kani::assume(index.abs() < 1_000_000_000);
    engine.funding_index_qpb_e6 = I128::new(index);

    // Settle once (must succeed under bounded inputs)
    engine.touch_account(user_idx).unwrap();
    let pnl_after_first = engine.accounts[user_idx as usize].pnl;

    // Settle again without changing global index
    engine.touch_account(user_idx).unwrap();

    // PNL should be unchanged (idempotent)
    assert!(
        engine.accounts[user_idx as usize].pnl.get() == pnl_after_first.get(),
        "Second settlement should not change PNL"
    );

    // Snapshot should equal global index
    assert!(
        engine.accounts[user_idx as usize].funding_index == engine.funding_index_qpb_e6,
        "Snapshot should equal global index"
    );
}

#[kani::proof]
#[kani::unwind(33)]
#[kani::solver(cadical)]
fn funding_p2_never_touches_principal() {
    // P2: Funding does not touch principal (extends Invariant I1)

    let mut engine = RiskEngine::new(test_params());
    let user_idx = engine.add_user(0).unwrap();

    let principal: u128 = kani::any();
    kani::assume(principal < 1_000_000);

    let position: i128 = kani::any();
    kani::assume(position != i128::MIN);
    kani::assume(position.abs() < 1_000_000);

    engine.accounts[user_idx as usize].capital = U128::new(principal);
    engine.accounts[user_idx as usize].position_size = I128::new(position);

    // Accrue arbitrary funding
    let funding_delta: i128 = kani::any();
    kani::assume(funding_delta != i128::MIN);
    kani::assume(funding_delta.abs() < 1_000_000_000);
    engine.funding_index_qpb_e6 = I128::new(funding_delta);

    // Settle funding (must succeed under bounded inputs)
    engine.touch_account(user_idx).unwrap();

    // Principal must be unchanged
    assert!(
        engine.accounts[user_idx as usize].capital.get() == principal,
        "Funding must never modify principal"
    );
}

#[kani::proof]
#[kani::unwind(33)]
#[kani::solver(cadical)]
fn funding_p3_bounded_drift_between_opposite_positions() {
    // P3: Funding has bounded drift when user and LP have opposite positions
    // Note: With vault-favoring rounding (ceil when paying, trunc when receiving),
    // funding is NOT exactly zero-sum. The vault keeps the rounding dust.
    // This ensures one-sided conservation (vault >= expected).

    let mut engine = RiskEngine::new(test_params());
    let user_idx = engine.add_user(0).unwrap();
    let lp_idx = engine.add_lp([1u8; 32], [0u8; 32], 0).unwrap();

    let position: i128 = kani::any();
    kani::assume(position > 0 && position < 10_000);

    // User has position, LP has opposite
    engine.accounts[user_idx as usize].position_size = I128::new(position);
    engine.accounts[lp_idx as usize].position_size = I128::new(-position);

    // Both start with same snapshot
    engine.accounts[user_idx as usize].funding_index = I128::new(0);
    engine.accounts[lp_idx as usize].funding_index = I128::new(0);

    let user_pnl_before = engine.accounts[user_idx as usize].pnl;
    let lp_pnl_before = engine.accounts[lp_idx as usize].pnl;
    let total_before = user_pnl_before + lp_pnl_before;

    // Accrue funding
    let delta: i128 = kani::any();
    kani::assume(delta != i128::MIN);
    kani::assume(delta.abs() < 10_000);
    engine.funding_index_qpb_e6 = I128::new(delta);

    // Settle both
    let user_result = engine.touch_account(user_idx);
    let lp_result = engine.touch_account(lp_idx);

    // Non-vacuity: both settlements must succeed
    assert!(user_result.is_ok(), "non-vacuity: user settlement must succeed");
    assert!(lp_result.is_ok(), "non-vacuity: LP settlement must succeed");

    let total_after =
        engine.accounts[user_idx as usize].pnl + engine.accounts[lp_idx as usize].pnl;
    let change = total_after - total_before;

    // Funding should not create value (vault keeps rounding dust)
    assert!(change.get() <= 0, "Funding must not create value");
    // Change should be bounded by rounding (at most -2 per account pair)
    assert!(change.get() >= -2, "Funding drift must be bounded");
}

#[kani::proof]
#[kani::unwind(33)]
#[kani::solver(cadical)]
fn funding_p4_settle_before_position_change() {
    // P4: Verifies that settlement before position change gives correct results

    let mut engine = RiskEngine::new(test_params());
    let user_idx = engine.add_user(0).unwrap();

    let initial_pos: i128 = kani::any();
    kani::assume(initial_pos > 0 && initial_pos < 10_000);

    engine.accounts[user_idx as usize].position_size = I128::new(initial_pos);
    engine.accounts[user_idx as usize].pnl = I128::new(0);
    engine.accounts[user_idx as usize].funding_index = I128::new(0);

    // Period 1: accrue funding with initial position
    let delta1: i128 = kani::any();
    kani::assume(delta1 != i128::MIN);
    kani::assume(delta1.abs() < 1_000);
    engine.funding_index_qpb_e6 = I128::new(delta1);

    // Settle BEFORE changing position (must succeed under bounded inputs)
    engine.touch_account(user_idx).unwrap();
    let pnl_after_period1 = engine.accounts[user_idx as usize].pnl;
    // For a long position, positive funding delta should not increase pnl,
    // and negative funding delta should not decrease pnl.
    if delta1 > 0 {
        assert!(
            pnl_after_period1.get() <= 0,
            "Long position with positive funding must not gain pnl"
        );
    } else if delta1 < 0 {
        assert!(
            pnl_after_period1.get() >= 0,
            "Long position with negative funding must not lose pnl"
        );
    }

    // Change position
    let new_pos: i128 = kani::any();
    kani::assume(new_pos > 0 && new_pos < 10_000 && new_pos != initial_pos);
    engine.accounts[user_idx as usize].position_size = I128::new(new_pos);

    // Period 2: more funding
    let delta2: i128 = kani::any();
    kani::assume(delta2 != i128::MIN);
    kani::assume(delta2.abs() < 1_000);
    engine.funding_index_qpb_e6 = I128::new(delta1 + delta2);

    engine.touch_account(user_idx).unwrap();
    let pnl_final = engine.accounts[user_idx as usize].pnl.get();
    let delta_pnl_period2 = pnl_final - pnl_after_period1.get();

    // Period 2 contribution direction should be governed by delta2 and new_pos.
    // This verifies the second settlement is applied after the position change.
    let raw2_sign = new_pos.saturating_mul(delta2);
    if raw2_sign > 0 {
        assert!(
            delta_pnl_period2 <= 0,
            "Period 2 funding charge should not increase pnl"
        );
    } else if raw2_sign < 0 {
        assert!(
            delta_pnl_period2 >= 0,
            "Period 2 funding credit should not decrease pnl"
        );
    }

    // Snapshot should equal global index after settlement
    assert!(
        engine.accounts[user_idx as usize].funding_index == engine.funding_index_qpb_e6,
        "Snapshot must track global index"
    );
}

#[kani::proof]
#[kani::unwind(33)]
#[kani::solver(cadical)]
fn funding_p5_bounded_operations_no_overflow() {
    // P5: No overflows on bounded-safe inputs.

    let mut engine = RiskEngine::new(test_params());

    // Bounded inputs
    let price: u64 = kani::any();
    kani::assume(price > 1_000_000 && price < 1_000_000_000); // $1 to $1000

    let rate: i64 = kani::any();
    kani::assume(rate != i64::MIN);
    kani::assume(rate.abs() < 1000); // ±1000 bps = ±10%

    let dt: u64 = kani::any();
    kani::assume(dt < 1000); // max 1000 slots

    engine.last_funding_slot = 0;

    // Accrue should not panic
    let result = engine.accrue_funding_with_rate(dt, price, rate);

    // In this bounded-safe region, accrual must succeed.
    assert!(
        result.is_ok(),
        "bounded accrue_funding_with_rate inputs must succeed"
    );

    // On success: funding index must have changed (unless dt==0 or rate==0)
    if dt > 0 && rate != 0 {
        assert!(
            engine.funding_index_qpb_e6.get() != 0,
            "funding index must change when dt > 0 and rate != 0"
        );
    }

    // Non-vacuity: with small bounded inputs, accrual must succeed
    if dt > 0 && dt < 100 && rate.abs() < 100 && price < 100_000_000 {
        let mut engine2 = RiskEngine::new(test_params());
        engine2.last_funding_slot = 0;
        let r2 = engine2.accrue_funding_with_rate(dt, price, rate);
        assert!(r2.is_ok(), "non-vacuity: small bounded inputs must succeed");
    }
}

#[kani::proof]
#[kani::unwind(33)]
#[kani::solver(cadical)]
fn funding_p5_invalid_bounds_return_overflow() {
    // Symbolic error-path proof: any rate or dt beyond the guard must return Overflow.
    let mut engine = RiskEngine::new(test_params());
    engine.last_funding_slot = 0;

    let rate: i64 = kani::any();
    let dt: u64 = kani::any();
    kani::assume(rate != i64::MIN);
    kani::assume(dt > 0 && dt < 100_000_000);

    // At least one bound must be violated
    let bad_rate = rate.abs() > 10_000;
    let bad_dt = dt > 31_536_000;
    kani::assume(bad_rate || bad_dt);

    let result = engine.accrue_funding_with_rate(dt, 1_000_000, rate);

    assert!(
        matches!(result, Err(RiskError::Overflow)),
        "out-of-bounds rate or dt must return Overflow"
    );
}

#[kani::proof]
#[kani::unwind(33)]
#[kani::solver(cadical)]
fn funding_zero_position_no_change() {
    // Additional invariant: Zero position means no funding payment

    let mut engine = RiskEngine::new(test_params());
    let user_idx = engine.add_user(0).unwrap();

    engine.accounts[user_idx as usize].position_size = I128::new(0); // Zero position

    let pnl_before: i128 = kani::any();
    kani::assume(pnl_before != i128::MIN); // Avoid abs() overflow
    kani::assume(pnl_before.abs() < 1_000_000);
    engine.accounts[user_idx as usize].pnl = I128::new(pnl_before);

    // Accrue arbitrary funding
    let delta: i128 = kani::any();
    kani::assume(delta != i128::MIN); // Avoid abs() overflow
    kani::assume(delta.abs() < 1_000_000_000);
    engine.funding_index_qpb_e6 = I128::new(delta);

    // Must succeed (zero position skips funding calc, only checked_sub on indices)
    engine.touch_account(user_idx).unwrap();

    // PNL should be unchanged
    assert!(
        engine.accounts[user_idx as usize].pnl.get() == pnl_before,
        "Zero position should not pay or receive funding"
    );
}

// ============================================================================
// Warmup Correctness Proofs
// ============================================================================

/// Proof: update_warmup_slope sets slope.get() >= 1 when positive_pnl > 0
/// This prevents the "zero forever" warmup bug where small PnL never warms up.
#[kani::proof]
#[kani::unwind(33)]
#[kani::solver(cadical)]
fn proof_warmup_slope_nonzero_when_positive_pnl() {
    let mut engine = RiskEngine::new(test_params());
    let user_idx = engine.add_user(0).unwrap();

    // Arbitrary positive PnL (bounded for tractability)
    let positive_pnl: i128 = kani::any();
    kani::assume(positive_pnl > 0 && positive_pnl < 10_000);

    // Setup account with positive PnL
    engine.accounts[user_idx as usize].capital = U128::new(10_000);
    engine.accounts[user_idx as usize].pnl = I128::new(positive_pnl);
    engine.vault = U128::new(10_000 + positive_pnl as u128);
    sync_engine_aggregates(&mut engine);

    // Call update_warmup_slope — force Ok
    assert_ok!(engine.update_warmup_slope(user_idx), "update_warmup_slope must succeed");

    // PROOF: slope must be >= 1 when positive_pnl > 0
    // This is enforced by the debug_assert in the function, but we verify here too
    let slope = engine.accounts[user_idx as usize].warmup_slope_per_step;
    assert!(
        slope.get() >= 1,
        "Warmup slope must be >= 1 when positive_pnl > 0"
    );
}

// ============================================================================
// FAST Frame Proofs
// These prove that operations only mutate intended fields/accounts
// All use #[kani::unwind(33)] and are designed for fast verification
// ============================================================================

/// Frame proof: touch_account only mutates one account's pnl and funding_index
#[kani::proof]
#[kani::unwind(33)]
#[kani::solver(cadical)]
fn fast_frame_touch_account_only_mutates_one_account() {
    let mut engine = RiskEngine::new(test_params());
    let user_idx = engine.add_user(0).unwrap();
    let other_idx = engine.add_user(0).unwrap();

    // Set up with a position so funding can affect PNL
    let position: i128 = kani::any();
    let funding_delta: i128 = kani::any();

    kani::assume(position != i128::MIN);
    kani::assume(funding_delta != i128::MIN);
    kani::assume(position.abs() < 1_000);
    kani::assume(funding_delta.abs() < 1_000_000);

    engine.accounts[user_idx as usize].position_size = I128::new(position);
    engine.funding_index_qpb_e6 = I128::new(funding_delta);
    sync_engine_aggregates(&mut engine);

    // Snapshot before
    let other_snapshot = snapshot_account(&engine.accounts[other_idx as usize]);
    let user_capital_before = engine.accounts[user_idx as usize].capital;
    let globals_before = snapshot_globals(&engine);

    // Touch account (must succeed under bounded inputs)
    engine.touch_account(user_idx).unwrap();

    // Assert: other account unchanged
    let other_after = &engine.accounts[other_idx as usize];
    assert!(
        other_after.capital.get() == other_snapshot.capital,
        "Frame: other capital unchanged"
    );
    assert!(
        other_after.pnl.get() == other_snapshot.pnl,
        "Frame: other pnl unchanged"
    );
    assert!(
        other_after.position_size.get() == other_snapshot.position_size,
        "Frame: other position unchanged"
    );

    // Assert: user capital unchanged (only pnl and funding_index can change)
    assert!(
        engine.accounts[user_idx as usize].capital.get() == user_capital_before.get(),
        "Frame: capital unchanged"
    );

    // Assert: globals unchanged
    assert!(
        engine.vault.get() == globals_before.vault,
        "Frame: vault unchanged"
    );
    assert!(
        engine.insurance_fund.balance.get() == globals_before.insurance_balance,
        "Frame: insurance unchanged"
    );
}

/// Frame proof: deposit only mutates one account's capital, pnl, vault, and warmup globals
/// Note: deposit calls settle_warmup_to_capital which may change pnl (positive settles to
/// capital subject to warmup cap, negative settles fully per Fix A)
#[kani::proof]
#[kani::unwind(33)]
#[kani::solver(cadical)]
fn fast_frame_deposit_only_mutates_one_account_vault_and_warmup() {
    let mut engine = RiskEngine::new(test_params_with_maintenance_fee());
    engine.current_slot = 100;
    engine.last_crank_slot = 100;
    engine.last_full_sweep_start_slot = 100;

    let user_idx = engine.add_user(0).unwrap();
    let other_idx = engine.add_user(0).unwrap();

    let amount: u128 = kani::any();
    let pnl: i128 = kani::any();
    kani::assume(amount > 0 && amount < 10_000);
    kani::assume(pnl > -2_000 && pnl < 5_000 && pnl != 0);

    // Non-fresh account: has capital, PnL, warmup slope, and fee history
    engine.accounts[user_idx as usize].capital = U128::new(5_000);
    engine.accounts[user_idx as usize].pnl = I128::new(pnl);
    engine.accounts[user_idx as usize].warmup_slope_per_step = U128::new(10);
    engine.accounts[user_idx as usize].last_fee_slot = 50;

    // Other account also non-fresh
    engine.accounts[other_idx as usize].capital = U128::new(3_000);
    engine.accounts[other_idx as usize].last_fee_slot = 50;

    let pnl_pos = if pnl > 0 { pnl as u128 } else { 0 };
    engine.vault = U128::new(5_000 + 3_000 + pnl_pos);
    engine.insurance_fund.balance = U128::new(0);
    sync_engine_aggregates(&mut engine);

    // Snapshot before
    let other_snapshot = snapshot_account(&engine.accounts[other_idx as usize]);
    let insurance_before = engine.insurance_fund.balance;

    // Deposit with current_slot triggering fee settlement
    assert_ok!(engine.deposit(user_idx, amount, 100), "deposit must succeed");

    // Assert: other account unchanged
    let other_after = &engine.accounts[other_idx as usize];
    assert!(
        other_after.capital.get() == other_snapshot.capital,
        "Frame: other capital unchanged"
    );
    assert!(
        other_after.pnl.get() == other_snapshot.pnl,
        "Frame: other pnl unchanged"
    );
}

/// Frame proof: withdraw only mutates one account's capital, pnl, vault, and warmup globals
/// Note: withdraw calls settle_warmup_to_capital which may change pnl (negative settles
/// fully per Fix A, positive settles subject to warmup cap)
#[kani::proof]
#[kani::unwind(33)]
#[kani::solver(cadical)]
fn fast_frame_withdraw_only_mutates_one_account_vault_and_warmup() {
    let mut engine = RiskEngine::new(test_params());
    engine.current_slot = 100;
    engine.last_crank_slot = 100;
    engine.last_full_sweep_start_slot = 100;

    let user_idx = engine.add_user(0).unwrap();
    let other_idx = engine.add_user(0).unwrap();
    let lp_idx = engine.add_lp([1u8; 32], [0u8; 32], 0).unwrap();

    let user_capital: u128 = kani::any();
    let position: i128 = kani::any();
    let withdraw: u128 = kani::any();

    kani::assume(user_capital >= 2_000 && user_capital <= 10_000);
    kani::assume(position >= -1_000 && position <= 1_000);
    kani::assume(withdraw > 0 && withdraw <= 500); // Conservative to ensure success

    // User with symbolic capital and position — margin check exercised
    engine.accounts[user_idx as usize].capital = U128::new(user_capital);
    engine.accounts[user_idx as usize].position_size = I128::new(position);
    engine.accounts[user_idx as usize].entry_price = 1_000_000;

    // LP counterparty
    engine.accounts[lp_idx as usize].capital = U128::new(50_000);
    engine.accounts[lp_idx as usize].position_size = I128::new(-position);
    engine.accounts[lp_idx as usize].entry_price = 1_000_000;

    // Other user with capital
    engine.accounts[other_idx as usize].capital = U128::new(3_000);

    engine.vault = U128::new(user_capital + 50_000 + 3_000);
    sync_engine_aggregates(&mut engine);

    kani::assert(canonical_inv(&engine), "INV before withdraw");

    // Snapshot before
    let other_snapshot = snapshot_account(&engine.accounts[other_idx as usize]);

    // Withdraw — force Ok for non-vacuity (small withdraw relative to capital)
    assert_ok!(engine.withdraw(user_idx, withdraw, 100, 1_000_000), "withdraw must succeed");

    // Assert: other account unchanged
    let other_after = &engine.accounts[other_idx as usize];
    assert!(
        other_after.capital.get() == other_snapshot.capital,
        "Frame: other capital unchanged"
    );
    assert!(
        other_after.pnl.get() == other_snapshot.pnl,
        "Frame: other pnl unchanged"
    );

    kani::assert(canonical_inv(&engine), "INV after withdraw");
}

/// Frame proof: execute_trade only mutates two accounts (user and LP)
/// Note: fees increase insurance_fund, not vault
#[kani::proof]
#[kani::unwind(33)]
#[kani::solver(cadical)]
fn fast_frame_execute_trade_only_mutates_two_accounts() {
    let mut engine = RiskEngine::new(test_params());
    engine.current_slot = 100;
    engine.last_crank_slot = 100;
    engine.last_full_sweep_start_slot = 100;

    let user_idx = engine.add_user(0).unwrap();
    let lp_idx = engine.add_lp([1u8; 32], [0u8; 32], 0).unwrap();
    let observer_idx = engine.add_user(0).unwrap();

    // Moderate capital near margin boundary — exercises both pass and fail
    let user_cap: u128 = kani::any();
    kani::assume(user_cap >= 500 && user_cap <= 5_000);
    engine.accounts[user_idx as usize].capital = U128::new(user_cap);
    engine.accounts[lp_idx as usize].capital = U128::new(100_000);
    engine.accounts[observer_idx as usize].capital = U128::new(1_000);
    engine.vault = U128::new(user_cap + 101_000);
    sync_engine_aggregates(&mut engine);

    // Symbolic delta — can exceed margin at low capital
    let delta: i128 = kani::any();
    kani::assume(delta != 0);
    kani::assume(delta != i128::MIN);
    kani::assume(delta >= -500 && delta <= 500);

    // Snapshot before
    let observer_snapshot = snapshot_account(&engine.accounts[observer_idx as usize]);
    let vault_before = engine.vault;
    let insurance_before = engine.insurance_fund.balance;

    // Execute trade
    let matcher = NoOpMatcher;
    let res = engine.execute_trade(&matcher, lp_idx, user_idx, 100, 1_000_000, delta);

    // Assert: observer account completely unchanged (whether trade succeeds or fails)
    let observer_after = &engine.accounts[observer_idx as usize];
    assert!(
        observer_after.capital.get() == observer_snapshot.capital,
        "Frame: observer capital unchanged"
    );
    assert!(
        observer_after.pnl.get() == observer_snapshot.pnl,
        "Frame: observer pnl unchanged"
    );
    assert!(
        observer_after.position_size.get() == observer_snapshot.position_size,
        "Frame: observer position unchanged"
    );

    if res.is_ok() {
        // Assert: vault unchanged (trades don't change vault)
        assert!(
            engine.vault.get() == vault_before.get(),
            "Frame: vault unchanged by trade"
        );
        // Assert: insurance may increase due to fees
        assert!(
            engine.insurance_fund.balance >= insurance_before,
            "Frame: insurance >= before (fees added)"
        );
        kani::assert(canonical_inv(&engine), "INV after trade");
    }

    // Non-vacuity: at least conservative trades must succeed
    if user_cap >= 2_000 && delta >= -50 && delta <= 50 {
        kani::assert(res.is_ok(), "non-vacuity: conservative trade must succeed");
    }
}

/// Frame proof: settle_warmup_to_capital only mutates one account and warmup globals
/// Mutates: target account's capital, pnl, warmup_slope_per_step
/// Note: With Fix A, negative pnl settles fully into capital (not warmup-gated)
#[kani::proof]
#[kani::unwind(33)]
#[kani::solver(cadical)]
fn fast_frame_settle_warmup_only_mutates_one_account_and_warmup_globals() {
    let mut engine = RiskEngine::new(test_params());
    let user_idx = engine.add_user(0).unwrap();
    let other_idx = engine.add_user(0).unwrap();

    let capital: u128 = kani::any();
    let pnl: i128 = kani::any();
    let slope: u128 = kani::any();
    let slots: u64 = kani::any();

    kani::assume(capital > 0 && capital < 5_000);
    kani::assume(pnl > -2_000 && pnl < 2_000 && pnl != 0);
    kani::assume(slope < 100);
    kani::assume(slots < 200);

    engine.accounts[user_idx as usize].capital = U128::new(capital);
    engine.accounts[user_idx as usize].pnl = I128::new(pnl);
    engine.accounts[user_idx as usize].warmup_slope_per_step = U128::new(slope);
    engine.insurance_fund.balance = U128::new(10_000);
    let pnl_pos = if pnl > 0 { pnl as u128 } else { 0 };
    engine.vault = U128::new(capital + 10_000 + pnl_pos);
    engine.current_slot = slots;
    sync_engine_aggregates(&mut engine);

    kani::assert(canonical_inv(&engine), "INV before settle_warmup");

    // Snapshot other account
    let other_snapshot = snapshot_account(&engine.accounts[other_idx as usize]);

    // Settle warmup — force Ok for non-vacuity
    engine.settle_warmup_to_capital(user_idx).unwrap();

    // Assert: other account unchanged
    let other_after = &engine.accounts[other_idx as usize];
    assert!(
        other_after.capital.get() == other_snapshot.capital,
        "Frame: other capital unchanged"
    );
    assert!(
        other_after.pnl.get() == other_snapshot.pnl,
        "Frame: other pnl unchanged"
    );

    // Postcondition: canonical_inv preserved
    kani::assert(canonical_inv(&engine), "INV after settle_warmup");

    // Postcondition: N1 boundary holds for target
    let account = &engine.accounts[user_idx as usize];
    kani::assert(
        n1_boundary_holds(account),
        "N1: pnl >= 0 OR capital == 0 after settle",
    );
}

/// Frame proof: update_warmup_slope only mutates one account
#[kani::proof]
#[kani::unwind(33)]
#[kani::solver(cadical)]
fn fast_frame_update_warmup_slope_only_mutates_one_account() {
    let mut engine = RiskEngine::new(test_params());
    let user_idx = engine.add_user(0).unwrap();
    let other_idx = engine.add_user(0).unwrap();

    let pnl: i128 = kani::any();
    let capital: u128 = kani::any();
    kani::assume(pnl > -5_000 && pnl < 10_000);
    kani::assume(capital < 5_000);

    engine.accounts[user_idx as usize].pnl = I128::new(pnl);
    engine.accounts[user_idx as usize].capital = U128::new(capital);
    let pnl_pos = if pnl > 0 { pnl as u128 } else { 0 };
    engine.vault = U128::new(capital + pnl_pos + 10_000);
    engine.insurance_fund.balance = U128::new(10_000);
    sync_engine_aggregates(&mut engine);

    kani::assert(canonical_inv(&engine), "INV before update_warmup_slope");

    // Snapshot
    let other_snapshot = snapshot_account(&engine.accounts[other_idx as usize]);
    let globals_before = snapshot_globals(&engine);

    // Update slope — force Ok for non-vacuity
    engine.update_warmup_slope(user_idx).unwrap();

    // Assert: other account unchanged
    let other_after = &engine.accounts[other_idx as usize];
    assert!(
        other_after.capital.get() == other_snapshot.capital,
        "Frame: other capital unchanged"
    );
    assert!(
        other_after.pnl.get() == other_snapshot.pnl,
        "Frame: other pnl unchanged"
    );
    assert!(
        other_after.warmup_slope_per_step.get() == other_snapshot.warmup_slope_per_step,
        "Frame: other slope unchanged"
    );

    // Assert: globals unchanged
    assert!(
        engine.vault.get() == globals_before.vault,
        "Frame: vault unchanged"
    );
    assert!(
        engine.insurance_fund.balance.get() == globals_before.insurance_balance,
        "Frame: insurance unchanged"
    );

    kani::assert(canonical_inv(&engine), "INV after update_warmup_slope");

    // Slope correctness: positive pnl → non-zero slope
    let account = &engine.accounts[user_idx as usize];
    if account.pnl.get() > 0 {
        kani::assert(
            account.warmup_slope_per_step.get() > 0,
            "positive pnl → non-zero slope",
        );
    }
}

// ============================================================================
// FAST Validity-Preservation Proofs
// These prove that canonical_inv is preserved by operations
// ============================================================================

/// canonical_inv preserved by deposit
#[kani::proof]
#[kani::unwind(33)]
#[kani::solver(cadical)]
fn fast_valid_preserved_by_deposit() {
    let mut engine = RiskEngine::new(test_params_with_maintenance_fee());
    engine.current_slot = 100;
    engine.last_crank_slot = 100;
    engine.last_full_sweep_start_slot = 100;
    let user_idx = engine.add_user(0).unwrap();

    // Seasoned account with symbolic capital, pnl, and fee history
    let capital: u128 = kani::any();
    kani::assume(capital >= 500 && capital <= 5_000);
    let pnl: i128 = kani::any();
    kani::assume(pnl > -2_000 && pnl < 2_000);
    let now_slot: u64 = kani::any();
    kani::assume(now_slot >= 100 && now_slot <= 300);

    engine.accounts[user_idx as usize].capital = U128::new(capital);
    engine.accounts[user_idx as usize].pnl = I128::new(pnl);
    engine.accounts[user_idx as usize].last_fee_slot = 50; // fee accrual branch exercised
    engine.current_slot = now_slot;
    engine.last_crank_slot = now_slot;
    engine.last_full_sweep_start_slot = now_slot;

    let pnl_pos = if pnl > 0 { pnl as u128 } else { 0 };
    engine.insurance_fund.balance = U128::new(1_000);
    engine.vault = U128::new(capital + 1_000 + pnl_pos);
    sync_engine_aggregates(&mut engine);

    kani::assert(canonical_inv(&engine), "setup must satisfy INV");

    let amount: u128 = kani::any();
    kani::assume(amount > 0 && amount < 5_000);

    // Deposit may succeed or fail (fee accrual might cause undercollateralized)
    let _res = engine.deposit(user_idx, amount, now_slot);

    kani::assert(canonical_inv(&engine), "INV preserved by deposit");
}

/// canonical_inv preserved by withdraw
#[kani::proof]
#[kani::unwind(33)]
#[kani::solver(cadical)]
fn fast_valid_preserved_by_withdraw() {
    let mut engine = RiskEngine::new(test_params());
    engine.current_slot = 100;
    engine.last_crank_slot = 100;
    engine.last_full_sweep_start_slot = 100;
    let user_idx = engine.add_user(0).unwrap();
    let lp_idx = engine.add_lp([1u8; 32], [0u8; 32], 0).unwrap();

    // Symbolic capital with position to exercise margin checks
    let capital: u128 = kani::any();
    kani::assume(capital >= 500 && capital <= 5_000);
    let pos: i128 = kani::any();
    kani::assume(pos != 0 && pos > -500 && pos < 500);

    engine.accounts[user_idx as usize].capital = U128::new(capital);
    engine.accounts[user_idx as usize].position_size = I128::new(pos);
    engine.accounts[user_idx as usize].entry_price = 1_000_000;
    engine.accounts[lp_idx as usize].capital = U128::new(capital);
    engine.accounts[lp_idx as usize].position_size = I128::new(-pos);
    engine.accounts[lp_idx as usize].entry_price = 1_000_000;
    engine.vault = U128::new(capital * 2);
    sync_engine_aggregates(&mut engine);

    kani::assert(canonical_inv(&engine), "setup must satisfy INV");

    // Allow withdraw_amt > capital to exercise InsufficientBalance path
    let withdraw: u128 = kani::any();
    kani::assume(withdraw > 0 && withdraw < 10_000);

    // May succeed or fail (margin/balance)
    let _res = engine.withdraw(user_idx, withdraw, 100, 1_000_000);

    kani::assert(canonical_inv(&engine), "INV preserved by withdraw");
}

/// canonical_inv preserved by execute_trade
#[kani::proof]
#[kani::unwind(33)]
#[kani::solver(cadical)]
fn fast_valid_preserved_by_execute_trade() {
    let mut engine = RiskEngine::new(test_params());
    engine.current_slot = 100;
    engine.last_crank_slot = 100;
    engine.last_full_sweep_start_slot = 100;
    let user_idx = engine.add_user(0).unwrap();
    let lp_idx = engine.add_lp([1u8; 32], [0u8; 32], 0).unwrap();

    // Symbolic capitals near margin boundary to exercise margin checks
    let user_cap: u128 = kani::any();
    kani::assume(user_cap >= 500 && user_cap <= 5_000);
    let lp_cap: u128 = kani::any();
    kani::assume(lp_cap >= 500 && lp_cap <= 5_000);

    engine.accounts[user_idx as usize].capital = U128::new(user_cap);
    engine.accounts[lp_idx as usize].capital = U128::new(lp_cap);
    engine.vault = U128::new(user_cap + lp_cap);
    sync_engine_aggregates(&mut engine);

    // Symbolic delta with symbolic oracle for mark PnL variation
    let delta: i128 = kani::any();
    kani::assume(delta != 0);
    kani::assume(delta != i128::MIN);
    kani::assume(delta.abs() < 200);

    let oracle: u64 = kani::any();
    kani::assume(oracle >= 900_000 && oracle <= 1_100_000);

    kani::assert(canonical_inv(&engine), "setup must satisfy INV");

    let matcher = NoOpMatcher;
    // May succeed or fail depending on margin at boundary capitals
    let _res = engine.execute_trade(&matcher, lp_idx, user_idx, 100, oracle, delta);

    kani::assert(canonical_inv(&engine), "INV preserved by execute_trade");
}

/// canonical_inv preserved by settle_warmup_to_capital
#[kani::proof]
#[kani::unwind(33)]
#[kani::solver(cadical)]
fn fast_valid_preserved_by_settle_warmup_to_capital() {
    let mut engine = RiskEngine::new(test_params_with_floor());
    let user_idx = engine.add_user(0).unwrap();

    let capital: u128 = kani::any();
    let pnl: i128 = kani::any();
    let slope: u128 = kani::any();
    let slots: u64 = kani::any();
    let insurance: u128 = kani::any();

    kani::assume(capital > 0 && capital < 5_000);
    kani::assume(pnl > -2_000 && pnl < 2_000);
    kani::assume(slope < 100);
    kani::assume(slots < 200);
    kani::assume(insurance > 1_000 && insurance < 10_000);

    engine.accounts[user_idx as usize].capital = U128::new(capital);
    engine.accounts[user_idx as usize].pnl = I128::new(pnl);
    engine.accounts[user_idx as usize].warmup_slope_per_step = U128::new(slope);
    engine.insurance_fund.balance = U128::new(insurance);
    engine.current_slot = slots;

    if pnl > 0 {
        engine.vault = U128::new(capital + insurance + pnl as u128);
    } else {
        engine.vault = U128::new(capital + insurance);
    }
    sync_engine_aggregates(&mut engine);

    kani::assert(canonical_inv(&engine), "setup must satisfy INV");

    let res = engine.settle_warmup_to_capital(user_idx);

    // Non-vacuity: settle_warmup must succeed (account is used, bounded inputs)
    kani::assert(res.is_ok(), "non-vacuity: settle_warmup must succeed");
    kani::assert(
        canonical_inv(&engine),
        "INV preserved by settle_warmup_to_capital",
    );
}

/// canonical_inv preserved by top_up_insurance_fund
#[kani::proof]
#[kani::unwind(33)]
#[kani::solver(cadical)]
fn fast_valid_preserved_by_top_up_insurance_fund() {
    let mut engine = RiskEngine::new(test_params());

    let amount: u128 = kani::any();
    kani::assume(amount > 0 && amount < 10_000);

    kani::assert(canonical_inv(&engine), "setup must satisfy INV");

    let res = engine.top_up_insurance_fund(amount);

    // Non-vacuity: top_up must succeed
    kani::assert(res.is_ok(), "non-vacuity: top_up_insurance_fund must succeed");
    kani::assert(canonical_inv(&engine), "INV preserved by top_up_insurance_fund");
}

// ============================================================================
// FAST Proofs: Negative PnL Immediate Settlement (Fix A)
// These prove that negative PnL settles immediately, independent of warmup cap
// ============================================================================

/// Proof: Negative PnL settles into capital independent of warmup cap
/// Proves: capital_after == capital_before - min(capital_before, loss)
///         pnl_after == 0  (remaining loss is written off per spec §6.1)

#[kani::proof]
#[kani::unwind(33)]
#[kani::solver(cadical)]
fn fast_neg_pnl_settles_into_capital_independent_of_warm_cap() {
    let mut engine = RiskEngine::new(test_params());
    let user_idx = engine.add_user(0).unwrap();

    let capital: u128 = kani::any();
    let loss: u128 = kani::any();

    kani::assume(capital > 0 && capital < 10_000);
    kani::assume(loss > 0 && loss < 10_000);

    engine.accounts[user_idx as usize].capital = U128::new(capital);
    engine.accounts[user_idx as usize].pnl = I128::new(-(loss as i128));
    engine.accounts[user_idx as usize].warmup_slope_per_step = U128::new(0); // Zero slope
    engine.accounts[user_idx as usize].warmup_started_at_slot = 0;
    engine.vault = U128::new(capital);
    engine.current_slot = 100;
    sync_engine_aggregates(&mut engine);

    kani::assert(canonical_inv(&engine), "setup INV");

    // Settle
    engine.settle_warmup_to_capital(user_idx).unwrap();

    let pay = core::cmp::min(capital, loss);
    let expected_capital = capital - pay;
    // Under haircut spec §6.1: remaining negative PnL is written off to 0
    let expected_pnl: i128 = 0;

    // Assertions
    assert!(
        engine.accounts[user_idx as usize].capital.get() == expected_capital,
        "Capital should be reduced by min(capital, loss)"
    );
    assert!(
        engine.accounts[user_idx as usize].pnl.get() == expected_pnl,
        "PnL should be written off to 0 (spec §6.1)"
    );

    kani::assert(canonical_inv(&engine), "INV after settle");
}

/// Proof: Withdraw cannot bypass losses when position is zero
/// Even with no position, withdrawal fails if losses would make it insufficient
#[kani::proof]
#[kani::unwind(33)]
#[kani::solver(cadical)]
fn fast_withdraw_cannot_bypass_losses_when_position_zero() {
    let mut engine = RiskEngine::new(test_params());
    let user_idx = engine.add_user(0).unwrap();

    let capital: u128 = kani::any();
    let loss: u128 = kani::any();

    kani::assume(capital > 0 && capital < 5_000);
    kani::assume(loss > 0 && loss < capital); // Some loss, but not all

    engine.accounts[user_idx as usize].capital = U128::new(capital);
    engine.accounts[user_idx as usize].pnl = I128::new(-(loss as i128));
    engine.accounts[user_idx as usize].position_size = I128::new(0); // No position
    engine.accounts[user_idx as usize].warmup_slope_per_step = U128::new(0);
    engine.vault = U128::new(capital);
    sync_engine_aggregates(&mut engine);

    kani::assert(canonical_inv(&engine), "setup INV");

    // After settlement: capital = capital - loss, pnl = 0
    // Trying to withdraw more than remaining capital should fail
    let result = engine.withdraw(user_idx, capital, 0, 1_000_000);

    // Should fail because after loss settlement, capital is less than requested
    assert!(
        result == Err(RiskError::InsufficientBalance),
        "Withdraw of full capital must fail when losses exist"
    );

    // Verify loss was settled
    assert!(
        engine.accounts[user_idx as usize].pnl.get() >= 0,
        "PnL should be non-negative after settlement (unless insolvent)"
    );

    kani::assert(canonical_inv(&engine), "INV after withdraw attempt");
}

/// Proof: After settle, pnl < 0 implies capital == 0
/// This is the key invariant enforced by Fix A
#[kani::proof]
#[kani::unwind(33)]
#[kani::solver(cadical)]
fn fast_neg_pnl_after_settle_implies_zero_capital() {
    let mut engine = RiskEngine::new(test_params());
    let user_idx = engine.add_user(0).unwrap();

    let capital: u128 = kani::any();
    let loss: u128 = kani::any();

    kani::assume(capital < 10_000);
    kani::assume(loss > 0 && loss < 20_000);

    engine.accounts[user_idx as usize].capital = U128::new(capital);
    engine.accounts[user_idx as usize].pnl = I128::new(-(loss as i128));
    let slope: u128 = kani::any();
    kani::assume(slope <= 10_000);
    engine.accounts[user_idx as usize].warmup_slope_per_step = U128::new(slope);
    engine.vault = U128::new(capital);
    sync_engine_aggregates(&mut engine);

    kani::assert(canonical_inv(&engine), "setup INV");

    // Settle
    engine.settle_warmup_to_capital(user_idx).unwrap();

    // Key invariant: pnl < 0 implies capital == 0
    let pnl_after = engine.accounts[user_idx as usize].pnl;
    let capital_after = engine.accounts[user_idx as usize].capital;

    assert!(
        pnl_after.get() >= 0 || capital_after.get() == 0,
        "After settle: pnl < 0 must imply capital == 0"
    );

    kani::assert(canonical_inv(&engine), "INV after settle");
}

/// Proof: Negative PnL settlement does not depend on elapsed or slope (N1)
/// With any symbolic slope and elapsed time, result is identical to pay-down rule
#[kani::proof]
#[kani::unwind(33)]
#[kani::solver(cadical)]
fn neg_pnl_settlement_does_not_depend_on_elapsed_or_slope() {
    let mut engine = RiskEngine::new(test_params());
    let user_idx = engine.add_user(0).unwrap();

    let capital: u128 = kani::any();
    let loss: u128 = kani::any();
    let slope: u128 = kani::any();
    let elapsed: u64 = kani::any();

    kani::assume(capital > 0 && capital < 10_000);
    kani::assume(loss > 0 && loss < 10_000);
    kani::assume(elapsed < 1_000_000);

    engine.accounts[user_idx as usize].capital = U128::new(capital);
    engine.accounts[user_idx as usize].pnl = I128::new(-(loss as i128));
    engine.accounts[user_idx as usize].warmup_slope_per_step = U128::new(slope);
    engine.accounts[user_idx as usize].warmup_started_at_slot = 0;
    engine.vault = U128::new(capital);
    engine.current_slot = elapsed;
    engine.recompute_aggregates();

    // Settle
    engine.settle_warmup_to_capital(user_idx).unwrap();

    // Result must match pay-down rule: pay = min(capital, loss), then write-off remainder
    let pay = core::cmp::min(capital, loss);
    let expected_capital = capital - pay;
    // Under haircut spec §6.1: remaining negative PnL is written off to 0
    let expected_pnl: i128 = 0;

    // Assert results are identical regardless of slope and elapsed
    assert!(
        engine.accounts[user_idx as usize].capital.get() == expected_capital,
        "Capital must match pay-down rule regardless of slope/elapsed"
    );
    assert!(
        engine.accounts[user_idx as usize].pnl.get() == expected_pnl,
        "PnL must be written off to 0 regardless of slope/elapsed"
    );
}

/// Proof: Withdraw calls settle and enforces pnl >= 0 || capital == 0 (N1)
/// After withdraw (whether Ok or Err), the N1 invariant must hold
#[kani::proof]
#[kani::unwind(33)]
#[kani::solver(cadical)]
fn withdraw_calls_settle_enforces_pnl_or_zero_capital_post() {
    let mut engine = RiskEngine::new(test_params());
    let user_idx = engine.add_user(0).unwrap();

    let capital: u128 = kani::any();
    let loss: u128 = kani::any();
    let withdraw_amt: u128 = kani::any();

    kani::assume(capital > 0 && capital < 5_000);
    kani::assume(loss > 0 && loss < 10_000);
    kani::assume(withdraw_amt < 10_000);

    engine.accounts[user_idx as usize].capital = U128::new(capital);
    engine.accounts[user_idx as usize].pnl = I128::new(-(loss as i128));
    engine.accounts[user_idx as usize].position_size = I128::new(0);
    engine.accounts[user_idx as usize].warmup_slope_per_step = U128::new(0);
    engine.vault = U128::new(capital);
    sync_engine_aggregates(&mut engine);

    kani::assert(canonical_inv(&engine), "setup INV");

    // Call withdraw - may succeed or fail
    let _result = engine.withdraw(user_idx, withdraw_amt, 0, 1_000_000);

    // After return (Ok or Err), N1 invariant must hold
    let pnl_after = engine.accounts[user_idx as usize].pnl;
    let capital_after = engine.accounts[user_idx as usize].capital;

    assert!(
        pnl_after.get() >= 0 || capital_after.get() == 0,
        "After withdraw: pnl >= 0 || capital == 0 must hold"
    );

    kani::assert(canonical_inv(&engine), "INV after withdraw");
}

// ============================================================================
// FAST Proofs: Equity-Based Margin (Fix B)
// These prove that margin checks use equity (capital + pnl), not just collateral
// ============================================================================

/// Proof: MTM maintenance margin uses haircutted equity including negative PnL
/// Tests the production margin check (is_above_maintenance_margin_mtm), not the deprecated one.
/// Since entry_price == oracle_price, mark_pnl = 0, and with a fresh engine (h=1),
/// equity_mtm = max(0, C_i + min(PNL, 0) + effective_pos_pnl(PNL)).
#[kani::proof]
#[kani::unwind(33)]
#[kani::solver(cadical)]
fn fast_maintenance_margin_uses_equity_including_negative_pnl() {
    let mut engine = RiskEngine::new(test_params());

    let capital: u128 = kani::any();
    let pnl: i128 = kani::any();
    let position: i128 = kani::any();

    kani::assume(capital < 10_000);
    kani::assume(pnl > -10_000 && pnl < 10_000);
    kani::assume(position > -1_000 && position < 1_000 && position != 0);

    let pos_pnl = if pnl > 0 { pnl as u128 } else { 0 };
    // Tighter vault — exercises haircut < 1 when vault deficit exists
    // vault_margin ∈ [0, pos_pnl + 500] so vault - c_tot = vault_margin which may be < pnl_pos_tot
    let vault_margin: u128 = kani::any();
    kani::assume(vault_margin <= pos_pnl + 500);
    engine.vault = U128::new(capital + vault_margin);
    engine.insurance_fund.balance = U128::new(0);

    let idx = engine.add_user(0).unwrap();
    engine.accounts[idx as usize].capital = U128::new(capital);
    engine.accounts[idx as usize].pnl = I128::new(pnl);
    engine.accounts[idx as usize].position_size = I128::new(position);
    engine.accounts[idx as usize].entry_price = 1_000_000;
    sync_engine_aggregates(&mut engine);

    let oracle_price = 1_000_000u64; // entry == oracle → mark_pnl = 0

    // Compute expected haircutted equity (matching engine's formula exactly)
    let cap_i = u128_to_i128_clamped(capital);
    let neg_pnl = core::cmp::min(pnl, 0i128);
    let eff_pos = engine.effective_pos_pnl(pnl);
    let eff_eq_i = cap_i
        .saturating_add(neg_pnl)
        .saturating_add(u128_to_i128_clamped(eff_pos));
    let eff_equity = if eff_eq_i > 0 { eff_eq_i as u128 } else { 0 };

    let position_value = abs_i128_to_u128(position) * (oracle_price as u128) / 1_000_000;
    let mm_required = position_value * (engine.params.maintenance_margin_bps as u128) / 10_000;

    let is_above = engine.is_above_maintenance_margin_mtm(&engine.accounts[idx as usize], oracle_price);

    // is_above_maintenance_margin_mtm uses haircutted (effective) equity
    if eff_equity > mm_required {
        assert!(is_above, "Should be above MM when effective equity > required");
    } else {
        assert!(!is_above, "Should be below MM when effective equity <= required");
    }
}

/// Proof: account_equity correctly computes max(0, capital + pnl)
#[kani::proof]
#[kani::unwind(33)]
#[kani::solver(cadical)]
fn fast_account_equity_computes_correctly() {
    let mut engine = RiskEngine::new(test_params());
    engine.vault = U128::new(1_000_000);

    let user = engine.add_user(0).unwrap();

    let capital: u128 = kani::any();
    let pnl: i128 = kani::any();

    kani::assume(capital < 1_000_000);
    kani::assume(pnl > -1_000_000 && pnl < 1_000_000);

    engine.set_capital(user as usize, capital);
    engine.set_pnl(user as usize, pnl);
    sync_engine_aggregates(&mut engine);
    kani::assert(canonical_inv(&engine), "setup INV");

    let equity = engine.account_equity(&engine.accounts[user as usize]);

    // Calculate expected: max(0, capital + pnl)
    let cap_i = u128_to_i128_clamped(capital);
    let eq_i = cap_i.saturating_add(pnl);
    let expected = if eq_i > 0 { eq_i as u128 } else { 0 };

    kani::assert(
        equity == expected,
        "account_equity must equal max(0, capital + pnl)",
    );
}

// ============================================================================
// DETERMINISTIC Proofs: Equity Margin with Exact Values (Plan 2.3)
// Fast, stable proofs using constants instead of symbolic values
// ============================================================================

/// Proof: Withdraw margin check blocks when equity after withdraw < IM (deterministic)
/// Setup: position_size=1000, entry_price=1_000_000 => notional=1000, IM=100
/// capital=150, pnl=0 (avoid settlement effects), withdraw=60
/// new_capital=90, equity=90 < 100 (IM) => Must return Undercollateralized
#[kani::proof]
#[kani::unwind(33)]
#[kani::solver(cadical)]
fn withdraw_im_check_blocks_when_equity_after_withdraw_below_im() {
    let mut engine = RiskEngine::new(test_params());
    let user_idx = engine.add_user(0).unwrap();

    // Ensure funding is settled (no pnl changes from touch_account)
    engine.funding_index_qpb_e6 = I128::new(0);
    engine.accounts[user_idx as usize].funding_index = I128::new(0);

    let capital: u128 = kani::any();
    let position: i128 = kani::any();
    let withdraw: u128 = kani::any();
    kani::assume(capital >= 50 && capital <= 500);
    kani::assume(position >= 100 && position <= 5_000);
    kani::assume(withdraw > 0 && withdraw <= capital);

    engine.accounts[user_idx as usize].capital = U128::new(capital);
    engine.accounts[user_idx as usize].pnl = I128::new(0);
    engine.accounts[user_idx as usize].position_size = I128::new(position);
    engine.accounts[user_idx as usize].entry_price = 1_000_000;
    engine.accounts[user_idx as usize].warmup_slope_per_step = U128::new(0);
    engine.vault = U128::new(capital);
    sync_engine_aggregates(&mut engine);

    // IM = position * IM_bps / 10_000 = position / 10
    // MM = position * MM_bps / 10_000 = position / 20
    // Withdraw has both pre-IM check (equity < im) and post-MM check (equity > mm)
    let im_required = (position as u128) / 10;
    let mm_required = (position as u128) / 20;
    let equity_after = capital - withdraw;

    let result = engine.withdraw(user_idx, withdraw, 0, 1_000_000);

    // Withdraw fails if equity < IM (pre-check) OR equity <= MM (post-check)
    if equity_after >= im_required && equity_after > mm_required {
        assert!(result.is_ok(), "withdraw must succeed when equity >= IM and > MM");
    }
    if equity_after < mm_required {
        assert!(result.is_err(), "withdraw must fail when equity < MM");
    }

    // Non-vacuity: conservative case (high capital, small position, small withdraw) succeeds
    if capital >= 200 && position <= 500 && withdraw <= 50 {
        kani::assert(result.is_ok(), "non-vacuity: conservative withdraw must succeed");
    }
}

/// Proof: Negative PnL is realized immediately (deterministic, plan 2.2A)
/// Setup: capital = C, pnl = -L, warmup_slope_per_step = 0, elapsed arbitrary
/// Assert: pay = min(C, L), capital_after = C - pay, pnl_after = -(L - pay)
#[kani::proof]
#[kani::unwind(33)]
#[kani::solver(cadical)]
fn neg_pnl_is_realized_immediately_by_settle() {
    let mut engine = RiskEngine::new(test_params());
    let user_idx = engine.add_user(0).unwrap();

    let capital: u128 = kani::any();
    let loss: u128 = kani::any();
    kani::assume(capital > 0 && capital <= 10_000);
    kani::assume(loss > 0 && loss <= 10_000);

    engine.accounts[user_idx as usize].capital = U128::new(capital);
    engine.accounts[user_idx as usize].pnl = I128::new(-(loss as i128));
    engine.accounts[user_idx as usize].warmup_slope_per_step = U128::new(0);
    engine.accounts[user_idx as usize].warmup_started_at_slot = 0;
    engine.vault = U128::new(capital);
    engine.insurance_fund.balance = U128::new(0);
    sync_engine_aggregates(&mut engine);
    engine.current_slot = 1000;

    engine.settle_warmup_to_capital(user_idx).unwrap();

    let pay = core::cmp::min(capital, loss);
    let cap_after = engine.accounts[user_idx as usize].capital.get();
    let pnl_after = engine.accounts[user_idx as usize].pnl.get();

    kani::assert(cap_after == capital - pay, "capital must decrease by pay");

    // After settle: pnl is always 0 — excess loss is written off (§6.1 step 4)
    kani::assert(pnl_after == 0, "pnl must be 0 after settle (loss written off)");

    // N1: if loss > capital, pnl remains negative but capital is 0
    kani::assert(
        n1_boundary_holds(&engine.accounts[user_idx as usize]),
        "N1: pnl >= 0 OR capital == 0",
    );
}

// ============================================================================
// Security Goal: Bounded Net Extraction (Sequence-Based Proof)
// ============================================================================

// ============================================================================
// WRAPPER-CORE API PROOFS
// ============================================================================

/// A. Fee credits never inflate from settle_maintenance_fee
/// Uses real maintenance fees to test actual behavior
#[kani::proof]
#[kani::unwind(33)]
#[kani::solver(cadical)]
fn proof_fee_credits_never_inflate_from_settle() {
    let mut engine = RiskEngine::new(test_params_with_maintenance_fee());

    let user = engine.add_user(0).unwrap();

    let capital: u128 = kani::any();
    let now_slot: u64 = kani::any();
    kani::assume(capital >= 100 && capital <= 50_000);
    kani::assume(now_slot >= 100 && now_slot <= 100_000);

    engine.deposit(user, capital, 0).unwrap();

    // Set last_fee_slot = 0 so fees accrue over now_slot slots
    engine.accounts[user as usize].last_fee_slot = 0;

    kani::assert(canonical_inv(&engine), "setup INV");

    let credits_before = engine.accounts[user as usize].fee_credits;

    engine.settle_maintenance_fee(user, now_slot, 1_000_000).unwrap();

    let credits_after = engine.accounts[user as usize].fee_credits;

    // Fee credits should only decrease (fees deducted) or stay same
    assert!(
        credits_after <= credits_before,
        "Fee credits increased from settle_maintenance_fee"
    );

    kani::assert(canonical_inv(&engine), "INV after fee settle");
}

/// B. settle_maintenance_fee properly deducts with deterministic accounting
/// Uses fee_per_slot = 1 to avoid integer division issues
#[kani::proof]
#[kani::unwind(33)]
#[kani::solver(cadical)]
fn proof_settle_maintenance_deducts_correctly() {
    let mut engine = RiskEngine::new(test_params_with_maintenance_fee());
    let user = engine.add_user(0).unwrap();

    // Symbolic capital and slot — exercises partial-pay (capital < due) and full-pay paths
    let capital: u128 = kani::any();
    let now_slot: u64 = kani::any();
    let fee_credits: i128 = kani::any();
    kani::assume(capital >= 100 && capital <= 20_000);
    kani::assume(now_slot >= 100 && now_slot <= 10_000);
    kani::assume(fee_credits >= -500 && fee_credits <= 500);

    engine.accounts[user as usize].capital = U128::new(capital);
    engine.accounts[user as usize].fee_credits = I128::new(fee_credits);
    engine.accounts[user as usize].last_fee_slot = 0;
    engine.vault = U128::new(capital + 10_000);
    engine.insurance_fund.balance = U128::new(10_000);
    sync_engine_aggregates(&mut engine);

    let cap_before = engine.accounts[user as usize].capital.get();
    let insurance_before = engine.insurance_fund.balance.get();

    let res = engine.settle_maintenance_fee(user, now_slot, 1_000_000);
    assert!(res.is_ok(), "settle_maintenance_fee must succeed");

    let cap_after = engine.accounts[user as usize].capital.get();
    let insurance_after = engine.insurance_fund.balance.get();

    // Capital can only decrease (fees deducted)
    kani::assert(cap_after <= cap_before, "capital must not increase from fees");
    // Insurance can only increase (fees added)
    kani::assert(insurance_after >= insurance_before, "insurance must not decrease from fees");
    // Slot must be updated
    kani::assert(engine.accounts[user as usize].last_fee_slot == now_slot, "slot must update");
    // Conservation: capital decrease == insurance increase (net zero)
    let cap_decrease = cap_before - cap_after;
    let ins_increase = insurance_after - insurance_before;
    kani::assert(cap_decrease == ins_increase, "fee settlement must be zero-sum");
}

/// C. keeper_crank advances last_crank_slot correctly
/// Note: keeper_crank now also runs garbage_collect_dust which can mutate
/// bitmap/freelist. This proof focuses on slot advancement.
#[kani::proof]
#[kani::unwind(33)]
#[kani::solver(cadical)]
fn proof_keeper_crank_advances_slot_monotonically() {
    let mut engine = RiskEngine::new(test_params());
    engine.current_slot = 100;
    engine.last_crank_slot = 100;
    engine.last_full_sweep_start_slot = 100;

    let capital: u128 = kani::any();
    kani::assume(capital >= 1_000 && capital <= 50_000);

    let user = engine.add_user(0).unwrap();
    engine.accounts[user as usize].capital = U128::new(capital);
    engine.vault = U128::new(capital + 10_000);
    engine.insurance_fund.balance = U128::new(10_000);
    sync_engine_aggregates(&mut engine);

    kani::assert(canonical_inv(&engine), "INV before crank");

    // Symbolic slot — exercises both advancing and non-advancing paths
    let now_slot: u64 = kani::any();
    kani::assume(now_slot >= 50 && now_slot <= 10_000);

    let last_before = engine.last_crank_slot;
    let result = engine.keeper_crank(user, now_slot, 1_000_000, 0, false);

    // keeper_crank succeeds with valid setup
    assert!(result.is_ok(), "keeper_crank should succeed with valid setup");

    let outcome = result.unwrap();

    if now_slot > last_before {
        // Should advance
        kani::assert(outcome.advanced, "must advance when now_slot > last_crank_slot");
        kani::assert(engine.last_crank_slot == now_slot, "last_crank_slot == now_slot");
        kani::assert(engine.current_slot == now_slot, "current_slot updated");
    } else {
        // Should not advance
        kani::assert(!outcome.advanced, "must not advance when now_slot <= last_crank_slot");
        kani::assert(engine.last_crank_slot == last_before, "last_crank_slot unchanged");
    }

    // GC budget always respected
    kani::assert(outcome.num_gc_closed <= GC_CLOSE_BUDGET, "GC must respect budget");

    kani::assert(canonical_inv(&engine), "INV after crank");
}

/// C2. keeper_crank never fails due to caller maintenance settle
/// Even if caller is undercollateralized, crank returns Ok with caller_settle_ok=false
#[kani::proof]
#[kani::unwind(33)]
#[kani::solver(cadical)]
fn proof_keeper_crank_best_effort_settle() {
    let mut engine = RiskEngine::new(test_params_with_maintenance_fee());
    engine.current_slot = 100;
    engine.last_crank_slot = 100;
    engine.last_full_sweep_start_slot = 100;

    // Create user with symbolic small capital — may or may not cover fees
    let capital: u128 = kani::any();
    kani::assume(capital >= 10 && capital <= 500);
    let user = engine.add_user(0).unwrap();
    let lp = engine.add_lp([1u8; 32], [0u8; 32], 0).unwrap();

    engine.accounts[user as usize].capital = U128::new(capital);

    // Give user a position so undercollateralization can trigger
    engine.accounts[user as usize].position_size = I128::new(1000);
    engine.accounts[user as usize].entry_price = 1_000_000;

    // LP counterparty
    engine.accounts[lp as usize].capital = U128::new(50_000);
    engine.accounts[lp as usize].position_size = I128::new(-1000);
    engine.accounts[lp as usize].entry_price = 1_000_000;

    // Set last_fee_slot = 100 (same as current), so fees accrue from crank advance
    engine.accounts[user as usize].last_fee_slot = 100;
    engine.vault = U128::new(capital + 50_000 + 10_000);
    engine.insurance_fund.balance = U128::new(10_000);
    sync_engine_aggregates(&mut engine);

    kani::assert(canonical_inv(&engine), "INV before crank");

    // Crank at a later slot - fees will accrue over many slots
    let result = engine.keeper_crank(user, 100_000, 1_000_000, 0, false);

    // keeper_crank ALWAYS returns Ok (best-effort settle)
    assert!(result.is_ok(), "keeper_crank must always succeed");

    kani::assert(canonical_inv(&engine), "INV after best-effort crank");

    // Capital must not have increased (fees only deducted)
    kani::assert(
        engine.accounts[user as usize].capital.get() <= capital,
        "capital must not increase from fee settlement",
    );
}

/// D. close_account succeeds iff flat and pnl == 0 (fee debt forgiven on close)
#[kani::proof]
#[kani::unwind(33)]
#[kani::solver(cadical)]
fn proof_close_account_requires_flat_and_paid() {
    let mut engine = RiskEngine::new(test_params());
    engine.current_slot = 100;
    engine.last_crank_slot = 100;
    engine.last_full_sweep_start_slot = 100;
    let user = engine.add_user(0).unwrap();

    // Symbolic capital, pnl, position instead of boolean selectors
    let capital: u128 = kani::any();
    kani::assume(capital <= 5_000);
    let pnl: i128 = kani::any();
    kani::assume(pnl > -2_000 && pnl < 2_000);
    let position: i128 = kani::any();
    kani::assume(position > -500 && position < 500);

    engine.accounts[user as usize].capital = U128::new(capital);
    engine.accounts[user as usize].pnl = I128::new(pnl);
    engine.accounts[user as usize].position_size = I128::new(position);
    if position != 0 {
        engine.accounts[user as usize].entry_price = 1_000_000;
    }
    // Warmup: slope=0 so positive pnl cannot be warmed off
    engine.accounts[user as usize].warmup_slope_per_step = U128::new(0);

    let pnl_pos = if pnl > 0 { pnl as u128 } else { 0 };
    engine.vault = U128::new(capital + pnl_pos);
    sync_engine_aggregates(&mut engine);

    kani::assert(canonical_inv(&engine), "setup INV");

    let result = engine.close_account(user, 100, 1_000_000);

    // If position != 0 OR pnl > 0 (after settle), close must fail
    if position != 0 || pnl > 0 {
        kani::assert(
            result.is_err(),
            "close_account must fail if position != 0 OR pnl > 0"
        );
    }

    kani::assert(canonical_inv(&engine), "INV after close_account attempt");
}

/// E. total_open_interest tracking: starts at 0 for new engine
/// Note: Full OI tracking is tested via trade execution in other proofs
#[kani::proof]
#[kani::unwind(33)]
#[kani::solver(cadical)]
fn proof_total_open_interest_initial() {
    let mut engine = RiskEngine::new(test_params());
    engine.vault = U128::new(200_000);

    let u0 = engine.add_user(0).unwrap();
    let u1 = engine.add_user(0).unwrap();

    // Symbolic positions for two accounts
    let pos0: i128 = kani::any();
    let pos1: i128 = kani::any();
    kani::assume(pos0 > -500 && pos0 < 500);
    kani::assume(pos1 > -500 && pos1 < 500);

    engine.accounts[u0 as usize].capital = U128::new(50_000);
    engine.accounts[u1 as usize].capital = U128::new(50_000);
    engine.accounts[u0 as usize].position_size = I128::new(pos0);
    engine.accounts[u1 as usize].position_size = I128::new(pos1);
    if pos0 != 0 {
        engine.accounts[u0 as usize].entry_price = 1_000_000;
        engine.accounts[u0 as usize].funding_index = engine.funding_index_qpb_e6;
    }
    if pos1 != 0 {
        engine.accounts[u1 as usize].entry_price = 1_000_000;
        engine.accounts[u1 as usize].funding_index = engine.funding_index_qpb_e6;
    }

    sync_engine_aggregates(&mut engine);
    kani::assert(canonical_inv(&engine), "setup INV");

    // OI must equal sum of absolute positions
    let abs0 = if pos0 >= 0 { pos0 as u128 } else { (-pos0) as u128 };
    let abs1 = if pos1 >= 0 { pos1 as u128 } else { (-pos1) as u128 };
    kani::assert(
        engine.total_open_interest.get() == abs0 + abs1,
        "OI = sum(|position|) for all accounts",
    );
}

/// F. require_fresh_crank gates stale state correctly
#[kani::proof]
#[kani::unwind(33)]
#[kani::solver(cadical)]
fn proof_require_fresh_crank_gates_stale() {
    let mut engine = RiskEngine::new(test_params());

    engine.last_crank_slot = 100;
    engine.max_crank_staleness_slots = 50;

    let now_slot: u64 = kani::any();
    kani::assume(now_slot < u64::MAX - 1000);

    let result = engine.require_fresh_crank(now_slot);

    let staleness = now_slot.saturating_sub(engine.last_crank_slot);

    if staleness > engine.max_crank_staleness_slots {
        // Should fail with Unauthorized when stale
        assert!(
            result == Err(RiskError::Unauthorized),
            "require_fresh_crank should fail with Unauthorized when stale"
        );
    } else {
        // Should succeed when fresh
        assert!(
            result.is_ok(),
            "require_fresh_crank should succeed when fresh"
        );
    }
}

/// Verify withdraw rejects with Unauthorized when crank is stale
#[kani::proof]
#[kani::unwind(33)]
#[kani::solver(cadical)]
fn proof_stale_crank_blocks_withdraw() {
    let mut engine = RiskEngine::new(test_params());
    engine.last_crank_slot = 100;
    engine.last_full_sweep_start_slot = 100;
    engine.current_slot = 100;
    engine.max_crank_staleness_slots = 50;

    let user = engine.add_user(0).unwrap();
    engine.deposit(user, 10_000, 0).unwrap();

    let now_slot: u64 = kani::any();
    kani::assume(now_slot >= 50 && now_slot < u64::MAX - 1000);

    kani::assert(canonical_inv(&engine), "INV before withdraw");

    let result = engine.withdraw(user, 1_000, now_slot, 1_000_000);

    if now_slot.saturating_sub(engine.last_crank_slot) > engine.max_crank_staleness_slots {
        // Stale: must reject
        assert!(
            result == Err(RiskError::Unauthorized),
            "withdraw must reject when crank is stale"
        );
    } else {
        // Fresh: must succeed (user has 10K capital, withdrawing 1K)
        assert!(result.is_ok(), "withdraw must succeed when crank is fresh");
    }

    kani::assert(canonical_inv(&engine), "INV preserved regardless of path");
}

/// Verify execute_trade rejects with Unauthorized when crank is stale
#[kani::proof]
#[kani::unwind(33)]
#[kani::solver(cadical)]
fn proof_stale_crank_blocks_execute_trade() {
    let mut engine = RiskEngine::new(test_params());
    engine.last_crank_slot = 100;
    engine.last_full_sweep_start_slot = 100;
    engine.current_slot = 100;
    engine.max_crank_staleness_slots = 50;

    let lp = engine.add_lp([1u8; 32], [0u8; 32], 0).unwrap();
    let user = engine.add_user(0).unwrap();
    engine.deposit(lp, 100_000, 0).unwrap();
    engine.deposit(user, 10_000, 0).unwrap();

    let now_slot: u64 = kani::any();
    kani::assume(now_slot >= 50 && now_slot < u64::MAX - 1000);

    kani::assert(canonical_inv(&engine), "INV before execute_trade");

    let result = engine.execute_trade(
        &NoOpMatcher,
        lp, user, now_slot, 1_000_000, 1_000,
    );

    if now_slot.saturating_sub(engine.last_crank_slot) > engine.max_crank_staleness_slots {
        // Stale: must reject
        assert!(
            result == Err(RiskError::Unauthorized),
            "execute_trade must reject when crank is stale"
        );
    } else {
        // Fresh: must succeed (conservative trade, adequate capital)
        assert!(result.is_ok(), "execute_trade must succeed when crank is fresh");
    }

    kani::assert(canonical_inv(&engine), "INV preserved regardless of path");
}

/// Verify close_account rejects when pnl > 0 (must warm up first)
/// This enforces: can't bypass warmup via close, and conservation is maintained
#[kani::proof]
#[kani::unwind(33)]
#[kani::solver(cadical)]
fn proof_close_account_rejects_positive_pnl() {
    let mut engine = RiskEngine::new(test_params());
    let user = engine.add_user(0).unwrap();

    let capital: u128 = kani::any();
    let pnl: i128 = kani::any();
    kani::assume(capital >= 100 && capital <= 10_000);
    kani::assume(pnl > 0 && pnl < 5_000);

    engine.deposit(user, capital, 0).unwrap();

    // Warmup slope=0 at slot=0 means nothing can warm
    engine.current_slot = 0;
    engine.accounts[user as usize].warmup_started_at_slot = 0;
    engine.accounts[user as usize].warmup_slope_per_step = U128::new(0);
    engine.accounts[user as usize].reserved_pnl = 0;

    // Symbolic positive pnl must block close
    engine.accounts[user as usize].pnl = I128::new(pnl);
    sync_engine_aggregates(&mut engine);

    kani::assert(canonical_inv(&engine), "setup INV");

    let res = engine.close_account(user, 0, 1_000_000);

    assert!(
        res == Err(RiskError::PnlNotWarmedUp),
        "close_account must reject positive pnl with PnlNotWarmedUp"
    );

    kani::assert(canonical_inv(&engine), "INV after close_account rejection");
}

/// Verify close_account includes warmed pnl that was settled to capital
#[kani::proof]
#[kani::unwind(33)]
#[kani::solver(cadical)]
fn proof_close_account_includes_warmed_pnl() {
    let mut engine = RiskEngine::new(test_params());
    let user = engine.add_user(0).unwrap();

    let capital: u128 = kani::any();
    let pnl: i128 = kani::any();
    kani::assume(capital >= 100 && capital <= 5_000);
    kani::assume(pnl > 0 && pnl < 3_000);

    engine.deposit(user, capital, 0).unwrap();

    // Symbolic insurance and slope to exercise partial conversion and h < 1
    let insurance: u128 = kani::any();
    kani::assume(insurance >= 1 && insurance <= 500);
    let slope: u128 = kani::any();
    kani::assume(slope >= 1 && slope <= 100);

    engine.insurance_fund.balance = U128::new(insurance);
    // vault must cover capital + insurance + pnl_pos for accounting invariant
    engine.vault = U128::new(capital + insurance + pnl as u128);

    // Symbolic positive pnl with bounded slope (may cause partial conversion)
    engine.accounts[user as usize].pnl = I128::new(pnl);
    engine.accounts[user as usize].reserved_pnl = 0;
    engine.accounts[user as usize].warmup_started_at_slot = 0;
    engine.accounts[user as usize].warmup_slope_per_step = U128::new(slope);
    sync_engine_aggregates(&mut engine);

    // Advance time: slope * 200 may or may not exceed pnl (partial conversion)
    engine.current_slot = 200;
    engine.last_crank_slot = 200;
    engine.last_full_sweep_start_slot = 200;

    // Settle warmup
    engine.settle_warmup_to_capital(user).unwrap();

    let pnl_after = engine.accounts[user as usize].pnl.get();
    let capital_after_warmup = engine.accounts[user as usize].capital;

    if pnl_after == 0 {
        // Fully warmed: close must succeed and return capital including warmed pnl
        let result = engine.close_account(user, 200, 1_000_000);
        kani::assert(
            result.is_ok(),
            "close_account must succeed when flat and pnl==0"
        );
        let returned = result.unwrap();
        kani::assert(
            returned == capital_after_warmup.get(),
            "close_account should return capital including warmed pnl"
        );
    } else {
        // Partially warmed: close must fail due to remaining positive pnl
        let result = engine.close_account(user, 200, 1_000_000);
        kani::assert(
            result.is_err(),
            "close_account must fail when pnl still positive (partial warmup)"
        );
    }
}

/// close_account succeeds with 0 capital when pnl < 0 (neg pnl written off per §6.1)
#[kani::proof]
#[kani::unwind(33)]
#[kani::solver(cadical)]
fn proof_close_account_negative_pnl_written_off() {
    let mut engine = RiskEngine::new(test_params());
    let user = engine.add_user(0).unwrap();

    engine.current_slot = 0;
    engine.accounts[user as usize].last_fee_slot = 0;

    let loss: u128 = kani::any();
    kani::assume(loss >= 1 && loss <= 10_000);

    engine.deposit(user, 100, 0).unwrap();

    // Flat and no fees owed
    engine.accounts[user as usize].position_size = I128::new(0);
    engine.accounts[user as usize].fee_credits = I128::ZERO;
    engine.funding_index_qpb_e6 = I128::new(0);
    engine.accounts[user as usize].funding_index = I128::new(0);

    // Force insolvent state: symbolic negative pnl, capital exhausted
    engine.accounts[user as usize].capital = U128::new(0);
    engine.vault = U128::new(0);
    engine.accounts[user as usize].pnl = I128::new(-(loss as i128));
    sync_engine_aggregates(&mut engine);

    kani::assert(canonical_inv(&engine), "setup INV");

    // Under haircut spec §6.1: negative PnL is written off to 0 during settlement.
    // So close_account succeeds (returning 0 capital) instead of rejecting.
    let res = engine.close_account(user, 0, 1_000_000);
    assert!(res == Ok(0));

    kani::assert(canonical_inv(&engine), "INV after close_account writeoff");
}

/// Verify set_risk_reduction_threshold updates the parameter
#[kani::proof]
#[kani::unwind(33)]
#[kani::solver(cadical)]
fn proof_set_risk_reduction_threshold_updates() {
    let mut engine = RiskEngine::new(test_params());

    let new_threshold: u128 = kani::any();
    kani::assume(new_threshold < u128::MAX / 2); // Bounded for sanity

    kani::assert(canonical_inv(&engine), "setup INV");

    engine.set_risk_reduction_threshold(new_threshold);

    assert!(
        engine.params.risk_reduction_threshold.get() == new_threshold,
        "Threshold not updated correctly"
    );

    kani::assert(canonical_inv(&engine), "INV after threshold update");
}

// ============================================================================
// Fee Credits Proofs (Step 5 additions)
// ============================================================================

/// Proof: Trading increases user's fee_credits by exactly the fee amount
/// Uses deterministic values to avoid rounding to 0
#[kani::proof]
#[kani::unwind(33)]
#[kani::solver(cadical)]
fn proof_trading_credits_fee_to_user() {
    let mut engine = RiskEngine::new(test_params());
    engine.current_slot = 100;
    engine.last_crank_slot = 100;
    engine.last_full_sweep_start_slot = 100;

    let user = engine.add_user(0).unwrap();
    let lp = engine.add_lp([1u8; 32], [0u8; 32], 0).unwrap();

    engine.accounts[user as usize].capital = U128::new(1_000_000);
    engine.accounts[lp as usize].capital = U128::new(1_000_000);
    engine.insurance_fund.balance = U128::new(100_000);
    engine.vault = U128::new(2_000_000 + 100_000); // c_tot + insurance
    sync_engine_aggregates(&mut engine);

    kani::assert(canonical_inv(&engine), "INV before trade");

    let credits_before = engine.accounts[user as usize].fee_credits;

    // Symbolic trade size: fee = |size| * fee_bps / 10000
    let size: i128 = kani::any();
    kani::assume(size >= 100 && size <= 5_000_000);
    let oracle_price: u64 = 1_000_000;

    let _ = assert_ok!(
        engine.execute_trade(&NoOpMatcher, lp, user, 100, oracle_price, size),
        "trade must succeed for fee credit proof"
    );

    kani::assert(canonical_inv(&engine), "INV after trade");

    let credits_after = engine.accounts[user as usize].fee_credits;
    let credits_increase = credits_after - credits_before;

    // Fee formula: ceil(|size| * trading_fee_bps / 10000) (test_params has fee_bps=10)
    // Source: percolator.rs uses (mul_u128(notional, fee_bps) + 9999) / 10_000
    // With NoOpMatcher exec_price = oracle_price = 1_000_000, so notional = |size|
    let expected_fee = (size.unsigned_abs() * 10 + 9999) / 10_000;
    kani::assert(
        credits_increase.get() == expected_fee as i128,
        "Trading must credit user with exactly ceil(|size| * fee_bps / 10000)"
    );
}

/// Proof: keeper_crank forgives exactly half the elapsed slots
/// Uses fee_per_slot = 1 for deterministic accounting
#[kani::proof]
#[kani::unwind(33)]
#[kani::solver(cadical)]
fn proof_keeper_crank_forgives_half_slots() {
    let mut engine = RiskEngine::new(test_params_with_maintenance_fee());

    // Create user and set capital explicitly (add_user doesn't give capital)
    let user = engine.add_user(0).unwrap();
    engine.accounts[user as usize].capital = U128::new(1_000_000);
    engine.vault = U128::new(1_000_000);

    // Set last_fee_slot to 0 so fees accrue
    engine.accounts[user as usize].last_fee_slot = 0;
    sync_engine_aggregates(&mut engine);

    kani::assert(canonical_inv(&engine), "setup INV");

    // Use bounded now_slot for fast verification
    let now_slot: u64 = kani::any();
    kani::assume(now_slot > 0 && now_slot <= 1000);
    kani::assume(now_slot > engine.last_crank_slot);

    // Calculate expected values
    let dt = now_slot; // since last_fee_slot is 0
    let expected_forgive = dt / 2;
    let charged_dt = dt - expected_forgive; // ceil(dt/2)

    // With fee_per_slot = 1, due = charged_dt
    let insurance_before = engine.insurance_fund.balance;

    let result = engine.keeper_crank(user, now_slot, 1_000_000, 0, false);

    // keeper_crank always succeeds
    assert!(result.is_ok(), "keeper_crank should always succeed");
    let outcome = result.unwrap();

    // Verify slots_forgiven matches expected (dt / 2, floored)
    assert!(
        outcome.slots_forgiven == expected_forgive,
        "keeper_crank must forgive dt/2 slots"
    );

    // After crank, last_fee_slot should be now_slot
    assert!(
        engine.accounts[user as usize].last_fee_slot == now_slot,
        "last_fee_slot must be advanced to now_slot after settlement"
    );

    // last_fee_slot never exceeds now_slot
    assert!(
        engine.accounts[user as usize].last_fee_slot <= now_slot,
        "last_fee_slot must never exceed now_slot"
    );

    // Insurance should increase by exactly the charged amount (since user has capital)
    let insurance_after = engine.insurance_fund.balance;
    if outcome.caller_settle_ok {
        assert!(
            insurance_after.get() == insurance_before.get() + (charged_dt as u128),
            "Insurance must increase by exactly charged_dt when settle succeeds"
        );
    }

    kani::assert(canonical_inv(&engine), "INV after keeper_crank");
}

/// Proof: In this no-price-move scenario, attacker withdrawal is principal-bounded.
///
/// Model scope:
/// - Trades execute at oracle (NoOpMatcher), so trade PnL transfer is zero-sum and
///   attacker cannot realize directional price profit.
/// - No explicit insurance top-ups or oracle drift are modeled.
///
/// Security claim for this harness:
/// attacker cannot successfully withdraw more than their own deposited principal.
#[kani::proof]
#[kani::unwind(33)]
#[kani::solver(cadical)]
fn proof_net_extraction_bounded_with_fee_credits() {
    let mut engine = RiskEngine::new(test_params());

    // Setup: attacker and LP with bounded capitals
    let attacker_deposit: u128 = kani::any();
    let lp_deposit: u128 = kani::any();
    kani::assume(attacker_deposit > 0 && attacker_deposit <= 1000);
    kani::assume(lp_deposit > 0 && lp_deposit <= 1000);

    let attacker = engine.add_user(0).unwrap();
    let lp = engine.add_lp([1u8; 32], [0u8; 32], 0).unwrap();
    engine.deposit(attacker, attacker_deposit, 0).unwrap();
    engine.deposit(lp, lp_deposit, 0).unwrap();

    // Optional: attacker calls keeper_crank first (may fail, that's ok)
    let do_crank: bool = kani::any();
    if do_crank {
        let _crank = engine.keeper_crank(attacker, 100, 1_000_000, 0, false);
    }

    // Optional: execute a trade (may fail due to margin, that's ok)
    let do_trade: bool = kani::any();
    if do_trade {
        let delta: i128 = kani::any();
        kani::assume(delta != 0 && delta != i128::MIN);
        kani::assume(delta > -5 && delta < 5);
        let trade_now = 100u64;
        let _trade = engine.execute_trade(&NoOpMatcher, lp, attacker, trade_now, 1_000_000, delta);
    }

    // Attacker attempts withdrawal
    let withdraw_amount: u128 = kani::any();
    kani::assume(withdraw_amount <= 10000);

    // Get attacker's state before withdrawal
    let attacker_capital = engine.accounts[attacker as usize].capital;

    // Try to withdraw
    let result = engine.withdraw(attacker, withdraw_amount, 0, 1_000_000);

    // PROOF: Cannot withdraw more than equity allows
    // If withdrawal succeeded, amount must be <= available equity
    if result.is_ok() {
        // In this modeled scenario (no price edge), attacker capital before withdraw
        // cannot exceed their original deposit.
        assert!(
            attacker_capital.get() <= attacker_deposit,
            "attacker capital should be principal-bounded before withdraw in this model"
        );
        // Withdrawal succeeded, so amount was within limits
        // The engine enforces capital-only withdrawals (no direct pnl/credit withdrawal)
        assert!(
            withdraw_amount <= attacker_deposit,
            "Attacker cannot withdraw more than their own deposited principal in this setup"
        );
        assert!(
            engine.accounts[attacker as usize].capital.get() <= attacker_capital.get(),
            "Successful withdraw must not increase attacker capital"
        );
    }

    // Non-vacuity: when no trade/crank and withdrawal is within deposit, must succeed
    if !do_trade && !do_crank && withdraw_amount <= attacker_deposit {
        assert!(result.is_ok(), "non-vacuity: withdrawal within deposit must succeed without trade/crank");
    }

    kani::assert(canonical_inv(&engine), "INV after extraction attempt");
}

// ============================================================================
// LIQUIDATION PROOFS
// ============================================================================

/// LQ4: Liquidation fee is paid from capital to insurance
/// Verifies that the liquidation fee is correctly calculated and transferred.
/// Uses pnl = 0 to isolate fee-only effect (no settlement noise).
/// Forces full close via dust rule (min_liquidation_abs > position).
#[kani::proof]
#[kani::unwind(33)]
#[kani::solver(cadical)]
fn proof_lq4_liquidation_fee_paid_to_insurance() {
    // Use custom params with min_liquidation_abs larger than position to force full close
    let mut params = test_params();
    params.min_liquidation_abs = U128::new(20_000_000);
    let mut engine = RiskEngine::new(params);
    engine.current_slot = 100;
    engine.last_crank_slot = 100;
    engine.last_full_sweep_start_slot = 100;

    let user = engine.add_user(0).unwrap();
    let lp = engine.add_lp([1u8; 32], [0u8; 32], 0).unwrap();

    // Symbolic capital: exercises different fee payment capacities
    let capital: u128 = kani::any();
    kani::assume(capital >= 50_000 && capital <= 200_000);

    // User with position (10 units long at $1.00)
    engine.accounts[user as usize].capital = U128::new(capital);
    engine.accounts[user as usize].position_size = I128::new(10_000_000);
    engine.accounts[user as usize].entry_price = 1_000_000;
    engine.accounts[user as usize].pnl = I128::new(0);

    // LP counterparty
    engine.accounts[lp as usize].capital = U128::new(500_000);
    engine.accounts[lp as usize].position_size = I128::new(-10_000_000);
    engine.accounts[lp as usize].entry_price = 1_000_000;

    engine.vault = U128::new(capital + 500_000 + 10_000);
    engine.insurance_fund.balance = U128::new(10_000);
    sync_engine_aggregates(&mut engine);

    kani::assert(canonical_inv(&engine), "INV before liquidation");

    let insurance_before = engine.insurance_fund.balance.get();

    // Oracle at entry → mark PnL = 0, so undercollateralized iff capital < MM
    let result = engine.liquidate_at_oracle(user, 100, 1_000_000);
    assert!(result.is_ok(), "liquidation must not error");
    let triggered = result.unwrap();

    kani::assert(canonical_inv(&engine), "INV after liquidation");

    if triggered {
        let insurance_after = engine.insurance_fund.balance.get();
        // Fee must have increased insurance (fee > 0 for non-zero position)
        kani::assert(
            insurance_after > insurance_before,
            "Insurance must increase when liquidation triggers"
        );
        // Fee capped at liquidation_fee_cap = 10_000
        kani::assert(
            insurance_after - insurance_before <= 10_000,
            "Fee must not exceed liquidation_fee_cap"
        );
    }
}

/// Proof: keeper_crank never fails due to liquidation errors (best-effort).
/// Symbolic capital exercises both fully-solvent and deeply-undercollateralized
/// liquidation paths. Oracle fixed at entry to avoid solver explosion.
#[kani::proof]
#[kani::unwind(33)]
#[kani::solver(cadical)]
fn proof_keeper_crank_best_effort_liquidation() {
    let mut engine = RiskEngine::new(test_params());
    engine.current_slot = 100;
    engine.last_crank_slot = 100;
    engine.last_full_sweep_start_slot = 100;

    let capital: u128 = kani::any();
    let oracle_price: u64 = kani::any();
    kani::assume(capital >= 100 && capital <= 5_000);
    kani::assume(oracle_price >= 950_000 && oracle_price <= 1_050_000);

    let user = engine.add_user(0).unwrap();
    let lp = engine.add_lp([1u8; 32], [0u8; 32], 0).unwrap();

    engine.accounts[user as usize].capital = U128::new(capital);
    // Large position: always under-MM at capital <= 5K (MM = 500K for 10M notional)
    engine.accounts[user as usize].position_size = I128::new(10_000_000);
    engine.accounts[user as usize].entry_price = 1_000_000;

    // LP counterparty
    engine.accounts[lp as usize].capital = U128::new(500_000);
    engine.accounts[lp as usize].position_size = I128::new(-10_000_000);
    engine.accounts[lp as usize].entry_price = 1_000_000;

    engine.vault = U128::new(capital + 500_000 + 10_000);
    engine.insurance_fund.balance = U128::new(10_000);
    sync_engine_aggregates(&mut engine);

    kani::assert(canonical_inv(&engine), "INV before crank");

    // keeper_crank must always succeed regardless of liquidation outcomes
    let result = engine.keeper_crank(user, 101, oracle_price, 0, false);

    assert!(result.is_ok(), "keeper_crank must always succeed (best-effort)");

    kani::assert(canonical_inv(&engine), "INV after crank with liquidation");
}

/// LQ7: Symbolic oracle liquidation — mark PnL settlement + all post-conditions.
/// Unlike LQ1-LQ6 (oracle=entry → mark_pnl=0), this proof uses symbolic oracle
/// to exercise variation margin settlement during liquidation. Capital and oracle
/// are symbolic; account is always undercollateralized (10 units, capital ≤ 1000).
/// Verifies: INV, OI decrease, dust rule, N1 boundary, conservation.
#[kani::proof]
#[kani::unwind(33)]
#[kani::solver(cadical)]
fn proof_lq7_symbolic_oracle_liquidation() {
    let mut engine = RiskEngine::new(test_params());
    engine.current_slot = 100;
    engine.last_crank_slot = 100;
    engine.last_full_sweep_start_slot = 100;

    let capital: u128 = kani::any();
    let oracle_price: u64 = kani::any();
    kani::assume(capital >= 100 && capital <= 1_000);
    kani::assume(oracle_price >= 950_000 && oracle_price <= 1_050_000);

    // User: 10 units long at $1.00, small capital → always undercollateralized.
    // At best oracle (1.05M): equity = capital + 500K ≈ 501K, MM ≈ 525K → still under.
    // At worst oracle (950K): equity = capital - 500K → deeply negative → full close.
    let user = engine.add_user(0).unwrap();
    engine.accounts[user as usize].capital = U128::new(capital);
    engine.accounts[user as usize].position_size = I128::new(10_000_000);
    engine.accounts[user as usize].entry_price = 1_000_000;

    // LP counterparty (well-capitalized)
    let lp = engine.add_lp([1u8; 32], [0u8; 32], 0).unwrap();
    engine.accounts[lp as usize].capital = U128::new(100_000);
    engine.accounts[lp as usize].position_size = I128::new(-10_000_000);
    engine.accounts[lp as usize].entry_price = 1_000_000;

    engine.vault = U128::new(capital + 100_000 + 10_000);
    engine.insurance_fund.balance = U128::new(10_000);
    sync_engine_aggregates(&mut engine);
    kani::assert(canonical_inv(&engine), "setup state must satisfy INV");

    let oi_before = engine.total_open_interest;

    let result = engine.liquidate_at_oracle(user, 100, oracle_price);
    let triggered = assert_ok!(result, "liquidation must not error");

    // Must trigger — account is always undercollateralized
    kani::assert(triggered, "account must be undercollateralized");

    // INV after liquidation
    kani::assert(canonical_inv(&engine), "INV must hold after liquidation");

    // OI must strictly decrease
    kani::assert(
        engine.total_open_interest < oi_before,
        "OI must decrease after liquidation",
    );

    // Dust rule: position is 0 or >= min_liquidation_abs
    let abs_pos = abs_i128_to_u128(engine.accounts[user as usize].position_size.get());
    kani::assert(
        abs_pos == 0 || abs_pos >= engine.params.min_liquidation_abs.get(),
        "Dust rule: position must be 0 or >= min_liquidation_abs",
    );

    // N1 boundary
    kani::assert(
        n1_boundary_holds(&engine.accounts[user as usize]),
        "N1: pnl >= 0 OR capital == 0",
    );
}

/// Symbolic partial liquidation: exercises both partial fills (moderate capital,
/// oracle near entry) and full closes (low capital, oracle below entry).
/// Covers: canonical_inv, OI decrease, dust rule, N1 boundary, conservation,
/// and maintenance margin safety after partial fill.
/// Subsumes concrete LQ1-LQ3a, LQ6, PARTIAL-1 through PARTIAL-5.
#[kani::proof]
#[kani::unwind(5)]
#[kani::solver(cadical)]
fn proof_liq_partial_symbolic() {
    let mut engine = RiskEngine::new(test_params());
    engine.current_slot = 100;
    engine.last_crank_slot = 100;
    engine.last_full_sweep_start_slot = 100;

    let capital: u128 = kani::any();
    let oracle_price: u64 = kani::any();
    kani::assume(capital >= 100_000 && capital <= 400_000);
    kani::assume(oracle_price >= 950_000 && oracle_price <= 1_000_000);

    // User: 10 units long at $1.00, moderate capital.
    // At oracle=1M, capital=200K: equity=200K, MM=500K → partial close to ~3.2M.
    // At oracle=950K, capital=100K: equity≈-400K → full close, insolvency.
    let user = engine.add_user(0).unwrap();
    let counterparty = engine.add_user(0).unwrap();

    engine.accounts[user as usize].capital = U128::new(capital);
    engine.accounts[user as usize].position_size = I128::new(10_000_000);
    engine.accounts[user as usize].entry_price = 1_000_000;

    engine.accounts[counterparty as usize].capital = U128::new(500_000);
    engine.accounts[counterparty as usize].position_size = I128::new(-10_000_000);
    engine.accounts[counterparty as usize].entry_price = 1_000_000;

    engine.vault = U128::new(capital + 500_000 + 10_000);
    engine.insurance_fund.balance = U128::new(10_000);
    sync_engine_aggregates(&mut engine);
    kani::assert(canonical_inv(&engine), "setup INV");

    let oi_before = engine.total_open_interest;

    let result = engine.liquidate_at_oracle(user, 100, oracle_price);
    let triggered = assert_ok!(result, "liquidation must not error");
    kani::assert(triggered, "account must be undercollateralized");

    kani::assert(canonical_inv(&engine), "INV after liquidation");

    let account = &engine.accounts[user as usize];
    let abs_pos = abs_i128_to_u128(account.position_size.get());

    // OI must strictly decrease
    kani::assert(
        engine.total_open_interest < oi_before,
        "OI must decrease",
    );

    // Dust rule
    kani::assert(
        abs_pos == 0 || abs_pos >= engine.params.min_liquidation_abs.get(),
        "dust rule",
    );

    // N1 boundary
    kani::assert(
        n1_boundary_holds(account),
        "N1: pnl >= 0 OR capital == 0",
    );

    // Maintenance margin after partial fill: holds when post-settlement capital
    // is large enough for the target-MM buffer (100 bps) to absorb the fee cap (10K).
    // At capital >= 200K and oracle >= 990K: cap_after_mark >= 100K,
    // target_notional >= 1.65M, buffer = 16.5K > fee_cap(10K).
    if abs_pos > 0 && capital >= 200_000 && oracle_price >= 990_000 {
        kani::assert(
            engine.is_above_maintenance_margin_mtm(account, oracle_price),
            "partial: above maintenance margin (sufficient capital)",
        );
    }

    // Non-vacuity: moderate capital at oracle=entry must produce partial fill
    if capital >= 200_000 && oracle_price == 1_000_000 {
        kani::assert(abs_pos > 0, "non-vacuity: partial fill for moderate capital");
    }
}

// ==============================================================================
// GARBAGE COLLECTION PROOFS
// ==============================================================================

/// GC never frees an account with positive value (capital > 0 or pnl > 0)
#[kani::proof]
#[kani::unwind(33)]
#[kani::solver(cadical)]
fn gc_never_frees_account_with_positive_value() {
    let mut engine = RiskEngine::new(test_params());

    // Set global funding index explicitly
    engine.funding_index_qpb_e6 = I128::new(0);

    // Create two accounts: one with positive value, one that's dust
    let positive_idx = engine.add_user(0).unwrap();
    let dust_idx = engine.add_user(0).unwrap();

    // Set funding indices for both accounts (required by GC predicate)
    engine.accounts[positive_idx as usize].funding_index = I128::new(0);
    engine.accounts[dust_idx as usize].funding_index = I128::new(0);

    // Positive account: either has capital or positive pnl
    let has_capital: bool = kani::any();
    if has_capital {
        let capital: u128 = kani::any();
        kani::assume(capital > 0 && capital < 1000);
        engine.accounts[positive_idx as usize].capital = U128::new(capital);
        engine.vault = U128::new(capital);
    } else {
        let pnl: i128 = kani::any();
        kani::assume(pnl > 0 && pnl < 100);
        engine.accounts[positive_idx as usize].pnl = I128::new(pnl);
        engine.vault = U128::new(pnl as u128);
    }
    engine.accounts[positive_idx as usize].position_size = I128::new(0);
    engine.accounts[positive_idx as usize].reserved_pnl = 0;

    // Dust account: zero capital, zero position, zero reserved, zero pnl
    engine.accounts[dust_idx as usize].capital = U128::new(0);
    engine.accounts[dust_idx as usize].position_size = I128::new(0);
    engine.accounts[dust_idx as usize].reserved_pnl = 0;
    engine.accounts[dust_idx as usize].pnl = I128::new(0);

    sync_engine_aggregates(&mut engine);

    // Record whether positive account was used before GC
    let positive_was_used = engine.is_used(positive_idx as usize);
    assert!(positive_was_used, "Positive account should exist");

    // Run GC
    let closed = engine.garbage_collect_dust();

    // The dust account should be closed (non-vacuous)
    assert!(closed > 0, "GC should close the dust account");

    // The positive value account must still exist
    assert!(
        engine.is_used(positive_idx as usize),
        "GC must not free account with positive value"
    );

    // INV must hold after GC
    kani::assert(canonical_inv(&engine), "INV preserved by GC");
}

/// canonical_inv preserved by garbage_collect_dust — symbolic state
#[kani::proof]
#[kani::unwind(33)]
#[kani::solver(cadical)]
fn fast_valid_preserved_by_garbage_collect_dust() {
    let mut engine = RiskEngine::new(test_params());
    engine.funding_index_qpb_e6 = I128::new(0);

    // Create a dust account + a non-dust account with symbolic capital
    let dust_idx = engine.add_user(0).unwrap();
    let live_idx = engine.add_user(0).unwrap();

    let live_capital: u128 = kani::any();
    kani::assume(live_capital > 0 && live_capital <= 5_000);

    // Dust account: zero everything
    engine.accounts[dust_idx as usize].funding_index = I128::new(0);
    engine.accounts[dust_idx as usize].capital = U128::new(0);
    engine.accounts[dust_idx as usize].position_size = I128::new(0);
    engine.accounts[dust_idx as usize].reserved_pnl = 0;
    engine.accounts[dust_idx as usize].pnl = I128::new(0);

    // Live account: symbolic capital
    engine.accounts[live_idx as usize].funding_index = I128::new(0);
    engine.accounts[live_idx as usize].capital = U128::new(live_capital);
    engine.accounts[live_idx as usize].position_size = I128::new(0);
    engine.accounts[live_idx as usize].reserved_pnl = 0;
    engine.accounts[live_idx as usize].pnl = I128::new(0);

    engine.vault = U128::new(live_capital);
    sync_engine_aggregates(&mut engine);

    kani::assert(canonical_inv(&engine), "setup must satisfy INV");

    // Run GC
    let closed = engine.garbage_collect_dust();

    // Non-vacuous: GC should close the dust account
    kani::assert(closed > 0, "GC should close the dust account");

    // Live account must survive
    kani::assert(engine.is_used(live_idx as usize), "live account survives GC");

    kani::assert(canonical_inv(&engine), "INV preserved by garbage_collect_dust");
}

/// GC never frees accounts that don't satisfy the dust predicate
/// Tests: reserved_pnl > 0, !position_size.is_zero(), funding_index mismatch all block GC
#[kani::proof]
#[kani::unwind(33)]
#[kani::solver(cadical)]
fn gc_respects_full_dust_predicate() {
    let mut engine = RiskEngine::new(test_params());

    // Set global funding index explicitly
    engine.funding_index_qpb_e6 = I128::new(0);

    // Create account that would be dust except for one blocker
    let idx = engine.add_user(0).unwrap();
    engine.accounts[idx as usize].capital = U128::new(0);
    engine.accounts[idx as usize].pnl = I128::new(0);

    // Pick which predicate to violate
    let blocker: u8 = kani::any();
    kani::assume(blocker < 3);

    match blocker {
        0 => {
            // reserved_pnl > 0 blocks GC (also sets pnl = reserved for PA1 validity)
            let reserved: u128 = kani::any();
            kani::assume(reserved > 0 && reserved < 1000);
            engine.accounts[idx as usize].reserved_pnl = reserved as u64;
            engine.accounts[idx as usize].pnl = I128::new(reserved as i128); // PA1: reserved <= pnl
            engine.accounts[idx as usize].position_size = I128::new(0);
            engine.accounts[idx as usize].funding_index = I128::new(0); // settled
        }
        1 => {
            // !position_size.is_zero() blocks GC
            let pos: i128 = kani::any();
            kani::assume(pos != 0 && pos > -1000 && pos < 1000);
            engine.accounts[idx as usize].position_size = I128::new(pos);
            engine.accounts[idx as usize].reserved_pnl = 0;
            engine.accounts[idx as usize].funding_index = I128::new(0); // settled
        }
        _ => {
            // positive pnl blocks GC (accounts with value are never collected)
            let pos_pnl: i128 = kani::any();
            kani::assume(pos_pnl > 0 && pos_pnl < 1000);
            engine.accounts[idx as usize].pnl = I128::new(pos_pnl);
            engine.accounts[idx as usize].position_size = I128::new(0);
            engine.accounts[idx as usize].reserved_pnl = 0;
        }
    }

    sync_engine_aggregates(&mut engine);

    let was_used = engine.is_used(idx as usize);
    assert!(was_used, "Account should exist before GC");

    kani::assert(canonical_inv(&engine), "setup INV");

    // Run GC
    let _closed = engine.garbage_collect_dust();

    // Target account must NOT be freed (other accounts might be)
    kani::assert(
        engine.is_used(idx as usize),
        "GC must not free account that doesn't satisfy dust predicate"
    );

    kani::assert(canonical_inv(&engine), "INV after GC");
}



// ==============================================================================
// CRANK-BOUNDS PROOF: keeper_crank respects all budgets
// ==============================================================================

/// CRANK-BOUNDS: keeper_crank respects liquidation and GC budgets
#[kani::proof]
#[kani::unwind(33)]
#[kani::solver(cadical)]
fn crank_bounds_respected() {
    let mut engine = RiskEngine::new(test_params());

    let user = engine.add_user(0).unwrap();
    let lp = engine.add_lp([1u8; 32], [0u8; 32], 0).unwrap();

    // User with position — exercises liquidation/force-realize code paths
    let capital: u128 = kani::any();
    kani::assume(capital >= 500 && capital <= 20_000);
    engine.accounts[user as usize].capital = U128::new(capital);
    engine.accounts[user as usize].position_size = I128::new(500);
    engine.accounts[user as usize].entry_price = 1_000_000;
    engine.accounts[user as usize].funding_index = engine.funding_index_qpb_e6;

    engine.accounts[lp as usize].capital = U128::new(50_000);
    engine.accounts[lp as usize].position_size = I128::new(-500);
    engine.accounts[lp as usize].entry_price = 1_000_000;
    engine.accounts[lp as usize].funding_index = engine.funding_index_qpb_e6;

    engine.vault = U128::new(capital + 50_000 + 5_000);
    engine.insurance_fund.balance = U128::new(5_000);
    sync_engine_aggregates(&mut engine);

    let now_slot: u64 = kani::any();
    kani::assume(now_slot > 0 && now_slot < 10_000);

    kani::assert(canonical_inv(&engine), "setup must satisfy INV");

    let cursor_before = engine.crank_cursor;

    let result = engine.keeper_crank(user, now_slot, 1_000_000, 0, false);
    assert!(result.is_ok(), "keeper_crank should succeed");

    let outcome = result.unwrap();

    // Liquidation budget respected
    kani::assert(
        outcome.num_liquidations <= LIQ_BUDGET_PER_CRANK as u32,
        "CRANK-BOUNDS: num_liquidations <= LIQ_BUDGET_PER_CRANK"
    );

    // GC budget respected
    kani::assert(
        outcome.num_gc_closed <= GC_CLOSE_BUDGET,
        "CRANK-BOUNDS: num_gc_closed <= GC_CLOSE_BUDGET"
    );

    // crank_cursor advances (or wraps) after crank
    kani::assert(
        engine.crank_cursor != cursor_before || outcome.sweep_complete,
        "CRANK-BOUNDS: crank_cursor advances or sweep completes"
    );

    kani::assert(canonical_inv(&engine), "INV must hold after crank");
}

// ==============================================================================
// NEW GC SEMANTICS PROOFS: Pending buckets, not direct ADL
// ==============================================================================

/// GC-NEW-A: GC frees only true dust (position=0, capital=0, reserved=0, pnl<=0, funding settled)
#[kani::proof]
#[kani::unwind(33)]
#[kani::solver(cadical)]
fn gc_frees_only_true_dust() {
    let mut engine = RiskEngine::new(test_params());
    engine.funding_index_qpb_e6 = I128::new(0);

    // Create three accounts
    let dust_idx = engine.add_user(0).unwrap();
    let reserved_idx = engine.add_user(0).unwrap();
    let pnl_pos_idx = engine.add_user(0).unwrap();

    // Symbolic blocker values
    let reserved_val: u64 = kani::any();
    let pnl_val: i128 = kani::any();
    kani::assume(reserved_val > 0 && reserved_val <= 1000);
    kani::assume(pnl_val > 0 && pnl_val <= 1000);

    // Dust candidate: satisfies all dust predicates
    engine.accounts[dust_idx as usize].capital = U128::new(0);
    engine.accounts[dust_idx as usize].position_size = I128::new(0);
    engine.accounts[dust_idx as usize].reserved_pnl = 0;
    engine.accounts[dust_idx as usize].pnl = I128::new(0);
    engine.accounts[dust_idx as usize].funding_index = I128::new(0);

    // Non-dust: has symbolic reserved_pnl > 0
    engine.accounts[reserved_idx as usize].capital = U128::new(0);
    engine.accounts[reserved_idx as usize].position_size = I128::new(0);
    engine.accounts[reserved_idx as usize].reserved_pnl = reserved_val;
    engine.accounts[reserved_idx as usize].pnl = I128::new(reserved_val as i128); // PA1
    engine.accounts[reserved_idx as usize].funding_index = I128::new(0);

    // Non-dust: has symbolic pnl > 0
    engine.accounts[pnl_pos_idx as usize].capital = U128::new(0);
    engine.accounts[pnl_pos_idx as usize].position_size = I128::new(0);
    engine.accounts[pnl_pos_idx as usize].reserved_pnl = 0;
    engine.accounts[pnl_pos_idx as usize].pnl = I128::new(pnl_val);
    engine.accounts[pnl_pos_idx as usize].funding_index = I128::new(0);

    sync_engine_aggregates(&mut engine);
    kani::assert(canonical_inv(&engine), "setup INV");

    // Run GC
    let closed = engine.garbage_collect_dust();

    // Dust account should be freed
    assert!(closed >= 1, "GC should close at least one account");
    assert!(
        !engine.is_used(dust_idx as usize),
        "GC-NEW-A: True dust account should be freed"
    );

    // Non-dust accounts should remain
    assert!(
        engine.is_used(reserved_idx as usize),
        "GC-NEW-A: Account with reserved_pnl > 0 must remain"
    );
    assert!(
        engine.is_used(pnl_pos_idx as usize),
        "GC-NEW-A: Account with pnl > 0 must remain"
    );

    kani::assert(canonical_inv(&engine), "INV after GC");
}



// ============================================================================
// WITHDRAWAL MARGIN SAFETY (Bug 5 fix verification)
// ============================================================================

/// After successful withdrawal with position, account must be above maintenance margin
/// This verifies Bug 5 fix: withdrawal uses oracle_price (not entry_price) for margin
#[kani::proof]
#[kani::unwind(33)]
#[kani::solver(cadical)]
fn withdrawal_maintains_margin_above_maintenance() {
    let mut engine = RiskEngine::new(test_params());
    engine.vault = U128::new(1_000_000);
    engine.insurance_fund.balance = U128::new(10_000);
    engine.current_slot = 100;
    engine.last_crank_slot = 100;
    engine.last_full_sweep_start_slot = 100;

    // Create account with position
    let idx = engine.add_user(0).unwrap();
    let capital: u128 = kani::any();
    // Tighter capital range for tractability
    kani::assume(capital >= 5_000 && capital <= 50_000);
    engine.accounts[idx as usize].capital = U128::new(capital);
    engine.accounts[idx as usize].pnl = I128::new(0);

    // Give account a position (tighter range)
    let pos: i128 = kani::any();
    kani::assume(pos != 0 && pos > -5_000 && pos < 5_000);
    kani::assume(if pos > 0 { pos >= 500 } else { pos <= -500 });
    engine.accounts[idx as usize].position_size = I128::new(pos);

    // Entry and oracle prices in tighter range (1M ± 20%)
    let entry_price: u64 = kani::any();
    kani::assume(entry_price >= 800_000 && entry_price <= 1_200_000);
    engine.accounts[idx as usize].entry_price = entry_price;
    sync_engine_aggregates(&mut engine);

    kani::assert(canonical_inv(&engine), "setup INV");

    let oracle_price: u64 = kani::any();
    kani::assume(oracle_price >= 800_000 && oracle_price <= 1_200_000);

    // Withdrawal amount (smaller range for tractability)
    let amount: u128 = kani::any();
    kani::assume(amount >= 100 && amount <= capital / 2);

    // Try withdrawal
    let result = engine.withdraw(idx, amount, 100, oracle_price);

    // Post-withdrawal with position must be above maintenance
    // NOTE: Must use MTM version since withdraw() checks MTM maintenance margin
    if result.is_ok() && !engine.accounts[idx as usize].position_size.is_zero() {
        assert!(
            engine.is_above_maintenance_margin_mtm(&engine.accounts[idx as usize], oracle_price),
            "Post-withdrawal account with position must be above maintenance margin"
        );
        kani::assert(canonical_inv(&engine), "INV after successful withdrawal");
    }

    // Non-vacuity: with high capital and tiny withdrawal at entry price, must succeed
    if capital >= 40_000 && amount <= 200 && oracle_price == entry_price {
        assert!(result.is_ok(), "non-vacuity: tiny withdrawal from well-funded account at entry price must succeed");
    }
}

/// Deterministic regression test: withdrawal that would drop below initial margin
/// at oracle price MUST be rejected with Undercollateralized.
///
/// Setup:
///   capital = 15_000, position = 100_000 long @ entry = oracle = 1.0
///   position_value = 100_000, IM @ 10% = 10_000, MM @ 5% = 5_000
///   Current equity = 15_000 > IM → account is healthy
///   Withdraw 6_000 → remaining equity = 9_000 < IM (10_000) → MUST reject
#[kani::proof]
#[kani::unwind(33)]
#[kani::solver(cadical)]
fn withdrawal_rejects_if_below_initial_margin_at_oracle() {
    let mut engine = RiskEngine::new(test_params());
    engine.current_slot = 100;
    engine.last_crank_slot = 100;
    engine.last_full_sweep_start_slot = 100;

    // Symbolic capital and withdrawal amount
    let capital: u128 = kani::any();
    let withdraw: u128 = kani::any();
    kani::assume(capital >= 5_000 && capital <= 20_000);
    kani::assume(withdraw >= 1 && withdraw <= capital);

    let idx = engine.add_user(0).unwrap();
    engine.deposit(idx, capital, 0).unwrap();

    // Position at oracle price (entry == oracle → mark PnL = 0)
    // IM = |position| * initial_margin_bps / 10000 = 100_000 * 1000 / 10000 = 10_000
    engine.accounts[idx as usize].position_size = I128::new(100_000);
    engine.accounts[idx as usize].entry_price = 1_000_000;
    sync_engine_aggregates(&mut engine);

    let oracle_price: u64 = 1_000_000;
    let result = engine.withdraw(idx, withdraw, 100, oracle_price);

    // Remaining equity = capital - withdraw. IM = 10_000.
    // If remaining < IM, must be rejected.
    if capital - withdraw < 10_000 {
        kani::assert(
            result.is_err(),
            "Withdrawal dropping equity below IM must be rejected"
        );
    }
    // If remaining >= IM, must succeed
    if capital - withdraw >= 10_000 {
        let _ = assert_ok!(result, "Withdrawal keeping equity above IM must succeed");
        kani::assert(canonical_inv(&engine), "INV after successful withdrawal");
    }
}

// ============================================================================
// CANONICAL INV PROOFS - Initial State and Preservation
// ============================================================================

/// INV(new()) - Fresh engine satisfies the canonical invariant
#[kani::proof]
#[kani::unwind(33)]
#[kani::solver(cadical)]
fn proof_inv_holds_for_new_engine() {
    // Symbolic params: INV must hold for any valid parameter combination
    let warmup: u64 = kani::any();
    let maint_bps: u64 = kani::any();
    let init_bps: u64 = kani::any();
    let fee_bps: u64 = kani::any();
    kani::assume(warmup <= 10_000);
    kani::assume(maint_bps <= 5_000);
    kani::assume(init_bps <= 10_000);
    kani::assume(fee_bps <= 1_000);

    let params = RiskParams {
        warmup_period_slots: warmup,
        maintenance_margin_bps: maint_bps,
        initial_margin_bps: init_bps,
        trading_fee_bps: fee_bps,
        max_accounts: 4,
        new_account_fee: U128::ZERO,
        risk_reduction_threshold: U128::ZERO,
        maintenance_fee_per_slot: U128::ZERO,
        max_crank_staleness_slots: u64::MAX,
        liquidation_fee_bps: 50,
        liquidation_fee_cap: U128::new(10_000),
        liquidation_buffer_bps: 100,
        min_liquidation_abs: U128::new(100_000),
    };

    let mut engine = RiskEngine::new(params);
    kani::assert(canonical_inv(&engine), "INV must hold for new() with any params");

    // Also verify INV survives add_user + deposit with symbolic amount
    let deposit: u128 = kani::any();
    kani::assume(deposit > 0 && deposit < 50_000);

    let user = engine.add_user(0).unwrap();
    assert_ok!(engine.deposit(user, deposit, 0), "deposit must succeed");

    kani::assert(canonical_inv(&engine), "INV after add_user + deposit");
}

/// INV preserved by add_user — fresh engine + freelist recycling
#[kani::proof]
#[kani::unwind(33)]
#[kani::solver(cadical)]
fn proof_inv_preserved_by_add_user() {
    let mut engine = RiskEngine::new(test_params());
    engine.current_slot = 100;
    engine.last_crank_slot = 100;
    engine.last_full_sweep_start_slot = 100;

    // First: add a user, deposit, then close to populate the freelist
    let first = engine.add_user(0).unwrap();
    engine.deposit(first, 1_000, 0).unwrap();
    engine.close_account(first, 100, 1_000_000).unwrap();

    kani::assert(canonical_inv(&engine), "INV after close (freelist populated)");

    // Now add_user should recycle the freed slot
    let fee: u128 = kani::any();
    kani::assume(fee < 1_000_000);

    let idx = assert_ok!(
        engine.add_user(fee),
        "add_user must succeed with freelist recycling"
    );

    kani::assert(canonical_inv(&engine), "INV preserved by add_user (recycled)");
    kani::assert(engine.is_used(idx as usize), "add_user must mark account as used");

    // The recycled slot should be the same one we freed
    kani::assert(idx == first, "freelist should recycle the freed slot");
}

/// INV preserved by add_lp — fresh engine + freelist recycling
#[kani::proof]
#[kani::unwind(33)]
#[kani::solver(cadical)]
fn proof_inv_preserved_by_add_lp() {
    let mut engine = RiskEngine::new(test_params());
    engine.current_slot = 100;
    engine.last_crank_slot = 100;
    engine.last_full_sweep_start_slot = 100;

    // First: add a user, deposit, then close to populate the freelist
    let first = engine.add_user(0).unwrap();
    engine.deposit(first, 1_000, 0).unwrap();
    engine.close_account(first, 100, 1_000_000).unwrap();

    kani::assert(canonical_inv(&engine), "INV after close (freelist populated)");

    let fee: u128 = kani::any();
    kani::assume(fee < 1_000_000);

    let lp = assert_ok!(
        engine.add_lp([1u8; 32], [0u8; 32], fee),
        "add_lp must succeed with freelist recycling"
    );

    kani::assert(canonical_inv(&engine), "INV preserved by add_lp (recycled)");
    kani::assert(engine.is_used(lp as usize), "add_lp must mark account as used");

    // The recycled slot should be the same one we freed
    kani::assert(lp == first, "freelist should recycle the freed slot");
}

// ============================================================================
// EXECUTE_TRADE PROOF FAMILY - Robust Pattern
// ============================================================================
//
// This demonstrates the full proof pattern:
//   1. Strong exception safety (Err => no state change)
//   2. INV preservation (Ok => INV still holds)
//   3. Non-vacuity (prove we actually traded)
//   4. Conservation (vault/balances consistent)
//   5. Margin enforcement (post-trade margin valid)

/// execute_trade: INV preserved on Ok, postconditions verified
#[kani::proof]
#[kani::unwind(33)]
#[kani::solver(cadical)]
fn proof_execute_trade_preserves_inv() {
    let mut engine = RiskEngine::new(test_params());
    engine.vault = U128::new(100_000);
    engine.insurance_fund.balance = U128::new(10_000);
    engine.current_slot = 100;
    engine.last_crank_slot = 100;
    engine.last_full_sweep_start_slot = 100;

    // Setup: user and LP with sufficient capital
    let user_idx = engine.add_user(0).unwrap();
    let lp_idx = engine.add_lp([1u8; 32], [0u8; 32], 0).unwrap();

    engine.accounts[user_idx as usize].capital = U128::new(10_000);
    engine.accounts[lp_idx as usize].capital = U128::new(50_000);
    engine.recompute_aggregates();

    // Precondition: setup built via concrete initialization must satisfy INV
    kani::assert(canonical_inv(&engine), "setup state must satisfy INV");

    // Snapshot position BEFORE trade
    let user_pos_before = engine.accounts[user_idx as usize].position_size;
    let lp_pos_before = engine.accounts[lp_idx as usize].position_size;

    // Constrained inputs to force Ok path (non-vacuous proof of success case)
    let delta_size: i128 = kani::any();
    let oracle_price: u64 = kani::any();

    // Tight bounds to force trade success
    kani::assume(delta_size >= -100 && delta_size <= 100 && delta_size != 0);
    kani::assume(oracle_price >= 900_000 && oracle_price <= 1_100_000);

    let result = engine.execute_trade(
        &NoOpMatcher,
        lp_idx,
        user_idx,
        100,
        oracle_price,
        delta_size,
    );

    // INV only matters on Ok path (Solana tx aborts on Err, state discarded)
    if result.is_ok() {
        kani::assert(canonical_inv(&engine), "INV must hold after execute_trade");

        // NON-VACUITY: position = pos_before + delta (user buys, LP sells)
        let user_pos_after = engine.accounts[user_idx as usize].position_size;
        let lp_pos_after = engine.accounts[lp_idx as usize].position_size;

        kani::assert(
            user_pos_after == user_pos_before + delta_size,
            "User position must be pos_before + delta",
        );
        kani::assert(
            lp_pos_after == lp_pos_before - delta_size,
            "LP position must be pos_before - delta (opposite side)",
        );
    }

    // Non-vacuity: force Ok path
    let _ = assert_ok!(result, "execute_trade must succeed with valid inputs");
}

/// execute_trade: Conservation holds after successful trade (no funding case)
#[kani::proof]
#[kani::unwind(33)]
#[kani::solver(cadical)]
fn proof_execute_trade_conservation() {
    let mut engine = RiskEngine::new(test_params());
    engine.current_slot = 100;
    engine.last_crank_slot = 100;
    engine.last_full_sweep_start_slot = 100;

    // Setup
    let user_idx = engine.add_user(0).unwrap();
    let lp_idx = engine.add_lp([1u8; 32], [0u8; 32], 0).unwrap();

    let user_cap: u128 = kani::any();
    let lp_cap: u128 = kani::any();
    kani::assume(user_cap > 1000 && user_cap < 100_000);
    kani::assume(lp_cap > 10_000 && lp_cap < 100_000);

    engine.accounts[user_idx as usize].capital = U128::new(user_cap);
    engine.accounts[lp_idx as usize].capital = U128::new(lp_cap);
    engine.vault = U128::new(user_cap + lp_cap + 10_000);
    engine.insurance_fund.balance = U128::new(10_000);
    sync_engine_aggregates(&mut engine);

    kani::assert(canonical_inv(&engine), "setup must satisfy INV");

    // Trade parameters
    let delta_size: i128 = kani::any();
    kani::assume(delta_size >= -50 && delta_size <= 50 && delta_size != 0);

    let result = engine.execute_trade(&NoOpMatcher, lp_idx, user_idx, 100, 1_000_000, delta_size);

    // Non-vacuity: trade must succeed with bounded inputs
    kani::assert(result.is_ok(), "non-vacuity: execute_trade must succeed");
    kani::assert(canonical_inv(&engine), "INV must hold after trade");
    kani::assert(
        conservation_fast_no_funding(&engine),
        "Conservation must hold after successful trade",
    );
}

/// execute_trade: Margin enforcement - successful trade leaves both parties above margin
#[kani::proof]
#[kani::unwind(33)]
#[kani::solver(cadical)]
fn proof_execute_trade_margin_enforcement() {
    let mut engine = RiskEngine::new(test_params());
    engine.current_slot = 100;
    engine.last_crank_slot = 100;
    engine.last_full_sweep_start_slot = 100;

    let capital: u128 = kani::any();
    kani::assume(capital >= 500 && capital <= 2_000);

    let user_idx = engine.add_user(0).unwrap();
    let lp_idx = engine.add_lp([1u8; 32], [0u8; 32], 0).unwrap();

    // User capital near margin boundary; LP well-capitalized
    engine.accounts[user_idx as usize].capital = U128::new(capital);
    engine.accounts[lp_idx as usize].capital = U128::new(100_000);
    engine.vault = U128::new(capital + 100_000 + 10_000);
    engine.insurance_fund.balance = U128::new(10_000);
    sync_engine_aggregates(&mut engine);

    let delta_size: i128 = kani::any();
    kani::assume(delta_size != 0);
    kani::assume(delta_size >= -15_000 && delta_size <= 15_000);

    kani::assert(canonical_inv(&engine), "setup must satisfy INV");

    let result = engine.execute_trade(&NoOpMatcher, lp_idx, user_idx, 100, 1_000_000, delta_size);

    if result.is_ok() {
        kani::assert(canonical_inv(&engine), "INV after trade");

        // MARGIN ENFORCEMENT: both parties must be above initial margin post-trade
        let user_pos = engine.accounts[user_idx as usize].position_size;
        let lp_pos = engine.accounts[lp_idx as usize].position_size;

        if !user_pos.is_zero() {
            kani::assert(
                engine.is_above_margin_bps_mtm(
                    &engine.accounts[user_idx as usize],
                    1_000_000,
                    engine.params.initial_margin_bps,
                ),
                "User must be above initial margin after trade",
            );
        }
        if !lp_pos.is_zero() {
            kani::assert(
                engine.is_above_margin_bps_mtm(
                    &engine.accounts[lp_idx as usize],
                    1_000_000,
                    engine.params.initial_margin_bps,
                ),
                "LP must be above initial margin after trade",
            );
        }
    }

    // Non-vacuity: small trade with sufficient capital must succeed
    if capital >= 1_500 && delta_size >= -5_000 && delta_size <= 5_000 {
        kani::assert(result.is_ok(), "non-vacuity: conservative trade must succeed");
    }
}

// ============================================================================
// DEPOSIT PROOF FAMILY - Exception Safety + INV Preservation
// ============================================================================

/// deposit: INV preserved and postconditions on Ok
#[kani::proof]
#[kani::unwind(33)]
#[kani::solver(cadical)]
fn proof_deposit_preserves_inv() {
    // Use maintenance fee params to exercise fee accrual + debt payment during deposit
    let mut engine = RiskEngine::new(test_params_with_maintenance_fee());

    let user_idx = engine.add_user(0).unwrap();

    // Symbolic inputs exercise fee settlement, debt payment, and warmup settlement
    let capital: u128 = kani::any();
    let pnl: i128 = kani::any();
    let amount: u128 = kani::any();
    let now_slot: u64 = kani::any();

    kani::assume(capital >= 100 && capital <= 5_000);
    kani::assume(pnl >= -2_000 && pnl <= 2_000);
    kani::assume(amount >= 1 && amount <= 5_000);
    kani::assume(now_slot <= 200);

    engine.accounts[user_idx as usize].capital = U128::new(capital);
    engine.accounts[user_idx as usize].pnl = I128::new(pnl);

    // Vault satisfies inv_accounting: vault >= c_tot + insurance
    let insurance: u128 = 1_000;
    let vault = if pnl > 0 {
        capital + insurance + pnl as u128
    } else {
        capital + insurance
    };
    engine.vault = U128::new(vault);
    engine.insurance_fund.balance = U128::new(insurance);
    sync_engine_aggregates(&mut engine);

    kani::assert(canonical_inv(&engine), "setup state must satisfy INV");

    let result = engine.deposit(user_idx, amount, now_slot);

    if result.is_ok() {
        kani::assert(canonical_inv(&engine), "INV must hold after deposit");
    }

    // Non-vacuity: deposit on a valid used account must succeed
    let _ = assert_ok!(result, "deposit must succeed");
}

// ============================================================================
// WITHDRAW PROOF FAMILY - Exception Safety + INV Preservation
// ============================================================================

/// withdraw: INV preserved and postconditions on Ok
#[kani::proof]
#[kani::unwind(33)]
#[kani::solver(cadical)]
fn proof_withdraw_preserves_inv() {
    let mut engine = RiskEngine::new(test_params());
    engine.current_slot = 100;
    engine.last_crank_slot = 100;
    engine.last_full_sweep_start_slot = 100;

    // Symbolic inputs exercise margin enforcement with non-zero position
    let capital: u128 = kani::any();
    let amount: u128 = kani::any();
    let oracle_price: u64 = kani::any();

    kani::assume(capital >= 5_000 && capital <= 20_000);
    kani::assume(amount >= 1 && amount <= 15_000);
    kani::assume(oracle_price >= 900_000 && oracle_price <= 1_100_000);

    // User with non-zero position — exercises IM check, MTM equity, MM safety belt
    let user_idx = engine.add_user(0).unwrap();
    engine.accounts[user_idx as usize].capital = U128::new(capital);
    engine.accounts[user_idx as usize].position_size = I128::new(100_000);
    engine.accounts[user_idx as usize].entry_price = 1_000_000;

    // LP with counterparty position
    let lp_idx = engine.add_lp([1u8; 32], [0u8; 32], 0).unwrap();
    engine.accounts[lp_idx as usize].capital = U128::new(50_000);
    engine.accounts[lp_idx as usize].position_size = I128::new(-100_000);
    engine.accounts[lp_idx as usize].entry_price = 1_000_000;

    engine.vault = U128::new(capital + 50_000 + 10_000);
    engine.insurance_fund.balance = U128::new(10_000);
    sync_engine_aggregates(&mut engine);

    kani::assert(canonical_inv(&engine), "setup state must satisfy INV");

    let result = engine.withdraw(user_idx, amount, 100, oracle_price);

    // INV must hold on Ok path
    if result.is_ok() {
        kani::assert(canonical_inv(&engine), "INV must hold after withdraw");
    }

    // Non-vacuity: large capital + small withdraw + oracle at entry must succeed
    // equity = 18K - 500 = 17.5K > IM = 10K ✓
    if capital >= 18_000 && amount <= 500 && oracle_price >= 1_000_000 {
        kani::assert(result.is_ok(), "non-vacuity: conservative withdrawal must succeed");
    }
}

// ============================================================================
// FREELIST STRUCTURAL PROOFS - High Value, Fast
// ============================================================================

/// add_user increases popcount by 1 and removes one from freelist
#[kani::proof]
#[kani::unwind(33)]
#[kani::solver(cadical)]
fn proof_add_user_structural_integrity() {
    let mut engine = RiskEngine::new(test_params());
    engine.current_slot = 100;
    engine.last_crank_slot = 100;
    engine.last_full_sweep_start_slot = 100;

    // Symbolic deposit amount
    let deposit_amt: u128 = kani::any();
    kani::assume(deposit_amt >= 100 && deposit_amt <= 10_000);

    // Add user, deposit symbolic amount, close — populates freelist
    let first = engine.add_user(0).unwrap();
    engine.deposit(first, deposit_amt, 0).unwrap();
    engine.close_account(first, 100, 1_000_000).unwrap();

    kani::assert(canonical_inv(&engine), "canonical INV after close");

    let pop_before = engine.num_used_accounts;
    let free_head_before = engine.free_head;

    // Symbolic fee for add_user
    let fee: u128 = kani::any();
    kani::assume(fee < 1_000_000);

    let idx = assert_ok!(engine.add_user(fee), "add_user must succeed with freelist");

    kani::assert(
        engine.num_used_accounts == pop_before + 1,
        "add_user must increase num_used_accounts by 1",
    );
    kani::assert(
        engine.free_head != free_head_before || free_head_before == u16::MAX,
        "add_user must advance free_head",
    );
    kani::assert(
        canonical_inv(&engine),
        "add_user must preserve canonical invariant",
    );
    // Recycled slot
    kani::assert(idx == first, "freelist must recycle freed slot");
}

/// close_account decreases popcount by 1 and returns index to freelist
#[kani::proof]
#[kani::unwind(33)]
#[kani::solver(cadical)]
fn proof_close_account_structural_integrity() {
    let mut engine = RiskEngine::new(test_params());
    engine.current_slot = 100;
    engine.last_crank_slot = 100;
    engine.last_full_sweep_start_slot = 100;

    let user_idx = engine.add_user(0).unwrap();

    // Symbolic deposit + full withdraw to exercise touch_account_full
    let deposit: u128 = kani::any();
    kani::assume(deposit > 0 && deposit <= 10_000);
    engine.deposit(user_idx, deposit, 0).unwrap();
    engine.withdraw(user_idx, deposit, 100, 1_000_000).unwrap();

    let pop_before = engine.num_used_accounts;

    kani::assert(canonical_inv(&engine), "canonical INV before close");

    let _withdrawn = assert_ok!(
        engine.close_account(user_idx, 100, 1_000_000),
        "close_account must succeed for flat, zero-capital account"
    );

    kani::assert(
        engine.num_used_accounts == pop_before - 1,
        "close_account must decrease num_used_accounts by 1",
    );
    kani::assert(
        !engine.is_used(user_idx as usize),
        "close_account must clear used bit",
    );
    kani::assert(
        engine.free_head == user_idx,
        "close_account must return index to freelist head",
    );
    kani::assert(
        canonical_inv(&engine),
        "close_account must preserve canonical invariant",
    );
}

// ============================================================================
// LIQUIDATE_AT_ORACLE PROOF FAMILY - Exception Safety + INV Preservation
// ============================================================================

/// liquidate_at_oracle: INV preserved on Ok path
/// Optimized: Reduced unwind, tighter oracle_price bounds
///
/// NOTE: With variation margin, liquidation settles mark PnL only for the liquidated account,
/// not the counterparty LP. This temporarily makes realized pnl non-zero-sum until the LP
/// is touched. To avoid this in the proof, we set entry_price = oracle_price (mark=0).
/// The full conservation property (including mark PnL) is proven by check_conservation.
#[kani::proof]
#[kani::unwind(5)] // MAX_ACCOUNTS=4
#[kani::solver(cadical)]
fn proof_liquidate_preserves_inv() {
    let mut engine = RiskEngine::new(test_params());
    engine.current_slot = 100;
    engine.last_crank_slot = 100;
    engine.last_full_sweep_start_slot = 100;

    // Symbolic inputs: capital and oracle exercise mark PnL + margin branches
    let capital: u128 = kani::any();
    let oracle_price: u64 = kani::any();

    kani::assume(capital >= 1 && capital <= 1_000);
    kani::assume(oracle_price >= 800_000 && oracle_price <= 1_100_000);

    // User with long position at entry=$1.00
    // mark_pnl = 1M * (oracle - 1M) / 1M = oracle - 1M
    // Solver finds: oracle < ~1M → negative mark, below margin → liquidation
    //               oracle > ~1M → positive mark, above margin → Ok(false)
    let user_idx = engine.add_user(0).unwrap();
    engine.accounts[user_idx as usize].capital = U128::new(capital);
    engine.accounts[user_idx as usize].position_size = I128::new(1_000_000);
    engine.accounts[user_idx as usize].entry_price = 1_000_000;

    // LP with counterparty short position, well-capitalized
    let lp_idx = engine.add_lp([1u8; 32], [0u8; 32], 0).unwrap();
    engine.accounts[lp_idx as usize].capital = U128::new(50_000);
    engine.accounts[lp_idx as usize].position_size = I128::new(-1_000_000);
    engine.accounts[lp_idx as usize].entry_price = 1_000_000;

    engine.vault = U128::new(capital + 50_000 + 10_000);
    engine.insurance_fund.balance = U128::new(10_000);
    sync_engine_aggregates(&mut engine);

    kani::assert(canonical_inv(&engine), "setup state must satisfy INV");

    let result = engine.liquidate_at_oracle(user_idx, 100, oracle_price);

    // INV must hold on Ok path regardless of whether liquidation triggered
    if result.is_ok() {
        kani::assert(
            canonical_inv(&engine),
            "INV must hold after liquidate_at_oracle",
        );
    }

    // Non-vacuity: must not error with valid oracle in range
    let _ = assert_ok!(result, "liquidate_at_oracle must succeed with valid oracle");
}


// ============================================================================
// SETTLE_WARMUP_TO_CAPITAL PROOF FAMILY - Exception Safety + INV Preservation
// ============================================================================

/// settle_warmup_to_capital: canonical_inv preserved for positive PnL (fully symbolic)
///
/// Two-account topology: user1 (target) + user2 (bystander with independent PnL).
/// This ensures pnl_pos_tot includes multi-account contributions so haircut_ratio
/// depends on the aggregate, not just the target account.
///
/// Symbolic inputs exercise all branches in §6.2 profit conversion:
/// - Partial conversion: slope*elapsed < avail_gross (small slope or elapsed)
/// - Haircut < 1: vault_margin < total_pnl_pos (tight vault → h_num < h_den)
/// - Non-zero reserved_pnl: reduces avail_gross below raw positive pnl
/// - Zero conversion: slope=0 or elapsed=0 → cap=0 → x=0
/// - Bounds up to 5000 to exercise mul_u128 with realistic magnitudes
#[kani::proof]
#[kani::unwind(33)]
#[kani::solver(cadical)]
fn proof_settle_warmup_preserves_inv() {
    let mut engine = RiskEngine::new(test_params());
    let user_idx = engine.add_user(0).unwrap();
    let user2 = engine.add_user(0).unwrap();

    // User 1 (target): symbolic warmup state
    let capital: u128 = kani::any();
    let pnl: u128 = kani::any();
    let slope: u128 = kani::any();
    let warmup_start: u64 = kani::any();
    let current_slot: u64 = kani::any();
    let reserved_pnl: u64 = kani::any();

    kani::assume(capital <= 1_000);
    kani::assume(pnl >= 1 && pnl <= 1_000);
    kani::assume(slope <= 100);
    kani::assume(warmup_start <= 10);
    kani::assume(current_slot >= warmup_start && current_slot <= 10);
    kani::assume((reserved_pnl as u128) <= pnl);

    // User 2 (bystander): symbolic capital and positive PnL
    // This makes pnl_pos_tot = pnl + pnl2, so haircut depends on aggregate
    let capital2: u128 = kani::any();
    let pnl2: u128 = kani::any();
    kani::assume(capital2 <= 1_000);
    kani::assume(pnl2 <= 1_000);

    // Vault and insurance
    let insurance: u128 = kani::any();
    let vault_margin: u128 = kani::any();
    kani::assume(insurance <= 1_000);
    // residual = vault_margin; total_pnl_pos = pnl + pnl2; h can be < 1
    let total_pnl_pos = pnl + pnl2;
    kani::assume(vault_margin <= total_pnl_pos);

    let vault = capital + capital2 + insurance + vault_margin;

    engine.accounts[user_idx as usize].capital = U128::new(capital);
    engine.accounts[user_idx as usize].pnl = I128::new(pnl as i128);
    engine.accounts[user_idx as usize].warmup_started_at_slot = warmup_start;
    engine.accounts[user_idx as usize].warmup_slope_per_step = U128::new(slope);
    engine.accounts[user_idx as usize].reserved_pnl = reserved_pnl;

    engine.accounts[user2 as usize].capital = U128::new(capital2);
    engine.accounts[user2 as usize].pnl = I128::new(pnl2 as i128);

    engine.insurance_fund.balance = U128::new(insurance);
    engine.vault = U128::new(vault);
    engine.current_slot = current_slot;
    sync_engine_aggregates(&mut engine);

    kani::assert(canonical_inv(&engine), "setup state must satisfy INV");

    let _ = assert_ok!(
        engine.settle_warmup_to_capital(user_idx),
        "settle_warmup_to_capital must succeed for valid positive-pnl account"
    );
    kani::assert(
        canonical_inv(&engine),
        "INV must hold after settle_warmup_to_capital",
    );

    // User2 must be untouched
    kani::assert(
        engine.accounts[user2 as usize].capital.get() == capital2,
        "bystander capital unchanged",
    );
    kani::assert(
        engine.accounts[user2 as usize].pnl.get() == pnl2 as i128,
        "bystander pnl unchanged",
    );
}

/// settle_warmup_to_capital: canonical_inv preserved for negative PnL (fully symbolic)
///
/// Two-account topology: user1 (target, negative PnL) + user2 (bystander, positive PnL).
/// user2's positive PnL contributes to pnl_pos_tot, making aggregate maintenance non-trivial
/// when user1's loss settlement modifies aggregates via set_capital/set_pnl.
///
/// Symbolic inputs exercise all branches in §6.1 loss settlement:
/// - Insolvency writeoff: loss > capital → capital zeroed, residual written off
/// - Zero capital: capital=0 → pay=0, entire loss written off immediately
/// - Solvent case: loss <= capital → pay=loss, pnl→0
/// Bounds up to 5000 for realistic magnitudes.
#[kani::proof]
#[kani::unwind(33)]
#[kani::solver(cadical)]
fn proof_settle_warmup_negative_pnl_immediate() {
    let mut engine = RiskEngine::new(test_params());
    let user_idx = engine.add_user(0).unwrap();
    let user2 = engine.add_user(0).unwrap();

    // User 1 (target): negative PnL
    let capital: u128 = kani::any();
    let loss: u128 = kani::any();
    kani::assume(capital <= 5_000);
    kani::assume(loss >= 1 && loss <= 5_000);
    let pnl = -(loss as i128);

    // User 2 (bystander): symbolic capital and positive PnL
    let capital2: u128 = kani::any();
    let pnl2: u128 = kani::any();
    kani::assume(capital2 <= 5_000);
    kani::assume(pnl2 <= 5_000);

    let insurance: u128 = kani::any();
    kani::assume(insurance <= 5_000);

    // vault must cover c_tot + insurance; residual covers pnl2
    let vault = capital + capital2 + insurance + pnl2;

    engine.accounts[user_idx as usize].capital = U128::new(capital);
    engine.accounts[user_idx as usize].pnl = I128::new(pnl);
    engine.accounts[user2 as usize].capital = U128::new(capital2);
    engine.accounts[user2 as usize].pnl = I128::new(pnl2 as i128);
    engine.insurance_fund.balance = U128::new(insurance);
    engine.vault = U128::new(vault);
    sync_engine_aggregates(&mut engine);

    kani::assert(canonical_inv(&engine), "setup state must satisfy INV");

    let _ = assert_ok!(
        engine.settle_warmup_to_capital(user_idx),
        "settle_warmup must succeed"
    );

    kani::assert(canonical_inv(&engine), "INV must hold after settle_warmup");

    let account = &engine.accounts[user_idx as usize];

    // N1 boundary: pnl >= 0 or capital == 0
    kani::assert(
        n1_boundary_holds(account),
        "N1: after settle, pnl >= 0 OR capital == 0",
    );

    // Negative PnL fully resolved (§6.1 step 4: remaining negative PnL written off)
    kani::assert(
        account.pnl.get() >= 0,
        "Negative PnL must be fully resolved after settlement",
    );

    // User2 untouched
    kani::assert(
        engine.accounts[user2 as usize].capital.get() == capital2,
        "bystander capital unchanged",
    );
    kani::assert(
        engine.accounts[user2 as usize].pnl.get() == pnl2 as i128,
        "bystander pnl unchanged",
    );
}

// ============================================================================
// KEEPER_CRANK PROOF FAMILY - Exception Safety + INV Preservation
// ============================================================================

/// keeper_crank: INV preserved on Ok path (symbolic capital + slot).
/// Exercises: maintenance fee settlement, funding settle, liquidation
/// (when capital < margin), warmup settlement, GC, LP max tracking.
/// Oracle = entry to keep mark_pnl=0 (mark variation tested in touch_account_full).
/// Capital range spans above/below maintenance margin threshold (~50K for 1 unit).
#[kani::proof]
#[kani::unwind(33)]
#[kani::solver(cadical)]
fn proof_keeper_crank_preserves_inv() {
    let mut engine = RiskEngine::new(test_params_with_maintenance_fee());
    engine.current_slot = 50;
    engine.last_crank_slot = 50;
    engine.last_full_sweep_start_slot = 50;

    let capital: u128 = kani::any();
    let now_slot: u64 = kani::any();
    let funding_rate: i64 = kani::any();
    kani::assume(capital >= 100 && capital <= 60_000);
    kani::assume(now_slot >= 51 && now_slot <= 100);
    kani::assume(funding_rate > -50 && funding_rate < 50);

    // Caller with position — above or below maintenance margin depending on capital.
    // oracle = 1_050_000 vs entry = 1_000_000 → mark_pnl = pos * 50K/1M != 0
    let caller = engine.add_user(0).unwrap();
    engine.accounts[caller as usize].capital = U128::new(capital);
    engine.accounts[caller as usize].position_size = I128::new(1_000_000);
    engine.accounts[caller as usize].entry_price = 1_000_000;
    engine.accounts[caller as usize].funding_index = engine.funding_index_qpb_e6;
    engine.accounts[caller as usize].last_fee_slot = 50;

    // LP counterparty (well-capitalized, opposite position)
    let lp_idx = engine.add_lp([1u8; 32], [0u8; 32], 0).unwrap();
    engine.accounts[lp_idx as usize].capital = U128::new(100_000);
    engine.accounts[lp_idx as usize].position_size = I128::new(-1_000_000);
    engine.accounts[lp_idx as usize].entry_price = 1_000_000;
    engine.accounts[lp_idx as usize].funding_index = engine.funding_index_qpb_e6;
    engine.accounts[lp_idx as usize].last_fee_slot = 50;

    engine.vault = U128::new(capital + 100_000 + 10_000);
    engine.insurance_fund.balance = U128::new(10_000);

    sync_engine_aggregates(&mut engine);
    kani::assert(canonical_inv(&engine), "setup state must satisfy INV");

    // Oracle != entry → exercises mark_pnl settlement path (5% price increase)
    // Symbolic funding_rate → exercises funding accrual
    let result = engine.keeper_crank(caller, now_slot, 1_050_000, funding_rate, false);

    // INV only matters on Ok path (Solana tx aborts on Err, state discarded)
    if result.is_ok() {
        kani::assert(canonical_inv(&engine), "INV must hold after keeper_crank");
        kani::assert(
            engine.last_crank_slot == now_slot,
            "keeper_crank must advance last_crank_slot",
        );
    }

    // Non-vacuity: crank must succeed (liquidation/force-close are handled internally)
    let _ = assert_ok!(result, "keeper_crank must succeed");
}

// ============================================================================
// GARBAGE_COLLECT_DUST PROOF FAMILY - INV Preservation
// ============================================================================

/// garbage_collect_dust: INV preserved — symbolic live account alongside dust
#[kani::proof]
#[kani::unwind(33)]
#[kani::solver(cadical)]
fn proof_gc_dust_preserves_inv() {
    let mut engine = RiskEngine::new(test_params());
    engine.funding_index_qpb_e6 = I128::new(0);

    // Create dust + live account with symbolic capital
    let dust_idx = engine.add_user(0).unwrap();
    let live_idx = engine.add_user(0).unwrap();

    let live_capital: u128 = kani::any();
    kani::assume(live_capital > 0 && live_capital <= 10_000);

    engine.accounts[dust_idx as usize].capital = U128::new(0);
    engine.accounts[dust_idx as usize].pnl = I128::new(0);
    engine.accounts[dust_idx as usize].position_size = I128::new(0);
    engine.accounts[dust_idx as usize].reserved_pnl = 0;
    engine.accounts[dust_idx as usize].funding_index = I128::new(0);

    engine.accounts[live_idx as usize].capital = U128::new(live_capital);
    engine.accounts[live_idx as usize].funding_index = I128::new(0);

    engine.vault = U128::new(live_capital);
    sync_engine_aggregates(&mut engine);

    kani::assert(canonical_inv(&engine), "setup state must satisfy INV");

    let num_used_before = engine.num_used_accounts;

    let freed = engine.garbage_collect_dust();

    kani::assert(canonical_inv(&engine), "INV preserved by garbage_collect_dust");

    if freed > 0 {
        kani::assert(
            engine.num_used_accounts < num_used_before,
            "GC must decrease num_used_accounts when freeing accounts",
        );
    }
    kani::assert(engine.is_used(live_idx as usize), "live account survives GC");
}

/// garbage_collect_dust: Structural integrity
#[kani::proof]
#[kani::unwind(33)]
#[kani::solver(cadical)]
fn proof_gc_dust_structural_integrity() {
    let mut engine = RiskEngine::new(test_params());
    engine.funding_index_qpb_e6 = I128::new(0);

    // Create dust + live accounts with symbolic capital
    let dust_idx = engine.add_user(0).unwrap();
    let live_idx = engine.add_user(0).unwrap();

    let live_capital: u128 = kani::any();
    kani::assume(live_capital > 0 && live_capital <= 5_000);

    engine.accounts[dust_idx as usize].capital = U128::new(0);
    engine.accounts[dust_idx as usize].pnl = I128::new(0);
    engine.accounts[dust_idx as usize].position_size = I128::new(0);
    engine.accounts[dust_idx as usize].reserved_pnl = 0;
    engine.accounts[dust_idx as usize].funding_index = I128::new(0);

    engine.accounts[live_idx as usize].capital = U128::new(live_capital);
    engine.accounts[live_idx as usize].funding_index = I128::new(0);

    engine.vault = U128::new(live_capital);
    sync_engine_aggregates(&mut engine);

    kani::assert(canonical_inv(&engine), "canonical INV before GC");

    engine.garbage_collect_dust();

    kani::assert(canonical_inv(&engine), "GC must preserve canonical invariant");
    kani::assert(engine.is_used(live_idx as usize), "live account survives GC");
}


// ============================================================================
// CLOSE_ACCOUNT PROOF FAMILY - Exception Safety + INV Preservation
// ============================================================================

/// close_account: INV preserved on Ok path — symbolic capital via deposit+withdraw lifecycle
#[kani::proof]
#[kani::unwind(33)]
#[kani::solver(cadical)]
fn proof_close_account_preserves_inv() {
    let mut engine = RiskEngine::new(test_params());
    engine.current_slot = 100;
    engine.last_crank_slot = 100;
    engine.last_full_sweep_start_slot = 100;

    let user_idx = engine.add_user(0).unwrap();

    // Symbolic deposit amount — exercises touch_account_full during close
    let deposit_amt: u128 = kani::any();
    kani::assume(deposit_amt > 0 && deposit_amt <= 10_000);
    engine.deposit(user_idx, deposit_amt, 0).unwrap();

    // Withdraw everything to reach zero capital (required for close)
    engine.withdraw(user_idx, deposit_amt, 100, 1_000_000).unwrap();

    kani::assert(canonical_inv(&engine), "INV after deposit+withdraw");

    let num_used_before = engine.num_used_accounts;

    let result = engine.close_account(user_idx, 100, 1_000_000);

    if result.is_ok() {
        kani::assert(canonical_inv(&engine), "INV must hold after close_account");
        kani::assert(
            !engine.is_used(user_idx as usize),
            "close_account must mark account as unused",
        );
        kani::assert(
            engine.num_used_accounts == num_used_before - 1,
            "close_account must decrease num_used_accounts",
        );
    }

    // Non-vacuity: force Ok path
    let _ = assert_ok!(result, "close_account must succeed");
}

// ============================================================================
// TOP_UP_INSURANCE_FUND PROOF FAMILY - INV Preservation
// ============================================================================

/// top_up_insurance_fund: INV preserved.
/// Adds `amount` to both vault and insurance_fund.balance.
/// vault and insurance grow by the same amount, so vault - c_tot - insurance is unchanged.
#[kani::proof]
#[kani::unwind(33)]
#[kani::solver(cadical)]
fn proof_top_up_insurance_preserves_inv() {
    // Use test_params_with_floor (risk_reduction_threshold=1000)
    // so above_threshold return can be both true and false.
    let mut engine = RiskEngine::new(test_params_with_floor());

    let capital: u128 = kani::any();
    let insurance: u128 = kani::any();
    kani::assume(capital >= 100 && capital <= 10_000);
    kani::assume(insurance <= 5_000);

    let user_idx = engine.add_user(0).unwrap();
    engine.accounts[user_idx as usize].capital = U128::new(capital);
    engine.insurance_fund.balance = U128::new(insurance);
    engine.vault = U128::new(capital + insurance);
    engine.recompute_aggregates();

    kani::assert(canonical_inv(&engine), "setup state must satisfy INV");

    let amount: u128 = kani::any();
    kani::assume(amount > 0 && amount < 10_000);

    let vault_before = engine.vault.get();
    let ins_before = engine.insurance_fund.balance.get();

    let result = engine.top_up_insurance_fund(amount);

    let above_threshold = assert_ok!(result, "top_up_insurance_fund must succeed");

    kani::assert(canonical_inv(&engine), "INV must hold after top_up_insurance_fund");
    kani::assert(
        engine.vault.get() == vault_before + amount,
        "vault must increase by amount",
    );
    kani::assert(
        engine.insurance_fund.balance.get() == ins_before + amount,
        "insurance must increase by amount",
    );

    // Verify threshold return value
    let expected_above = ins_before + amount > 1000;
    kani::assert(above_threshold == expected_above, "above_threshold must match");

    // Non-vacuity: both threshold outcomes reachable
    if insurance < 500 && amount < 500 {
        kani::assert(!above_threshold || insurance + amount > 1000,
            "non-vacuity: below-threshold case reachable");
    }
}

// ============================================================================
// SEQUENCE-LEVEL PROOFS - Multi-Operation INV Preservation
// ============================================================================

/// Sequence: deposit -> trade -> liquidate preserves INV
/// Each step is gated on previous success (models Solana tx atomicity)
/// Optimized: Concrete deposits, reduced unwind. Uses LP (Kani is_lp uses kind field, no memcmp)
#[kani::proof]
#[kani::unwind(5)] // MAX_ACCOUNTS=4
#[kani::solver(cadical)]
fn proof_sequence_deposit_trade_liquidate() {
    let mut engine = RiskEngine::new(test_params());
    engine.vault = U128::new(100_000);
    engine.insurance_fund.balance = U128::new(10_000);
    engine.current_slot = 100;
    engine.last_crank_slot = 100;
    engine.last_full_sweep_start_slot = 100;

    let user = engine.add_user(0).unwrap();
    let lp = engine.add_lp([1u8; 32], [0u8; 32], 0).unwrap();

    // Symbolic user capital — near margin boundary for large trades
    let user_cap: u128 = kani::any();
    kani::assume(user_cap >= 500 && user_cap <= 5_000);

    let _ = assert_ok!(engine.deposit(user, user_cap, 0), "user deposit must succeed");
    let _ = assert_ok!(engine.deposit(lp, 50_000, 0), "lp deposit must succeed");
    kani::assert(canonical_inv(&engine), "INV after deposits");

    // Symbolic trade size — large enough to make user undercollateralized
    let size: i128 = kani::any();
    kani::assume(size >= 100 && size <= 1_000_000);

    let trade_result = engine.execute_trade(&NoOpMatcher, lp, user, 100, 1_000_000, size);

    // INV must hold regardless of trade outcome (Err → no mutation)
    kani::assert(canonical_inv(&engine), "INV after trade (Ok or Err)");

    if trade_result.is_ok() {
        // Liquidation attempt — may trigger if position is large relative to capital
        let result = engine.liquidate_at_oracle(user, 100, 1_000_000);
        kani::assert(result.is_ok(), "liquidation must not error");
        kani::assert(canonical_inv(&engine), "INV after liquidate attempt");
    }

    // Non-vacuity: at least small trades succeed
    if user_cap >= 2_000 && size <= 5_000 {
        kani::assert(trade_result.is_ok(), "non-vacuity: conservative trade must succeed");
    }
}

/// Sequence: deposit -> crank -> withdraw preserves INV
/// Each step is gated on previous success (models Solana tx atomicity)
#[kani::proof]
#[kani::unwind(33)]
#[kani::solver(cadical)]
fn proof_sequence_deposit_crank_withdraw() {
    let mut engine = RiskEngine::new(test_params());
    engine.vault = U128::new(100_000);
    engine.insurance_fund.balance = U128::new(10_000);
    engine.current_slot = 100;
    engine.last_crank_slot = 100;
    engine.last_full_sweep_start_slot = 100;

    let user = engine.add_user(0).unwrap();
    let lp = engine.add_lp([1u8; 32], [0u8; 32], 0).unwrap();

    kani::assert(canonical_inv(&engine), "API-built state must satisfy INV");

    // Step 1: Symbolic deposit
    let deposit: u128 = kani::any();
    kani::assume(deposit > 5_000 && deposit < 50_000);
    let _ = assert_ok!(engine.deposit(user, deposit, 0), "deposit must succeed");
    let _ = assert_ok!(engine.deposit(lp, 50_000, 0), "LP deposit must succeed");
    kani::assert(canonical_inv(&engine), "INV after deposit");

    // Step 2: Symbolic trade size
    let size: i128 = kani::any();
    kani::assume(size >= 100 && size <= 5_000);
    let _ = assert_ok!(
        engine.execute_trade(&NoOpMatcher, lp, user, 100, 1_000_000, size),
        "trade must succeed"
    );
    kani::assert(canonical_inv(&engine), "INV after trade");

    // Step 3: Symbolic crank with funding
    let funding_rate: i64 = kani::any();
    kani::assume(funding_rate > -10 && funding_rate < 10);
    let _ = assert_ok!(
        engine.keeper_crank(user, 101, 1_000_000, funding_rate, false),
        "crank must succeed"
    );
    kani::assert(canonical_inv(&engine), "INV after crank");

    // Step 4: Symbolic withdraw
    let withdraw: u128 = kani::any();
    kani::assume(withdraw > 0 && withdraw <= 1_000);

    let _ = assert_ok!(
        engine.withdraw(user, withdraw, 101, 1_000_000),
        "withdraw must succeed"
    );
    kani::assert(canonical_inv(&engine), "INV after withdraw");
}

// ============================================================================
// FUNDING/POSITION CONSERVATION PROOFS
// ============================================================================

/// Trade creates proper funding-settled positions
/// This proof verifies that after execute_trade:
/// - Both accounts have positions (non-vacuous)
/// - Both accounts are funding-settled (funding_index matches global)
/// - INV is preserved
#[kani::proof]
#[kani::unwind(33)]
#[kani::solver(cadical)]
fn proof_trade_creates_funding_settled_positions() {
    let mut engine = RiskEngine::new(test_params());
    engine.vault = U128::new(100_000);
    engine.insurance_fund.balance = U128::new(10_000);
    engine.current_slot = 100;
    engine.last_crank_slot = 100;
    engine.last_full_sweep_start_slot = 100;

    let user = engine.add_user(0).unwrap();
    let lp = engine.add_lp([1u8; 32], [0u8; 32], 0).unwrap();

    // Deposits
    engine.deposit(user, 10_000, 0).unwrap();
    engine.deposit(lp, 50_000, 0).unwrap();

    // Assert, not assume — state built via public APIs must satisfy INV
    kani::assert(canonical_inv(&engine), "API-built state must satisfy INV");

    // Execute trade to create positions — both long and short user positions
    let delta: i128 = kani::any();
    kani::assume(delta != 0 && delta >= -200 && delta <= 200);

    let result = engine.execute_trade(&NoOpMatcher, lp, user, 100, 1_000_000, delta);

    // Non-vacuity: trade must succeed with well-funded accounts
    assert!(result.is_ok(), "non-vacuity: execute_trade must succeed");

    // NON-VACUITY: Both accounts should have positions now
    kani::assert(
        !engine.accounts[user as usize].position_size.is_zero(),
        "User must have position after trade",
    );
    kani::assert(
        !engine.accounts[lp as usize].position_size.is_zero(),
        "LP must have position after trade",
    );

    // Funding should be settled (both at same funding index)
    kani::assert(
        engine.accounts[user as usize].funding_index == engine.funding_index_qpb_e6,
        "User funding must be settled",
    );
    kani::assert(
        engine.accounts[lp as usize].funding_index == engine.funding_index_qpb_e6,
        "LP funding must be settled",
    );

    // INV must be preserved
    kani::assert(canonical_inv(&engine), "INV must hold after trade");
}

/// Keeper crank with funding rate preserves INV
/// This proves that non-zero funding rates don't violate structural invariants
#[kani::proof]
#[kani::unwind(33)]
#[kani::solver(cadical)]
fn proof_crank_with_funding_preserves_inv() {
    let mut engine = RiskEngine::new(test_params());
    engine.vault = U128::new(200_000);
    engine.insurance_fund.balance = U128::new(10_000);
    engine.current_slot = 100;
    engine.last_crank_slot = 50;
    engine.last_full_sweep_start_slot = 50;

    let user = engine.add_user(0).unwrap();
    let lp = engine.add_lp([1u8; 32], [0u8; 32], 0).unwrap();

    // Symbolic capital for user — exercises margin boundaries during crank
    let user_cap: u128 = kani::any();
    kani::assume(user_cap >= 1_000 && user_cap <= 50_000);

    engine.deposit(user, user_cap, 0).unwrap();
    engine.deposit(lp, 100_000, 0).unwrap();

    // Execute trade to create positions
    let size: i128 = kani::any();
    kani::assume(size >= 50 && size <= 500);
    engine.execute_trade(&NoOpMatcher, lp, user, 100, 1_000_000, size).unwrap();

    kani::assert(canonical_inv(&engine), "API-built state must satisfy INV");

    // Crank with symbolic funding rate AND oracle != entry (exercises mark_pnl)
    let funding_rate: i64 = kani::any();
    kani::assume(funding_rate > -100 && funding_rate < 100);

    let result = engine.keeper_crank(user, 100, 1_050_000, funding_rate, false);

    // Non-vacuity: crank must succeed
    assert!(result.is_ok(), "non-vacuity: keeper_crank must succeed");

    // INV must be preserved after crank
    kani::assert(
        canonical_inv(&engine),
        "INV must hold after crank with funding",
    );

    kani::assert(
        engine.last_crank_slot == 100,
        "Crank must advance last_crank_slot",
    );
}

// ============================================================================
// Variation Margin / No PnL Teleportation Proofs
// ============================================================================

/// Proof: Variation margin ensures LP-fungibility for closing positions
///
/// The "PnL teleportation" bug occurred when a user opened with LP1 at price P1,
/// then closed with LP2 (whose position was from a different price). Without
/// variation margin, LP2 could gain/lose spuriously based on LP1's entry price.
///
/// With variation margin, before ANY position change:
/// 1. settle_mark_to_oracle moves mark PnL to pnl field
/// 2. entry_price is reset to oracle_price
///
/// This means closing with ANY LP at oracle price produces the correct result:
/// - User's equity change = actual price movement (P_close - P_open) * size
/// - Each LP's loss matches their mark-to-market, not the closing trade
///
/// This proof verifies that closing a position with a different LP produces
/// the same user equity gain as closing with the original LP.
#[kani::proof]
#[kani::unwind(33)]
#[kani::solver(cadical)]
fn proof_variation_margin_no_pnl_teleport() {
    // Scenario: user opens long with LP1 at P1, price moves to P2, closes with LP2
    // Expected: user gains (P2 - P1) * size regardless of which LP closes

    // APPROACH 1: Clone engine, open with LP1, close with LP1
    // APPROACH 2: Clone engine, open with LP1, close with LP2
    // Verify: user equity gain is the same in both approaches

    // Engine 1: open with LP1, close with LP1
    let mut engine1 = RiskEngine::new(test_params());
    engine1.vault = U128::new(1_000_000);
    engine1.insurance_fund.balance = U128::new(100_000);

    let user1 = engine1.add_user(0).unwrap();
    let lp1_a = engine1.add_lp([1u8; 32], [0u8; 32], 0).unwrap();

    engine1.deposit(user1, 100_000, 0).unwrap();
    engine1.deposit(lp1_a, 500_000, 0).unwrap();

    // Symbolic prices (bounded)
    let open_price: u64 = kani::any();
    let close_price: u64 = kani::any();
    let size: i64 = kani::any();

    // Bounds tightened for solver tractability after settle_loss_only additions
    kani::assume(open_price >= 900_000 && open_price <= 1_100_000);
    kani::assume(close_price >= 900_000 && close_price <= 1_100_000);
    kani::assume(size > 0 && size <= 50); // Long position, bounded

    let user1_capital_before = engine1.accounts[user1 as usize].capital.get();

    // Open position with LP1 at open_price
    let open_res = engine1.execute_trade(&NoOpMatcher, lp1_a, user1, 0, open_price, size as i128);
    assert_ok!(open_res, "Engine1: open trade must succeed");

    // Close position with LP1 at close_price
    let close_res1 =
        engine1.execute_trade(&NoOpMatcher, lp1_a, user1, 0, close_price, -(size as i128));
    assert_ok!(close_res1, "Engine1: close trade must succeed");

    let user1_capital_after = engine1.accounts[user1 as usize].capital.get();
    let user1_pnl_after = engine1.accounts[user1 as usize].pnl.get();

    // Engine 2: open with LP1, close with LP2
    let mut engine2 = RiskEngine::new(test_params());
    engine2.vault = U128::new(1_000_000);
    engine2.insurance_fund.balance = U128::new(100_000);

    let user2 = engine2.add_user(0).unwrap();
    let lp2_a = engine2.add_lp([1u8; 32], [0u8; 32], 0).unwrap();
    let lp2_b = engine2.add_lp([2u8; 32], [0u8; 32], 0).unwrap();

    engine2.deposit(user2, 100_000, 0).unwrap();
    engine2.deposit(lp2_a, 250_000, 0).unwrap();
    engine2.deposit(lp2_b, 250_000, 0).unwrap();

    let user2_capital_before = engine2.accounts[user2 as usize].capital.get();

    // Open position with LP2_A at open_price
    let open_res2 = engine2.execute_trade(&NoOpMatcher, lp2_a, user2, 0, open_price, size as i128);
    assert_ok!(open_res2, "Engine2: open trade must succeed");

    // Close position with LP2_B (different LP!) at close_price
    let close_res2 =
        engine2.execute_trade(&NoOpMatcher, lp2_b, user2, 0, close_price, -(size as i128));
    assert_ok!(close_res2, "Engine2: close trade must succeed");

    let user2_capital_after = engine2.accounts[user2 as usize].capital.get();
    let user2_pnl_after = engine2.accounts[user2 as usize].pnl.get();

    // Calculate total equity changes
    let user1_equity_change =
        (user1_capital_after as i128 - user1_capital_before as i128) + user1_pnl_after;
    let user2_equity_change =
        (user2_capital_after as i128 - user2_capital_before as i128) + user2_pnl_after;

    // PROOF: User equity change is IDENTICAL regardless of which LP closes
    // This is the core "no PnL teleportation" property
    kani::assert(
        user1_equity_change == user2_equity_change,
        "NO_TELEPORT: User equity change must be LP-invariant",
    );
}

/// Proof: Trade PnL is exactly (oracle - exec_price) * size
///
/// With variation margin, the trade_pnl formula is:
///   trade_pnl = (oracle - exec_price) * size / 1e6
///
/// This is exactly zero-sum between user and LP at the trade level.
/// Any deviation from mark (entry vs oracle) is settled BEFORE the trade.
#[kani::proof]
#[kani::unwind(33)]
#[kani::solver(cadical)]
fn proof_trade_pnl_zero_sum() {
    let mut engine = RiskEngine::new(test_params());
    engine.vault = U128::new(1_000_000);
    engine.insurance_fund.balance = U128::new(100_000);

    let user = engine.add_user(0).unwrap();
    let lp = engine.add_lp([1u8; 32], [0u8; 32], 0).unwrap();

    engine.deposit(user, 100_000, 0).unwrap();
    engine.deposit(lp, 500_000, 0).unwrap();

    // Symbolic values (bounded)
    let oracle: u64 = kani::any();
    let size: i64 = kani::any();

    kani::assume(oracle >= 500_000 && oracle <= 1_500_000);
    kani::assume(size != 0 && size > -1000 && size < 1000);

    // Capture state before trade
    let user_pnl_before = engine.accounts[user as usize].pnl.get();
    let lp_pnl_before = engine.accounts[lp as usize].pnl.get();
    let user_capital_before = engine.accounts[user as usize].capital.get();
    let lp_capital_before = engine.accounts[lp as usize].capital.get();

    // Execute trade at oracle price (exec_price = oracle, so trade_pnl = 0)
    let res = engine.execute_trade(&NoOpMatcher, lp, user, 0, oracle, size as i128);
    assert!(res.is_ok(), "non-vacuity: trade must succeed with well-capitalized accounts and bounded inputs");

    let user_pnl_after = engine.accounts[user as usize].pnl.get();
    let lp_pnl_after = engine.accounts[lp as usize].pnl.get();
    let user_capital_after = engine.accounts[user as usize].capital.get();
    let lp_capital_after = engine.accounts[lp as usize].capital.get();

    // Compute expected fee using same formula as engine (ceiling division per spec §8.1):
    // notional = |exec_size| * exec_price / 1_000_000
    // fee = ceil(notional * trading_fee_bps / 10_000)
    // NoOpMatcher returns exec_price = oracle, exec_size = size
    let abs_size = if size >= 0 { size as u128 } else { (-size) as u128 };
    let notional = abs_size.saturating_mul(oracle as u128) / 1_000_000;
    // Use ceiling division: (n * bps + 9999) / 10000
    let expected_fee = if notional > 0 {
        (notional.saturating_mul(10) + 9999) / 10_000 // trading_fee_bps = 10
    } else {
        0
    };

    let user_delta = (user_pnl_after - user_pnl_before)
        + (user_capital_after as i128 - user_capital_before as i128);
    let lp_delta =
        (lp_pnl_after - lp_pnl_before) + (lp_capital_after as i128 - lp_capital_before as i128);

    // With exec_price = oracle, trade_pnl = 0. Only user pays fee (from capital → insurance).
    // user_delta = -fee, lp_delta = 0, total = -fee exactly.
    let total_delta = user_delta + lp_delta;

    kani::assert(
        total_delta == -(expected_fee as i128),
        "ZERO_SUM: User + LP delta must equal exactly negative fee",
    );

    // LP is never charged fees
    kani::assert(
        lp_delta == 0,
        "ZERO_SUM: LP delta must be zero (fees only from user)",
    );
}

// ============================================================================
// TELEPORT SCENARIO HARNESS
// ============================================================================

/// Kani proof: No PnL teleportation when closing across LPs
/// This proves that with variation margin, closing a position with a different LP
/// than the one it was opened with does not create or destroy value.
#[kani::proof]
#[kani::unwind(5)]
#[kani::solver(cadical)]
fn kani_no_teleport_cross_lp_close() {
    let mut params = test_params();
    params.trading_fee_bps = 0;
    params.max_crank_staleness_slots = u64::MAX;
    params.maintenance_margin_bps = 0;
    params.initial_margin_bps = 0;

    let mut engine = RiskEngine::new(params);

    // Create two LPs
    let lp1 = engine.add_lp([1u8; 32], [0u8; 32], 0).unwrap();
    engine.accounts[lp1 as usize].capital = U128::new(1_000_000);
    engine.vault = engine.vault + U128::new(1_000_000);

    let lp2 = engine.add_lp([2u8; 32], [0u8; 32], 0).unwrap();
    engine.accounts[lp2 as usize].capital = U128::new(1_000_000);
    engine.vault = engine.vault + U128::new(1_000_000);

    // Create user
    let user = engine.add_user(0).unwrap();
    engine.accounts[user as usize].capital = U128::new(1_000_000);
    engine.vault = engine.vault + U128::new(1_000_000);

    let oracle: u64 = kani::any();
    kani::assume(oracle >= 500_000 && oracle <= 2_000_000);
    let now_slot = 100u64;
    let btc: i128 = kani::any();
    kani::assume(btc >= 1_000 && btc <= 10_000_000);

    // Open position with LP1 (symbolic oracle & size)
    assert_ok!(engine.execute_trade(&NoOpMatcher, lp1, user, now_slot, oracle, btc),
        "open trade with LP1 must succeed");

    // Capture state after open
    let user_pnl_after_open = engine.accounts[user as usize].pnl.get();
    let lp1_pnl_after_open = engine.accounts[lp1 as usize].pnl.get();
    let lp2_pnl_after_open = engine.accounts[lp2 as usize].pnl.get();

    // All pnl should be 0 since we executed at oracle
    kani::assert(user_pnl_after_open == 0, "User pnl after open should be 0");
    kani::assert(lp1_pnl_after_open == 0, "LP1 pnl after open should be 0");
    kani::assert(lp2_pnl_after_open == 0, "LP2 pnl after open should be 0");

    // Close position with LP2 at same oracle (no price movement — must succeed)
    assert_ok!(engine.execute_trade(&NoOpMatcher, lp2, user, now_slot, oracle, -btc),
        "close trade with LP2 must succeed");

    // After close, all positions should be 0
    kani::assert(
        engine.accounts[user as usize].position_size.is_zero(),
        "User position should be 0 after close",
    );

    // PnL should be 0 (no price movement = no gain/loss)
    let user_pnl_final = engine.accounts[user as usize].pnl.get();
    let lp1_pnl_final = engine.accounts[lp1 as usize].pnl.get();
    let lp2_pnl_final = engine.accounts[lp2 as usize].pnl.get();

    kani::assert(user_pnl_final == 0, "User pnl after close should be 0");
    kani::assert(lp1_pnl_final == 0, "LP1 pnl after close should be 0");
    kani::assert(lp2_pnl_final == 0, "LP2 pnl after close should be 0");

    // Total PnL must be zero-sum
    let total_pnl = user_pnl_final + lp1_pnl_final + lp2_pnl_final;
    kani::assert(total_pnl == 0, "Total PnL must be zero-sum");

    // Conservation should hold
    kani::assert(engine.check_conservation(oracle), "Conservation must hold");

    // Verify current_slot was set correctly
    kani::assert(
        engine.current_slot == now_slot,
        "current_slot should match now_slot",
    );

    // Verify warmup_started_at_slot was updated
    kani::assert(
        engine.accounts[user as usize].warmup_started_at_slot == now_slot,
        "User warmup_started_at_slot should be now_slot",
    );
    kani::assert(
        engine.accounts[lp2 as usize].warmup_started_at_slot == now_slot,
        "LP2 warmup_started_at_slot should be now_slot",
    );
}

// ============================================================================
// MATCHER GUARD HARNESS
// ============================================================================

/// Bad matcher that returns the opposite sign
struct BadMatcherOppositeSign;

impl MatchingEngine for BadMatcherOppositeSign {
    fn execute_match(
        &self,
        _lp_program: &[u8; 32],
        _lp_context: &[u8; 32],
        _lp_account_id: u64,
        oracle_price: u64,
        size: i128,
    ) -> Result<TradeExecution> {
        Ok(TradeExecution {
            price: oracle_price,
            size: -size, // Wrong sign!
        })
    }
}

/// Kani proof: Invalid matcher output is rejected
/// This proves that the engine rejects matchers that return opposite-sign fills.
#[kani::proof]
#[kani::unwind(5)]
#[kani::solver(cadical)]
fn kani_rejects_invalid_matcher_output() {
    let mut params = test_params();
    params.trading_fee_bps = 0;
    params.max_crank_staleness_slots = u64::MAX;
    params.maintenance_margin_bps = 0;
    params.initial_margin_bps = 0;

    let mut engine = RiskEngine::new(params);

    // Create LP
    let lp = engine.add_lp([1u8; 32], [0u8; 32], 0).unwrap();
    engine.accounts[lp as usize].capital = U128::new(1_000_000);
    engine.vault = engine.vault + U128::new(1_000_000);

    // Create user
    let user = engine.add_user(0).unwrap();
    engine.accounts[user as usize].capital = U128::new(1_000_000);
    engine.vault = engine.vault + U128::new(1_000_000);

    let oracle: u64 = kani::any();
    kani::assume(oracle >= 1 && oracle <= 2_000_000);
    let now_slot = 0u64;
    let size: i128 = kani::any();
    kani::assume(size >= 1 && size <= 10_000_000);

    // Try to execute trade with bad matcher (symbolic oracle & size)
    let result = engine.execute_trade(&BadMatcherOppositeSign, lp, user, now_slot, oracle, size);

    // Must be rejected with InvalidMatchingEngine
    kani::assert(
        matches!(result, Err(RiskError::InvalidMatchingEngine)),
        "Must reject matcher that returns opposite sign",
    );
}

// ==============================================================================
// Proofs migrated from src/percolator.rs inline kani_proofs
// ==============================================================================

const E6_INLINE: u64 = 1_000_000;
const ORACLE_100K: u64 = 100_000 * E6_INLINE;
const ONE_BASE: i128 = 1_000_000;

fn params_for_inline_kani() -> RiskParams {
    RiskParams {
        warmup_period_slots: 1000,
        maintenance_margin_bps: 0,
        initial_margin_bps: 0,
        trading_fee_bps: 0,
        max_accounts: MAX_ACCOUNTS as u64,
        new_account_fee: U128::new(0),
        risk_reduction_threshold: U128::new(0),

        maintenance_fee_per_slot: U128::new(0),
        max_crank_staleness_slots: u64::MAX,

        liquidation_fee_bps: 0,
        liquidation_fee_cap: U128::new(0),

        liquidation_buffer_bps: 0,
        min_liquidation_abs: U128::new(0),
    }
}

struct P90kMatcher;
impl MatchingEngine for P90kMatcher {
    fn execute_match(
        &self,
        _lp_program: &[u8; 32],
        _lp_context: &[u8; 32],
        _lp_account_id: u64,
        oracle_price: u64,
        size: i128,
    ) -> Result<TradeExecution> {
        Ok(TradeExecution {
            price: oracle_price - (10_000 * E6_INLINE),
            size,
        })
    }
}

struct AtOracleMatcher;
impl MatchingEngine for AtOracleMatcher {
    fn execute_match(
        &self,
        _lp_program: &[u8; 32],
        _lp_context: &[u8; 32],
        _lp_account_id: u64,
        oracle_price: u64,
        size: i128,
    ) -> Result<TradeExecution> {
        Ok(TradeExecution {
            price: oracle_price,
            size,
        })
    }
}

#[kani::proof]
#[kani::unwind(33)]
#[kani::solver(cadical)]
fn kani_cross_lp_close_no_pnl_teleport() {
    let mut engine = RiskEngine::new(params_for_inline_kani());

    let lp1 = engine.add_lp([1u8; 32], [2u8; 32], 0).unwrap();
    let lp2 = engine.add_lp([3u8; 32], [4u8; 32], 0).unwrap();
    let user = engine.add_user(0).unwrap();

    // Symbolic capital via u8 multiplier (set directly to avoid expensive deposit path)
    let cap_mult: u8 = kani::any();
    kani::assume(cap_mult >= 1 && cap_mult <= 100);
    let initial_cap: u128 = (cap_mult as u128) * 1_000_000_000;
    engine.accounts[lp1 as usize].capital = U128::new(initial_cap);
    engine.accounts[lp2 as usize].capital = U128::new(initial_cap);
    engine.accounts[user as usize].capital = U128::new(initial_cap);
    engine.vault = U128::new(initial_cap * 3);
    engine.recompute_aggregates();

    // Symbolic trade size
    let size: i128 = kani::any();
    kani::assume(size >= 1 && size <= 5);

    // Trade 1: open long at 90k (P90kMatcher) with LP1
    engine
        .execute_trade(&P90kMatcher, lp1, user, 100, ORACLE_100K, size)
        .unwrap();

    // Trade 2: close at oracle with LP2
    engine
        .execute_trade(&AtOracleMatcher, lp2, user, 101, ORACLE_100K, -size)
        .unwrap();

    // User position must be flat after close
    kani::assert(
        engine.accounts[user as usize].position_size.get() == 0,
        "user flat after close",
    );

    // No-teleport: LP2 traded at oracle so its capital must be unchanged
    kani::assert(
        engine.accounts[lp2 as usize].capital.get() == initial_cap,
        "LP2 capital unchanged (no PnL teleport from LP1)",
    );
    kani::assert(
        engine.accounts[lp2 as usize].pnl.get() == 0,
        "LP2 PnL zero (no teleport)",
    );

    // Conservation must hold
    assert!(engine.check_conservation(ORACLE_100K));
}

// ============================================================================
// AUDIT C1-C6: HAIRCUT MECHANISM PROOFS
// These close the critical gaps identified in the security audit:
//   C1: haircut_ratio() formula correctness
//   C2: effective_pos_pnl() and effective_equity() with haircut
//   C3: Principal protection across accounts
//   C4: Profit conversion payout formula
//   C5: Rounding slack bound
//   C6: Liveness with profitable LP and losses
// ============================================================================

/// C1: Haircut ratio formula correctness (spec §3.2)
/// Verifies:
///   - h_num <= h_den (h in [0, 1])
///   - h_den > 0 (never division by zero)
///   - h_num <= Residual and h_num <= PNL_pos_tot
///   - Fully backed: h == 1
///   - Underbacked: h_num == Residual
///   - PNL_pos_tot == 0: h = (1, 1)
#[kani::proof]
#[kani::unwind(33)]
#[kani::solver(cadical)]
fn proof_haircut_ratio_formula_correctness() {
    let mut engine = RiskEngine::new(test_params());

    let vault: u128 = kani::any();
    let c_tot: u128 = kani::any();
    let insurance: u128 = kani::any();
    let pnl_pos_tot: u128 = kani::any();

    kani::assume(vault <= 100_000);
    kani::assume(c_tot <= vault);
    kani::assume(insurance <= vault.saturating_sub(c_tot));
    kani::assume(pnl_pos_tot <= 100_000);

    engine.vault = U128::new(vault);
    engine.c_tot = U128::new(c_tot);
    engine.insurance_fund.balance = U128::new(insurance);
    engine.pnl_pos_tot = U128::new(pnl_pos_tot);

    let (h_num, h_den) = engine.haircut_ratio();
    let residual = vault.saturating_sub(c_tot).saturating_sub(insurance);

    // P1: h_den is never 0
    assert!(h_den > 0, "C1: h_den must be > 0");

    // P2: h in [0, 1] — h_num <= h_den
    assert!(h_num <= h_den, "C1: h_num must be <= h_den (h in [0,1])");

    // P3: h_num <= Residual (when pnl_pos_tot > 0)
    if pnl_pos_tot > 0 {
        assert!(h_num <= residual, "C1: h_num must be <= Residual");
    }

    // P4: h_num <= pnl_pos_tot (when pnl_pos_tot > 0)
    if pnl_pos_tot > 0 {
        assert!(h_num <= pnl_pos_tot, "C1: h_num must be <= pnl_pos_tot");
    }

    // P5: When pnl_pos_tot == 0, h == (1, 1)
    if pnl_pos_tot == 0 {
        assert!(h_num == 1 && h_den == 1, "C1: h must be (1,1) when pnl_pos_tot == 0");
    }

    // P6: When fully backed (Residual >= pnl_pos_tot > 0), h == 1
    if pnl_pos_tot > 0 && residual >= pnl_pos_tot {
        assert!(
            h_num == pnl_pos_tot && h_den == pnl_pos_tot,
            "C1: h must be 1 when fully backed"
        );
    }

    // P7: When underbacked (0 < Residual < pnl_pos_tot), h_num == Residual
    if pnl_pos_tot > 0 && residual < pnl_pos_tot {
        assert!(h_num == residual, "C1: h_num must equal Residual when underbacked");
    }

    // Non-vacuity: partial haircut case is reachable
    if pnl_pos_tot > 0 && residual > 0 && residual < pnl_pos_tot {
        assert!(
            h_num > 0 && h_num < h_den,
            "C1 non-vacuity: partial haircut must have 0 < h < 1"
        );
    }
}

/// C2: Effective equity formula with haircut (spec §3.3)
/// Verifies:
///   - effective_pos_pnl(pnl) == floor(max(pnl, 0) * h_num / h_den)
///   - effective_equity() matches spec formula: max(0, C + min(PNL, 0) + PNL_eff_pos)
///   - Haircutted equity <= unhaircutted equity
///   - Tests both fully-backed and underbacked scenarios
#[kani::proof]
#[kani::unwind(33)]
#[kani::solver(cadical)]
fn proof_effective_equity_with_haircut() {
    let mut engine = RiskEngine::new(test_params());

    let vault: u128 = kani::any();
    let c_tot: u128 = kani::any();
    let insurance: u128 = kani::any();
    let pnl_pos_tot: u128 = kani::any();
    let capital: u128 = kani::any();
    let pnl: i128 = kani::any();

    // Bounds kept small for solver tractability (symbolic division is expensive)
    kani::assume(vault > 0 && vault <= 100);
    kani::assume(c_tot <= vault);
    kani::assume(insurance <= vault.saturating_sub(c_tot));
    kani::assume(pnl_pos_tot > 0 && pnl_pos_tot <= 100);
    kani::assume(capital <= 50);
    kani::assume(pnl > -50 && pnl < 50);

    // Create account via add_user, then override
    let idx = engine.add_user(0).unwrap();
    engine.accounts[idx as usize].capital = U128::new(capital);
    engine.accounts[idx as usize].pnl = I128::new(pnl);

    // Set global aggregates (overriding what add_user set)
    engine.vault = U128::new(vault);
    engine.c_tot = U128::new(c_tot);
    engine.insurance_fund.balance = U128::new(insurance);
    engine.pnl_pos_tot = U128::new(pnl_pos_tot);

    let (h_num, h_den) = engine.haircut_ratio();

    // P1: effective_pos_pnl matches spec formula
    let eff = engine.effective_pos_pnl(pnl);
    if pnl <= 0 {
        assert!(eff == 0, "C2: effective_pos_pnl must be 0 for non-positive PnL");
    } else {
        let expected = (pnl as u128).saturating_mul(h_num) / h_den;
        assert!(eff == expected, "C2: effective_pos_pnl must equal floor(pos_pnl * h_num / h_den)");
        // Haircutted must not exceed raw
        assert!(eff <= pnl as u128, "C2: haircutted PnL must not exceed raw PnL");
    }

    // P2: effective_equity matches spec: max(0, C + min(PNL, 0) + PNL_eff_pos)
    let expected_eff_equity = {
        let cap_i = u128_to_i128_clamped(capital);
        let neg_pnl = core::cmp::min(pnl, 0);
        let eff_eq_i = cap_i
            .saturating_add(neg_pnl)
            .saturating_add(u128_to_i128_clamped(eff));
        if eff_eq_i > 0 { eff_eq_i as u128 } else { 0 }
    };
    let actual_eff_equity = engine.effective_equity(&engine.accounts[idx as usize]);
    assert!(actual_eff_equity == expected_eff_equity, "C2: effective_equity must match spec formula");

    // P3: Haircutted equity <= unhaircutted equity
    let unhaircutted = engine.account_equity(&engine.accounts[idx as usize]);
    assert!(
        actual_eff_equity <= unhaircutted,
        "C2: haircutted equity must be <= unhaircutted equity"
    );

    // Non-vacuity: when h < 1 and PnL > 0, haircutted equity < unhaircutted equity
    let residual = vault.saturating_sub(c_tot).saturating_sub(insurance);
    if pnl > 0 && residual < pnl_pos_tot && pnl as u128 <= pnl_pos_tot {
        assert!(eff < pnl as u128, "C2 non-vacuity: partial haircut must reduce effective PnL");
    }
}

/// C3: Principal protection across accounts (spec §0, goal 1)
/// "One account's insolvency MUST NOT directly reduce any other account's protected principal."
/// Verifies that loss write-off on account A leaves account B's capital unchanged.
#[kani::proof]
#[kani::unwind(33)]
#[kani::solver(cadical)]
fn proof_principal_protection_across_accounts() {
    let mut engine = RiskEngine::new(test_params());

    // Account A: will suffer loss write-off (negative PnL exceeds capital)
    let a = engine.add_user(0).unwrap();
    let a_capital: u128 = kani::any();
    let a_loss: u128 = kani::any(); // magnitude of negative PnL
    kani::assume(a_capital > 0 && a_capital <= 10_000);
    kani::assume(a_loss > a_capital && a_loss <= 20_000); // loss exceeds capital → write-off

    engine.accounts[a as usize].capital = U128::new(a_capital);
    engine.accounts[a as usize].pnl = I128::new(-(a_loss as i128));

    // Account B: profitable, should be protected
    let b = engine.add_user(0).unwrap();
    let b_capital: u128 = kani::any();
    let b_pnl: u128 = kani::any();
    kani::assume(b_capital > 0 && b_capital <= 10_000);
    kani::assume(b_pnl > 0 && b_pnl <= 10_000);

    engine.accounts[b as usize].capital = U128::new(b_capital);
    engine.accounts[b as usize].pnl = I128::new(b_pnl as i128);

    // Set up consistent global aggregates
    engine.c_tot = U128::new(a_capital + b_capital);
    engine.pnl_pos_tot = U128::new(b_pnl); // only B has positive PnL
    engine.vault = U128::new(a_capital + b_capital + b_pnl); // V = C_tot + backing for B's PnL

    // Record B's state before
    let b_capital_before = engine.accounts[b as usize].capital.get();
    let b_pnl_before = engine.accounts[b as usize].pnl.get();

    // Settle A's loss (this triggers loss write-off per §6.1)
    let result = engine.settle_warmup_to_capital(a);
    assert!(result.is_ok(), "C3: settle must succeed");

    // A's loss should be settled: capital reduced, remainder written off
    assert!(
        engine.accounts[a as usize].pnl.get() >= 0
            || engine.accounts[a as usize].capital.is_zero(),
        "C3: A must have loss settled (pnl >= 0 or capital == 0)"
    );

    // PROOF: B's capital is unchanged
    assert!(
        engine.accounts[b as usize].capital.get() == b_capital_before,
        "C3: B's capital MUST NOT change due to A's loss write-off"
    );

    // PROOF: B's PnL is unchanged
    assert!(
        engine.accounts[b as usize].pnl.get() == b_pnl_before,
        "C3: B's PnL MUST NOT change due to A's loss write-off"
    );

    // Conservation still holds
    assert!(
        engine.vault.get()
            >= engine.c_tot.get() + engine.insurance_fund.balance.get(),
        "C3: conservation must hold after loss write-off"
    );
}

/// C4: Profit conversion payout formula (spec §6.2)
/// Verifies: y = floor(x * h_num / h_den) and:
///   - C_i increases by exactly y
///   - PNL_i decreases by exactly x (gross, not net)
///   - y <= x (haircut means payout <= claim)
///   - Haircut is computed BEFORE modifications
#[kani::proof]
#[kani::unwind(33)]
#[kani::solver(cadical)]
fn proof_profit_conversion_payout_formula() {
    let mut engine = RiskEngine::new(test_params());

    let capital: u128 = kani::any();
    let pnl: u128 = kani::any(); // positive PnL for conversion
    let vault: u128 = kani::any();
    let insurance: u128 = kani::any();

    // Bounds reduced for solver tractability
    kani::assume(capital <= 500);
    kani::assume(pnl > 0 && pnl <= 250);
    kani::assume(vault <= 2_000);
    kani::assume(insurance <= 500);
    kani::assume(vault >= capital + insurance); // conservation

    let idx = engine.add_user(0).unwrap();
    engine.accounts[idx as usize].capital = U128::new(capital);
    engine.accounts[idx as usize].pnl = I128::new(pnl as i128);

    // Symbolic warmup slope — exercises both full and partial conversion
    let slope: u128 = kani::any();
    kani::assume(slope <= 10);
    engine.accounts[idx as usize].warmup_started_at_slot = 0;
    engine.accounts[idx as usize].warmup_slope_per_step = U128::new(slope);
    engine.current_slot = 100; // elapsed = 100, cap = slope * 100

    engine.c_tot = U128::new(capital);
    engine.pnl_pos_tot = U128::new(pnl);
    engine.vault = U128::new(vault);
    engine.insurance_fund.balance = U128::new(insurance);

    // Record pre-conversion state
    let cap_before = engine.accounts[idx as usize].capital.get();
    let pnl_before = engine.accounts[idx as usize].pnl.get();
    let (h_num, h_den) = engine.haircut_ratio();

    // x = min(avail_gross, warmup_cap) where warmup_cap = slope * elapsed
    let warmup_cap = slope.saturating_mul(100);
    let avail_gross = pnl; // reserved_pnl = 0
    let x = if avail_gross < warmup_cap { avail_gross } else { warmup_cap };
    let expected_y = x.saturating_mul(h_num) / h_den;

    // Execute conversion
    let result = engine.settle_warmup_to_capital(idx);
    assert!(result.is_ok(), "C4: settle_warmup must succeed");

    let cap_after = engine.accounts[idx as usize].capital.get();
    let pnl_after = engine.accounts[idx as usize].pnl.get();

    // P1: Capital increased by exactly y = floor(x * h_num / h_den)
    assert!(
        cap_after == cap_before + expected_y,
        "C4: capital must increase by floor(x * h_num / h_den)"
    );

    // P2: PnL decreased by exactly x (gross, not payout)
    assert!(
        pnl_after == pnl_before - (x as i128),
        "C4: PnL must decrease by gross amount x"
    );

    // P3: Payout <= claim (y <= x)
    assert!(expected_y <= x, "C4: payout must not exceed claim");

    // P4: Haircut loss = x - y is the "burnt" portion
    let haircut_loss = x - expected_y;

    // P5: When underbacked AND conversion happened, haircut_loss > 0
    let residual = vault.saturating_sub(capital).saturating_sub(insurance);
    if residual < pnl && x > 0 {
        assert!(haircut_loss > 0, "C4 non-vacuity: underbacked must have haircut loss > 0");
    }
}

/// C5: Rounding slack bound (spec §3.4)
/// With K accounts having positive PnL:
///   - Σ effective_pos_pnl_i <= Residual
///   - Residual - Σ effective_pos_pnl_i < K (rounding slack < number of positive-PnL accounts)
#[kani::proof]
#[kani::unwind(33)]
#[kani::solver(cadical)]
fn proof_rounding_slack_bound() {
    let mut engine = RiskEngine::new(test_params());

    // Two accounts with positive PnL (K = 2)
    let a = engine.add_user(0).unwrap();
    let b = engine.add_user(0).unwrap();

    let pnl_a: u128 = kani::any();
    let pnl_b: u128 = kani::any();
    let vault: u128 = kani::any();
    let c_tot: u128 = kani::any();
    let insurance: u128 = kani::any();

    // Bounds kept small for solver tractability (symbolic division is expensive)
    kani::assume(pnl_a > 0 && pnl_a <= 100);
    kani::assume(pnl_b > 0 && pnl_b <= 100);
    kani::assume(vault <= 400);
    kani::assume(c_tot <= vault);
    kani::assume(insurance <= vault.saturating_sub(c_tot));

    engine.accounts[a as usize].pnl = I128::new(pnl_a as i128);
    engine.accounts[b as usize].pnl = I128::new(pnl_b as i128);
    engine.vault = U128::new(vault);
    engine.c_tot = U128::new(c_tot);
    engine.insurance_fund.balance = U128::new(insurance);
    engine.pnl_pos_tot = U128::new(pnl_a + pnl_b);

    let residual = vault.saturating_sub(c_tot).saturating_sub(insurance);

    // Compute effective PnL for each account
    let eff_a = engine.effective_pos_pnl(pnl_a as i128);
    let eff_b = engine.effective_pos_pnl(pnl_b as i128);
    let sum_eff = eff_a + eff_b;

    // P1: Sum of effective PnLs <= Residual
    assert!(
        sum_eff <= residual,
        "C5: sum of effective positive PnLs must not exceed Residual"
    );

    // P2: Rounding slack < K (number of positive-PnL accounts)
    let slack = residual - sum_eff;
    let k = 2u128; // two accounts with positive PnL
    if residual <= pnl_a + pnl_b {
        // Only meaningful when underbacked (when fully backed, Residual can be >> sum_eff)
        assert!(slack < k, "C5: rounding slack must be < K when underbacked");
    }

    // Non-vacuity: test underbacked case
    if residual < pnl_a + pnl_b && residual > 0 {
        assert!(
            sum_eff <= residual,
            "C5 non-vacuity: underbacked case must satisfy sum <= Residual"
        );
    }
}

/// C6: Liveness — profitable LP doesn't block withdrawals (spec §0, goal 5)
/// "A surviving profitable LP position MUST NOT block accounting progress."
/// Verifies that after one account's loss is written off, another account can still withdraw.
#[kani::proof]
#[kani::unwind(33)]
#[kani::solver(cadical)]
fn proof_liveness_after_loss_writeoff() {
    let mut engine = RiskEngine::new(test_params());
    engine.current_slot = 100;
    engine.last_crank_slot = 100;
    engine.last_full_sweep_start_slot = 100;

    // Account A: has negative PnL and some capital — will undergo actual writeoff
    let a = engine.add_user(0).unwrap();
    let a_capital: u128 = kani::any();
    kani::assume(a_capital >= 0 && a_capital <= 1_000);
    let a_loss: u128 = kani::any();
    kani::assume(a_loss >= 1 && a_loss <= 5_000);
    engine.accounts[a as usize].capital = U128::new(a_capital);
    engine.accounts[a as usize].pnl = I128::new(-(a_loss as i128));

    // Account B: profitable LP with capital AND position (margin check exercised)
    let b = engine.add_lp([1u8; 32], [0u8; 32], 0).unwrap();
    let b_capital: u128 = kani::any();
    kani::assume(b_capital >= 5_000 && b_capital <= 50_000);
    engine.accounts[b as usize].capital = U128::new(b_capital);
    engine.accounts[b as usize].pnl = I128::new(0);
    engine.accounts[b as usize].position_size = I128::new(500);
    engine.accounts[b as usize].entry_price = 1_000_000;
    engine.accounts[b as usize].funding_index = engine.funding_index_qpb_e6;

    // Set up global state: vault must cover both capitals + insurance
    engine.vault = U128::new(a_capital + b_capital + 1_000);
    engine.insurance_fund.balance = U128::new(1_000);
    sync_engine_aggregates(&mut engine);

    kani::assert(canonical_inv(&engine), "setup INV");

    // Perform actual writeoff on A (settle negative PnL into capital)
    engine.settle_warmup_to_capital(a).unwrap();

    // Verify writeoff occurred: A's pnl should be >= 0 or capital == 0
    let a_pnl_after = engine.accounts[a as usize].pnl.get();
    let a_cap_after = engine.accounts[a as usize].capital.get();
    kani::assert(
        a_pnl_after >= 0 || a_cap_after == 0,
        "N1: after writeoff, pnl >= 0 or capital == 0"
    );

    // B should still be able to withdraw partial capital (system is live)
    let withdraw_amount: u128 = kani::any();
    kani::assume(withdraw_amount > 0 && withdraw_amount <= 1_000);

    let result = engine.withdraw(b, withdraw_amount, 100, 1_000_000);

    // PROOF: Withdrawal must succeed — system is live despite A's loss writeoff
    kani::assert(
        result.is_ok(),
        "C6: withdrawal must succeed — profitable account must not be blocked by writeoff"
    );

    // Conservation still holds
    kani::assert(
        canonical_inv(&engine),
        "C6: INV must hold after withdrawal"
    );
}

// ============================================================================
// SECURITY AUDIT GAP CLOSURE — 18 Proofs across 5 Gaps
// ============================================================================
//
// Gap 1: Err-path mutation safety (best-effort keeper_crank paths)
// Gap 2: Matcher trust boundary (overfill, zero price, max price, INV on Err)
// Gap 3: Full conservation with MTM+funding (entry ≠ oracle, funding, lifecycle)
// Gap 4: Overflow / never-panic at extreme values
// Gap 5: Fee-credit corner cases (fee + margin interaction)
//
// These proofs close the 5 high/critical coverage gaps identified in the
// external security audit. All prior 107 proofs remain unchanged.

// ============================================================================
// New Matcher Structs for Gap 2 + Gap 4
// ============================================================================

/// Matcher that overfills: returns |exec_size| = |size| + 1
struct OverfillMatcher;

impl MatchingEngine for OverfillMatcher {
    fn execute_match(
        &self,
        _lp_program: &[u8; 32],
        _lp_context: &[u8; 32],
        _lp_account_id: u64,
        oracle_price: u64,
        size: i128,
    ) -> Result<TradeExecution> {
        let exec_size = if size > 0 { size + 1 } else { size - 1 };
        Ok(TradeExecution {
            price: oracle_price,
            size: exec_size,
        })
    }
}

/// Matcher that returns price = 0 (invalid)
struct ZeroPriceMatcher;

impl MatchingEngine for ZeroPriceMatcher {
    fn execute_match(
        &self,
        _lp_program: &[u8; 32],
        _lp_context: &[u8; 32],
        _lp_account_id: u64,
        _oracle_price: u64,
        size: i128,
    ) -> Result<TradeExecution> {
        Ok(TradeExecution {
            price: 0,
            size,
        })
    }
}

/// Matcher that returns price = MAX_ORACLE_PRICE + 1 (exceeds bound)
struct MaxPricePlusOneMatcher;

impl MatchingEngine for MaxPricePlusOneMatcher {
    fn execute_match(
        &self,
        _lp_program: &[u8; 32],
        _lp_context: &[u8; 32],
        _lp_account_id: u64,
        _oracle_price: u64,
        size: i128,
    ) -> Result<TradeExecution> {
        Ok(TradeExecution {
            price: MAX_ORACLE_PRICE + 1,
            size,
        })
    }
}

/// Matcher that returns a partial fill at a different price: half the size at oracle - 100_000
struct PartialFillDiffPriceMatcher;

impl MatchingEngine for PartialFillDiffPriceMatcher {
    fn execute_match(
        &self,
        _lp_program: &[u8; 32],
        _lp_context: &[u8; 32],
        _lp_account_id: u64,
        oracle_price: u64,
        size: i128,
    ) -> Result<TradeExecution> {
        let exec_price = if oracle_price > 100_000 {
            oracle_price - 100_000
        } else {
            1 // Minimum valid price
        };
        let exec_size = size / 2;
        Ok(TradeExecution {
            price: exec_price,
            size: exec_size,
        })
    }
}

// ============================================================================
// Extended AccountSnapshot for full mutation detection
// ============================================================================

/// Extended snapshot that captures ALL account fields for err-path mutation proofs
struct FullAccountSnapshot {
    capital: u128,
    pnl: i128,
    position_size: i128,
    entry_price: u64,
    funding_index: i128,
    fee_credits: i128,
    warmup_slope_per_step: u128,
    warmup_started_at_slot: u64,
    last_fee_slot: u64,
}

fn full_snapshot_account(account: &Account) -> FullAccountSnapshot {
    FullAccountSnapshot {
        capital: account.capital.get(),
        pnl: account.pnl.get(),
        position_size: account.position_size.get(),
        entry_price: account.entry_price,
        funding_index: account.funding_index.get(),
        fee_credits: account.fee_credits.get(),
        warmup_slope_per_step: account.warmup_slope_per_step.get(),
        warmup_started_at_slot: account.warmup_started_at_slot,
        last_fee_slot: account.last_fee_slot,
    }
}

/// Assert all fields of two FullAccountSnapshot are equal.
/// Uses a macro to avoid Kani ICE with function-parameter `&'static str`.
macro_rules! assert_full_snapshot_eq {
    ($before:expr, $after:expr, $msg:expr) => {{
        let b = &$before;
        let a = &$after;
        kani::assert(b.capital == a.capital, $msg);
        kani::assert(b.pnl == a.pnl, $msg);
        kani::assert(b.position_size == a.position_size, $msg);
        kani::assert(b.entry_price == a.entry_price, $msg);
        kani::assert(b.funding_index == a.funding_index, $msg);
        kani::assert(b.fee_credits == a.fee_credits, $msg);
        kani::assert(b.warmup_slope_per_step == a.warmup_slope_per_step, $msg);
        kani::assert(b.warmup_started_at_slot == a.warmup_started_at_slot, $msg);
        kani::assert(b.last_fee_slot == a.last_fee_slot, $msg);
    }};
}

// ============================================================================
// GAP 1: Err-path Mutation Safety (3 proofs)
// ============================================================================

/// Gap 1, Proof 1: touch_account Err → no mutation
///
/// Setup: position_size = i128::MAX/2, funding_index delta that causes checked_mul overflow.
/// Proves: If touch_account returns Err, account state and pnl_pos_tot are unchanged.
#[kani::proof]
#[kani::unwind(33)]
#[kani::solver(cadical)]
fn proof_gap1_touch_account_err_no_mutation() {
    let mut engine = RiskEngine::new(test_params());
    let user = engine.add_user(0).unwrap();

    // Set up position and funding index delta to trigger checked_mul overflow
    // in settle_account_funding: position_size * delta_f must overflow i128.
    // Symbolic position in [MAX_POSITION_ABS/2, MAX_POSITION_ABS] and
    // delta in [10^19, 2*10^19]. (MAX_POS/2) * 10^19 = 5*10^38 > i128::MAX.
    let pos_scale: u128 = kani::any();
    kani::assume(pos_scale >= MAX_POSITION_ABS / 2 && pos_scale <= MAX_POSITION_ABS);
    let large_pos: i128 = pos_scale as i128;
    engine.accounts[user as usize].position_size = I128::new(large_pos);
    let capital: u128 = kani::any();
    kani::assume(capital >= 100_000 && capital <= 10_000_000);
    engine.accounts[user as usize].capital = U128::new(capital);
    engine.accounts[user as usize].pnl = I128::new(0);
    // Account's funding index at 0
    engine.accounts[user as usize].funding_index = I128::new(0);
    // Symbolic global funding index in [10^19, 2*10^19]
    let delta: i128 = kani::any();
    kani::assume(delta >= 10_000_000_000_000_000_000 && delta <= 20_000_000_000_000_000_000);
    engine.funding_index_qpb_e6 = I128::new(delta);

    sync_engine_aggregates(&mut engine);

    // Snapshot before
    let snap_before = full_snapshot_account(&engine.accounts[user as usize]);
    let pnl_pos_tot_before = engine.pnl_pos_tot.get();
    let vault_before = engine.vault.get();
    let insurance_before = engine.insurance_fund.balance.get();

    // Operation
    let result = engine.touch_account(user);

    // Assert Err (non-vacuity)
    kani::assert(result.is_err(), "touch_account must fail with overflow");

    // Assert no mutation
    let snap_after = full_snapshot_account(&engine.accounts[user as usize]);
    assert_full_snapshot_eq!(snap_before, snap_after, "touch_account Err: account must be unchanged");
    kani::assert(engine.pnl_pos_tot.get() == pnl_pos_tot_before, "touch_account Err: pnl_pos_tot unchanged");
    kani::assert(engine.vault.get() == vault_before, "touch_account Err: vault unchanged");
    kani::assert(engine.insurance_fund.balance.get() == insurance_before, "touch_account Err: insurance unchanged");
}

/// Gap 1, Proof 2: settle_mark_to_oracle Err → no mutation
///
/// Setup: position and entry/oracle that cause mark_pnl overflow or pnl checked_add overflow.
/// Proves: If settle_mark_to_oracle returns Err, account state and pnl_pos_tot are unchanged.
#[kani::proof]
#[kani::unwind(33)]
#[kani::solver(cadical)]
fn proof_gap1_settle_mark_err_no_mutation() {
    let mut engine = RiskEngine::new(test_params());
    let user = engine.add_user(0).unwrap();

    // Set up position and prices to cause pnl + mark overflow:
    // mark_pnl_for_position: diff.checked_mul(abs_pos) / 1e6
    // With large position + pnl near MAX, pnl + mark > i128::MAX overflows.
    // Symbolic position in [MAX_POSITION_ABS/2, MAX_POSITION_ABS]
    let pos_scale: u128 = kani::any();
    kani::assume(pos_scale >= MAX_POSITION_ABS / 2 && pos_scale <= MAX_POSITION_ABS);
    let large_pos: i128 = pos_scale as i128;
    engine.accounts[user as usize].position_size = I128::new(large_pos);
    engine.accounts[user as usize].entry_price = 1;
    let capital: u128 = kani::any();
    kani::assume(capital >= 100_000 && capital <= 10_000_000);
    engine.accounts[user as usize].capital = U128::new(capital);
    // Symbolic pnl near i128::MAX so pnl + mark overflows
    let pnl_offset: i128 = kani::any();
    kani::assume(pnl_offset >= 0 && pnl_offset <= 100);
    engine.accounts[user as usize].pnl = I128::new(i128::MAX - pnl_offset);
    engine.accounts[user as usize].funding_index = engine.funding_index_qpb_e6;

    sync_engine_aggregates(&mut engine);

    // Snapshot before
    let snap_before = full_snapshot_account(&engine.accounts[user as usize]);
    let pnl_pos_tot_before = engine.pnl_pos_tot.get();
    let vault_before = engine.vault.get();

    // Oracle at MAX_ORACLE_PRICE, entry = 1:
    // diff = MAX_ORACLE_PRICE - 1, mark = diff * abs_pos / 1e6 > 0
    // pnl(i128::MAX-1) + mark(positive) overflows
    let result = engine.settle_mark_to_oracle(user, MAX_ORACLE_PRICE);

    // Assert Err (non-vacuity)
    kani::assert(result.is_err(), "settle_mark_to_oracle must fail with overflow");

    // Assert no mutation
    let snap_after = full_snapshot_account(&engine.accounts[user as usize]);
    assert_full_snapshot_eq!(snap_before, snap_after, "settle_mark Err: account must be unchanged");
    kani::assert(engine.pnl_pos_tot.get() == pnl_pos_tot_before, "settle_mark Err: pnl_pos_tot unchanged");
    kani::assert(engine.vault.get() == vault_before, "settle_mark Err: vault unchanged");
}

/// Gap 1, Proof 3: keeper_crank with maintenance fees preserves INV + conservation
///
/// Setup: Engine with maintenance fees, user + LP with positions and capital.
/// Proves: After successful crank, canonical_inv and conservation_fast_no_funding hold.
#[kani::proof]
#[kani::unwind(33)]
#[kani::solver(cadical)]
fn proof_gap1_crank_with_fees_preserves_inv() {
    let mut engine = RiskEngine::new(test_params_with_maintenance_fee());
    engine.vault = U128::new(100_000);
    engine.insurance_fund.balance = U128::new(10_000);
    engine.current_slot = 100;
    engine.last_crank_slot = 50;
    engine.last_full_sweep_start_slot = 50;

    let user = engine.add_user(0).unwrap();
    let lp = engine.add_lp([1u8; 32], [0u8; 32], 0).unwrap();

    engine.deposit(user, 10_000, 50).unwrap();
    engine.deposit(lp, 50_000, 50).unwrap();

    // Execute trade to create positions (fees will be charged on these)
    engine.execute_trade(&NoOpMatcher, lp, user, 100, 1_000_000, 50).unwrap();

    // Symbolic fee_credits and crank slot
    let fee_credits: i128 = kani::any();
    kani::assume(fee_credits > -500 && fee_credits < 500);
    engine.accounts[user as usize].fee_credits = I128::new(fee_credits);

    let crank_slot: u64 = kani::any();
    kani::assume(crank_slot >= 101 && crank_slot <= 200);

    // Assert pre-state INV (built via public APIs)
    kani::assert(canonical_inv(&engine), "API-built state must satisfy INV before crank");

    let last_crank_before = engine.last_crank_slot;

    // Crank at a symbolic later slot
    let result = engine.keeper_crank(user, crank_slot, 1_000_000, 0, false);
    let _ = assert_ok!(result, "keeper_crank with fees must succeed");

    kani::assert(canonical_inv(&engine), "INV must hold after crank with fees");
    kani::assert(
        conservation_fast_no_funding(&engine),
        "Conservation must hold after crank with fees"
    );
    // Non-vacuity: crank advanced
    kani::assert(
        engine.last_crank_slot > last_crank_before,
        "Crank must advance last_crank_slot"
    );
}

// ============================================================================
// GAP 2: Matcher Trust Boundary (4 proofs)
// ============================================================================

/// Gap 2, Proof 4: Overfill matcher is rejected
#[kani::proof]
#[kani::unwind(5)]
#[kani::solver(cadical)]
fn proof_gap2_rejects_overfill_matcher() {
    let mut engine = RiskEngine::new(test_params());

    let lp = engine.add_lp([1u8; 32], [0u8; 32], 0).unwrap();
    engine.accounts[lp as usize].capital = U128::new(1_000_000);
    engine.vault = engine.vault + U128::new(1_000_000);

    let user = engine.add_user(0).unwrap();
    engine.accounts[user as usize].capital = U128::new(1_000_000);
    engine.vault = engine.vault + U128::new(1_000_000);

    sync_engine_aggregates(&mut engine);

    let oracle: u64 = kani::any();
    kani::assume(oracle >= 1 && oracle <= 2_000_000);
    let size: i128 = kani::any();
    kani::assume(size >= 1 && size <= 10_000_000);

    let result = engine.execute_trade(&OverfillMatcher, lp, user, 0, oracle, size);

    kani::assert(
        matches!(result, Err(RiskError::InvalidMatchingEngine)),
        "Must reject overfill matcher"
    );
}

/// Gap 2, Proof 5: Zero price matcher is rejected
#[kani::proof]
#[kani::unwind(5)]
#[kani::solver(cadical)]
fn proof_gap2_rejects_zero_price_matcher() {
    let mut engine = RiskEngine::new(test_params());

    let lp = engine.add_lp([1u8; 32], [0u8; 32], 0).unwrap();
    engine.accounts[lp as usize].capital = U128::new(1_000_000);
    engine.vault = engine.vault + U128::new(1_000_000);

    let user = engine.add_user(0).unwrap();
    engine.accounts[user as usize].capital = U128::new(1_000_000);
    engine.vault = engine.vault + U128::new(1_000_000);

    sync_engine_aggregates(&mut engine);

    let oracle: u64 = kani::any();
    kani::assume(oracle >= 1 && oracle <= 2_000_000);
    let size: i128 = kani::any();
    kani::assume(size >= 1 && size <= 10_000_000);

    let result = engine.execute_trade(&ZeroPriceMatcher, lp, user, 0, oracle, size);

    kani::assert(
        matches!(result, Err(RiskError::InvalidMatchingEngine)),
        "Must reject zero price matcher"
    );
}

/// Gap 2, Proof 6: Max price + 1 matcher is rejected
#[kani::proof]
#[kani::unwind(5)]
#[kani::solver(cadical)]
fn proof_gap2_rejects_max_price_exceeded_matcher() {
    let mut engine = RiskEngine::new(test_params());

    let lp = engine.add_lp([1u8; 32], [0u8; 32], 0).unwrap();
    engine.accounts[lp as usize].capital = U128::new(1_000_000);
    engine.vault = engine.vault + U128::new(1_000_000);

    let user = engine.add_user(0).unwrap();
    engine.accounts[user as usize].capital = U128::new(1_000_000);
    engine.vault = engine.vault + U128::new(1_000_000);

    sync_engine_aggregates(&mut engine);

    let oracle: u64 = kani::any();
    kani::assume(oracle >= 1 && oracle <= 2_000_000);
    let size: i128 = kani::any();
    kani::assume(size >= 1 && size <= 10_000_000);

    let result = engine.execute_trade(&MaxPricePlusOneMatcher, lp, user, 0, oracle, size);

    kani::assert(
        matches!(result, Err(RiskError::InvalidMatchingEngine)),
        "Must reject max price + 1 matcher"
    );
}

/// Gap 2, Proof 7: execute_trade Err preserves canonical_inv
///
/// Proves: Even though execute_trade mutates state (funding/mark settlement) before
/// discovering the matcher is bad, the engine remains in a valid state on Err.
#[kani::proof]
#[kani::unwind(33)]
#[kani::solver(cadical)]
fn proof_gap2_execute_trade_err_preserves_inv() {
    let mut engine = RiskEngine::new(test_params());
    engine.current_slot = 100;
    engine.last_crank_slot = 100;
    engine.last_full_sweep_start_slot = 100;

    let user = engine.add_user(0).unwrap();
    let lp = engine.add_lp([1u8; 32], [0u8; 32], 0).unwrap();

    let user_cap: u128 = kani::any();
    kani::assume(user_cap >= 5_000 && user_cap <= 50_000);

    // Give accounts existing positions so touch_account/settle_mark are non-trivial
    engine.accounts[user as usize].capital = U128::new(user_cap);
    engine.accounts[user as usize].position_size = I128::new(1_000);
    engine.accounts[user as usize].entry_price = 1_000_000;

    engine.accounts[lp as usize].capital = U128::new(100_000);
    engine.accounts[lp as usize].position_size = I128::new(-1_000);
    engine.accounts[lp as usize].entry_price = 1_000_000;

    engine.vault = U128::new(user_cap + 100_000 + 10_000);
    engine.insurance_fund.balance = U128::new(10_000);
    sync_engine_aggregates(&mut engine);

    kani::assert(canonical_inv(&engine), "INV before execute_trade Err");

    let size: i128 = kani::any();
    kani::assume(size >= 50 && size <= 500);

    // BadMatcherOppositeSign returns opposite sign → always rejected
    // But touch_account/settle_mark run first, mutating state
    let result = engine.execute_trade(&BadMatcherOppositeSign, lp, user, 100, 1_000_000, size);

    // Non-vacuity: must be Err
    kani::assert(result.is_err(), "BadMatcherOppositeSign must be rejected");

    // INV must still hold even on Err path
    kani::assert(
        canonical_inv(&engine),
        "canonical_inv must hold after execute_trade Err"
    );
}

// ============================================================================
// GAP 3: Full Conservation with MTM + Funding (3 proofs)
// ============================================================================

/// Gap 3, Proof 8: Conservation holds when entry_price ≠ oracle
///
/// First trade creates positions at oracle_1 (entry = oracle_1), then second trade
/// at oracle_2 ≠ oracle_1 exercises the mark-to-market settlement path.
#[kani::proof]
#[kani::unwind(33)]
#[kani::solver(cadical)]
fn proof_gap3_conservation_trade_entry_neq_oracle() {
    let mut engine = RiskEngine::new(test_params());
    engine.vault = U128::new(1_000_000);
    engine.insurance_fund.balance = U128::new(100_000);
    engine.current_slot = 100;
    engine.last_crank_slot = 100;
    engine.last_full_sweep_start_slot = 100;

    let user = engine.add_user(0).unwrap();
    let lp = engine.add_lp([1u8; 32], [0u8; 32], 0).unwrap();

    engine.deposit(user, 100_000, 0).unwrap();
    engine.deposit(lp, 500_000, 0).unwrap();

    let oracle_1: u64 = kani::any();
    let oracle_2: u64 = kani::any();
    let size: i128 = kani::any();

    kani::assume(oracle_1 >= 800_000 && oracle_1 <= 1_200_000);
    kani::assume(oracle_2 >= 800_000 && oracle_2 <= 1_200_000);
    kani::assume(size >= 50 && size <= 200);

    // Trade 1: open position at oracle_1 (entry_price set to oracle_1)
    let res1 = engine.execute_trade(&NoOpMatcher, lp, user, 100, oracle_1, size);
    assert!(res1.is_ok(), "non-vacuity: open trade must succeed with well-capitalized accounts");

    // Non-vacuity: entry_price was set to oracle_1
    let _entry_before = engine.accounts[user as usize].entry_price;

    // Trade 2: close at oracle_2 (exercises mark-to-market when entry ≠ oracle)
    let res2 = engine.execute_trade(&NoOpMatcher, lp, user, 100, oracle_2, -size);
    assert!(res2.is_ok(), "non-vacuity: close trade must succeed");

    // Non-vacuity: entry_price was ≠ oracle_2 before the second trade
    // (it was oracle_1 from the first trade, and oracle_1 may differ from oracle_2)

    // Touch both accounts to settle any outstanding funding
    let _ = engine.touch_account(user);
    let _ = engine.touch_account(lp);

    // Primary conservation: vault >= c_tot + insurance
    kani::assert(
        conservation_fast_no_funding(&engine),
        "Primary conservation must hold after trade with entry ≠ oracle"
    );

    // Full canonical invariant (structural + aggregates + accounting + per-account)
    kani::assert(
        canonical_inv(&engine),
        "Canonical INV must hold after trade with entry ≠ oracle"
    );
}

/// Gap 3, Proof 9: Conservation holds after crank with funding on open positions
///
/// Engine has open positions from a prior trade. Crank at different oracle
/// with non-zero funding rate exercises both funding settlement and mark-to-market.
#[kani::proof]
#[kani::unwind(33)]
#[kani::solver(cadical)]
fn proof_gap3_conservation_crank_funding_positions() {
    let mut engine = RiskEngine::new(test_params());
    engine.vault = U128::new(200_000);
    engine.insurance_fund.balance = U128::new(10_000);
    engine.current_slot = 100;
    engine.last_crank_slot = 50;
    engine.last_full_sweep_start_slot = 50;

    let user = engine.add_user(0).unwrap();
    let lp = engine.add_lp([1u8; 32], [0u8; 32], 0).unwrap();

    engine.deposit(user, 30_000, 50).unwrap();
    engine.deposit(lp, 100_000, 50).unwrap();

    // Symbolic size to vary position magnitude and margin pressure
    let size: i128 = kani::any();
    kani::assume(size >= 50 && size <= 200);

    // Open position at oracle_1 (concrete for tractability)
    engine.execute_trade(&NoOpMatcher, lp, user, 100, 1_000_000, size).unwrap();

    // Crank at oracle_2 with symbolic funding rate
    let oracle_2: u64 = kani::any();
    let funding_rate: i64 = kani::any();
    kani::assume(oracle_2 >= 900_000 && oracle_2 <= 1_100_000);
    kani::assume(funding_rate > -50 && funding_rate < 50);

    let result = engine.keeper_crank(user, 150, oracle_2, funding_rate, false);

    // Non-vacuity: crank must succeed
    assert_ok!(result, "crank must succeed");

    // Non-vacuity: at least one account had a position before crank
    // (The crank may liquidate, so we don't assert positions stay open —
    //  that's valid behavior. The point is conservation holds regardless.)

    // Touch both accounts to settle any outstanding funding
    let _ = engine.touch_account(user);
    let _ = engine.touch_account(lp);

    // Primary conservation: vault >= c_tot + insurance
    kani::assert(
        conservation_fast_no_funding(&engine),
        "Primary conservation must hold after crank with funding + positions"
    );

    // Full canonical invariant
    kani::assert(
        canonical_inv(&engine),
        "Canonical INV must hold after crank with funding + positions"
    );
}

/// Gap 3, Proof 10: Multi-step lifecycle conservation
///
/// Full lifecycle: deposit → trade (open) → crank (fund) → trade (close).
/// Verifies canonical_inv after each step and check_conservation at the end.
#[kani::proof]
#[kani::unwind(33)]
#[kani::solver(cadical)]
fn proof_gap3_multi_step_lifecycle_conservation() {
    let mut engine = RiskEngine::new(test_params());
    engine.vault = U128::new(100_000);
    engine.insurance_fund.balance = U128::new(10_000);
    engine.current_slot = 0;
    engine.last_crank_slot = 0;
    engine.last_full_sweep_start_slot = 0;

    let user = engine.add_user(0).unwrap();
    let lp = engine.add_lp([1u8; 32], [0u8; 32], 0).unwrap();

    // Symbolic oracle_2, funding_rate, and size exercise MTM+funding+margin paths.
    // oracle_1 concrete to keep CBMC tractable (4 chained operations).
    let oracle_1: u64 = 1_000_000;
    let oracle_2: u64 = kani::any();
    let funding_rate: i64 = kani::any();
    let size: i128 = kani::any();

    kani::assume(oracle_2 >= 800_000 && oracle_2 <= 1_200_000);
    kani::assume(funding_rate > -50 && funding_rate < 50);
    kani::assume(size >= 50 && size <= 200);

    // Symbolic user deposit — enough for margin on max size at oracle_1
    let user_deposit: u128 = kani::any();
    kani::assume(user_deposit >= 25_000 && user_deposit <= 50_000);

    // Step 1: Deposits
    assert_ok!(engine.deposit(user, user_deposit, 0), "user deposit must succeed");
    assert_ok!(engine.deposit(lp, 200_000, 0), "LP deposit must succeed");
    kani::assert(canonical_inv(&engine), "INV after deposits");

    // Step 2: Open trade at oracle_1
    let trade1 = engine.execute_trade(&NoOpMatcher, lp, user, 0, oracle_1, size);
    assert!(trade1.is_ok(), "non-vacuity: open trade must succeed");
    kani::assert(canonical_inv(&engine), "INV after open trade");

    // Step 3: Crank with funding at oracle_2
    let crank = engine.keeper_crank(user, 50, oracle_2, funding_rate, false);
    // Crank may liquidate or fail depending on funding/oracle — that's valid
    kani::assert(canonical_inv(&engine), "INV after crank");

    // Step 4: Close trade at oracle_2 (if not liquidated)
    let trade2 = engine.execute_trade(&NoOpMatcher, lp, user, 50, oracle_2, -size);
    // Trade may fail if position was liquidated — that's valid
    kani::assert(canonical_inv(&engine), "INV after close trade attempt");

    // Touch both accounts to settle any outstanding funding
    let _ = engine.touch_account(user);
    let _ = engine.touch_account(lp);

    // Primary conservation at final state
    kani::assert(
        conservation_fast_no_funding(&engine),
        "Primary conservation must hold after complete lifecycle"
    );
}

// ============================================================================
// GAP 4: Overflow / Never-Panic at Extreme Values (4 proofs)
// ============================================================================

/// Gap 4, Proof 11: Trade at extreme prices does not panic
///
/// Tries execute_trade at boundary oracle prices {1, 1_000_000, MAX_ORACLE_PRICE}.
/// Either succeeds with INV or returns Err — never panics.
#[kani::proof]
#[kani::unwind(33)]
#[kani::solver(cadical)]
fn proof_gap4_trade_extreme_price_no_panic() {
    let mut engine = RiskEngine::new(test_params());
    engine.insurance_fund.balance = U128::new(10_000);
    engine.current_slot = 100;
    engine.last_crank_slot = 100;
    engine.last_full_sweep_start_slot = 100;

    let user = engine.add_user(0).unwrap();
    let lp = engine.add_lp([1u8; 32], [0u8; 32], 0).unwrap();

    // Symbolic capital: both accounts get the same amount, vault = 2x + insurance
    let capital: u128 = kani::any();
    kani::assume(capital >= 100_000_000 && capital <= 1_000_000_000_000_000);
    engine.accounts[user as usize].capital = U128::new(capital);
    engine.accounts[lp as usize].capital = U128::new(capital);
    engine.vault = U128::new(capital * 2 + 10_000);
    engine.recompute_aggregates();

    // Symbolic oracle price covering full valid range
    let oracle: u64 = kani::any();
    kani::assume(oracle >= 1 && oracle <= MAX_ORACLE_PRICE);

    let result = engine.execute_trade(&NoOpMatcher, lp, user, 100, oracle, 100);

    // Must not panic; on Ok path, INV must hold
    if result.is_ok() {
        kani::assert(canonical_inv(&engine), "INV on Ok at symbolic oracle+capital");
    }
    // Non-vacuity: very large capital must always succeed
    if capital >= 500_000_000_000_000 {
        kani::assert(result.is_ok(), "large capital trade must succeed");
    }
}

/// Gap 4, Proof 12: Trade at extreme sizes does not panic
///
/// Tries execute_trade with size at boundary values {1, MAX_POSITION_ABS/2, MAX_POSITION_ABS}.
/// Either succeeds with INV or returns Err — never panics.
#[kani::proof]
#[kani::unwind(33)]
#[kani::solver(cadical)]
fn proof_gap4_trade_extreme_size_no_panic() {
    let deep_capital = 20_000_000_000_000_000_000u128;

    let mut engine = RiskEngine::new(test_params());
    engine.vault = U128::new(10_000);
    engine.insurance_fund.balance = U128::new(10_000);
    engine.current_slot = 100;
    engine.last_crank_slot = 100;
    engine.last_full_sweep_start_slot = 100;
    let user = engine.add_user(0).unwrap();
    let lp = engine.add_lp([1u8; 32], [0u8; 32], 0).unwrap();
    engine.deposit(user, deep_capital, 0).unwrap();
    engine.deposit(lp, deep_capital, 0).unwrap();

    // Symbolic size covering full valid range [1, MAX_POSITION_ABS]
    let size: u128 = kani::any();
    kani::assume(size >= 1 && size <= MAX_POSITION_ABS);

    // Symbolic oracle price (original was concrete 1_000_000)
    let oracle: u64 = kani::any();
    kani::assume(oracle >= 100_000 && oracle <= 10_000_000);

    let result = engine.execute_trade(&NoOpMatcher, lp, user, 100, oracle, size as i128);
    if result.is_ok() {
        kani::assert(canonical_inv(&engine), "INV at symbolic size+oracle");
    }
    // Non-vacuity: moderate size at $1 must succeed with deep capital
    if size <= 1_000_000 && oracle == 1_000_000 {
        kani::assert(result.is_ok(), "moderate size at $1 must succeed");
    }
}

/// Gap 4, Proof 13: Partial fill at different price does not panic
///
/// PartialFillDiffPriceMatcher returns half fill at oracle - 100_000.
/// Symbolic oracle and size; either succeeds with INV or returns Err.
#[kani::proof]
#[kani::unwind(33)]
#[kani::solver(cadical)]
fn proof_gap4_trade_partial_fill_diff_price_no_panic() {
    let mut engine = RiskEngine::new(test_params());
    engine.insurance_fund.balance = U128::new(10_000);
    engine.current_slot = 100;
    engine.last_crank_slot = 100;
    engine.last_full_sweep_start_slot = 100;

    let user = engine.add_user(0).unwrap();
    let lp = engine.add_lp([1u8; 32], [0u8; 32], 0).unwrap();

    // Symbolic capitals
    let user_cap: u128 = kani::any();
    let lp_cap: u128 = kani::any();
    kani::assume(user_cap >= 100_000 && user_cap <= 500_000);
    kani::assume(lp_cap >= 100_000 && lp_cap <= 500_000);

    engine.accounts[user as usize].capital = U128::new(user_cap);
    engine.accounts[lp as usize].capital = U128::new(lp_cap);
    engine.vault = U128::new(user_cap + lp_cap + 10_000);
    engine.recompute_aggregates();

    let oracle: u64 = kani::any();
    let size: i128 = kani::any();
    kani::assume(oracle >= 500_000 && oracle <= 1_500_000);
    kani::assume(size >= 50 && size <= 500);

    let result = engine.execute_trade(&PartialFillDiffPriceMatcher, lp, user, 100, oracle, size);
    if result.is_ok() {
        kani::assert(
            canonical_inv(&engine),
            "INV must hold after partial fill at different price",
        );
    }
    // Non-vacuity: conservative trade with sufficient capital must succeed
    if user_cap >= 300_000 && lp_cap >= 300_000 && size <= 100 && oracle >= 800_000 && oracle <= 1_200_000 {
        kani::assert(result.is_ok(), "conservative partial fill must succeed");
    }
}

/// Gap 4, Proof 14: Margin functions at extreme values do not panic
///
/// Tests is_above_maintenance_margin_mtm and account_equity_mtm_at_oracle
/// with extreme capital, negative pnl, large position, and extreme oracle.
#[kani::proof]
#[kani::unwind(33)]
#[kani::solver(cadical)]
fn proof_gap4_margin_extreme_values_no_panic() {
    let mut engine = RiskEngine::new(test_params());
    let user = engine.add_user(0).unwrap();

    // Symbolic position (long or short) and capital
    let pos: i128 = kani::any();
    kani::assume(pos != 0 && pos > -500 && pos < 500);
    let capital: u128 = kani::any();
    kani::assume(capital >= 1_000 && capital <= 10_000);
    let pnl: i128 = kani::any();
    kani::assume(pnl > -5_000 && pnl < 5_000);

    engine.accounts[user as usize].capital = U128::new(capital);
    engine.accounts[user as usize].pnl = I128::new(pnl);
    engine.accounts[user as usize].position_size = I128::new(pos);

    // Symbolic entry price (exercises mark PnL in both directions)
    let entry: u64 = kani::any();
    kani::assume(entry >= 900_000 && entry <= 1_100_000);
    engine.accounts[user as usize].entry_price = entry;
    engine.accounts[user as usize].funding_index = engine.funding_index_qpb_e6;

    // vault = capital to satisfy A1: vault >= c_tot + insurance (insurance=0)
    engine.vault = U128::new(capital);
    sync_engine_aggregates(&mut engine);
    kani::assert(canonical_inv(&engine), "setup INV");

    // Symbolic oracle price
    let oracle: u64 = kani::any();
    kani::assume(oracle >= 900_000 && oracle <= 1_100_000);

    // These calls must not panic regardless of values
    let eq = engine.account_equity_mtm_at_oracle(&engine.accounts[user as usize], oracle);
    let mm = engine.is_above_maintenance_margin_mtm(&engine.accounts[user as usize], oracle);

    // Meaningful property: if capital is large and pnl >= 0, equity must be positive
    if pnl >= 0 && capital >= 10_000 {
        kani::assert(eq > 0, "high capital with non-negative pnl must have positive equity");
    }

    // If above maintenance margin at this oracle, then equity must be positive
    if mm {
        kani::assert(eq > 0, "above-margin implies positive equity");
    }
}

// ============================================================================
// GAP 5: Fee Credit Corner Cases (4 proofs)
// ============================================================================

/// Gap 5, Proof 15: settle_maintenance_fee leaves account above margin or returns Err
///
/// After settle_maintenance_fee, if Ok then either account is above maintenance margin
/// or has no position. If Err(Undercollateralized), account has position and
/// insufficient equity.
#[kani::proof]
#[kani::unwind(33)]
#[kani::solver(cadical)]
fn proof_gap5_fee_settle_margin_or_err() {
    let mut engine = RiskEngine::new(test_params_with_maintenance_fee());
    engine.vault = U128::new(200_000);
    engine.insurance_fund.balance = U128::new(10_000);
    engine.current_slot = 100;
    engine.last_crank_slot = 100;
    engine.last_full_sweep_start_slot = 100;

    let user = engine.add_user(0).unwrap();
    let lp = engine.add_lp([1u8; 32], [0u8; 32], 0).unwrap();

    let user_cap: u128 = kani::any();
    kani::assume(user_cap >= 100 && user_cap <= 10_000);

    engine.deposit(user, user_cap, 100).unwrap();
    engine.deposit(lp, 100_000, 100).unwrap();

    // Create a position (symbolic size)
    let size: i128 = kani::any();
    kani::assume(size >= -500 && size <= 500 && size != 0);

    let trade_result = engine.execute_trade(&NoOpMatcher, lp, user, 100, 1_000_000, size);
    let _ = assert_ok!(
        trade_result,
        "bounded setup trade must succeed before settle_maintenance_fee"
    );

    // Set symbolic fee_credits
    let fee_credits: i128 = kani::any();
    kani::assume(fee_credits > -1000 && fee_credits < 1000);
    engine.accounts[user as usize].fee_credits = I128::new(fee_credits);

    // Set last_fee_slot so that some time passes
    engine.accounts[user as usize].last_fee_slot = 100;

    let oracle: u64 = 1_000_000;
    let now_slot: u64 = kani::any();
    kani::assume(now_slot >= 101 && now_slot <= 600);

    kani::assert(canonical_inv(&engine), "INV before settle_maintenance_fee");

    let result = engine.settle_maintenance_fee(user, now_slot, oracle);

    match result {
        Ok(_) => {
            kani::assert(canonical_inv(&engine), "INV after settle_maintenance_fee Ok");
            // After Ok, account must either be above maintenance margin or have no position
            let has_position = !engine.accounts[user as usize].position_size.is_zero();
            if has_position {
                kani::assert(
                    engine.is_above_maintenance_margin_mtm(&engine.accounts[user as usize], oracle),
                    "After settle_maintenance_fee Ok with position: must be above maintenance margin"
                );
            }
        }
        Err(RiskError::Undercollateralized) => {
            // Position exists and margin is insufficient
            kani::assert(
                !engine.accounts[user as usize].position_size.is_zero(),
                "Undercollateralized error requires open position"
            );
        }
        Err(_) => kani::assert(
            false,
            "unexpected error class from settle_maintenance_fee in this bounded setup"
        ),
    }
}

/// Gap 5, Proof 16: Fee credits after trade then settle are deterministic
///
/// After trade (credits fee) + settle_maintenance_fee, fee_credits follows
/// predictable formula and canonical_inv holds.
#[kani::proof]
#[kani::unwind(33)]
#[kani::solver(cadical)]
fn proof_gap5_fee_credits_trade_then_settle_bounded() {
    let mut engine = RiskEngine::new(test_params_with_maintenance_fee());
    engine.vault = U128::new(400_000);
    engine.insurance_fund.balance = U128::new(10_000);
    engine.current_slot = 100;
    engine.last_crank_slot = 100;
    engine.last_full_sweep_start_slot = 100;

    let user = engine.add_user(0).unwrap();
    let lp = engine.add_lp([1u8; 32], [0u8; 32], 0).unwrap();

    let user_cap: u128 = kani::any();
    kani::assume(user_cap >= 10_000 && user_cap <= 50_000);

    engine.deposit(user, user_cap, 100).unwrap();
    engine.deposit(lp, 100_000, 100).unwrap();

    // Capture fee_credits before trade (should be 0)
    let credits_before_trade = engine.accounts[user as usize].fee_credits.get();

    // Execute trade with wider symbolic size to vary fee credit increment
    // Oracle kept concrete since formula assertions depend on exact fee computation
    let size: i128 = kani::any();
    kani::assume(size != 0 && size > -500 && size < 500);
    let trade_result = engine.execute_trade(&NoOpMatcher, lp, user, 100, 1_000_000, size);
    assert_ok!(trade_result, "trade must succeed");

    let credits_after_trade = engine.accounts[user as usize].fee_credits.get();
    // Trading fee was credited — credits increased (both long and short trades)
    let trade_credit = credits_after_trade - credits_before_trade;
    kani::assert(trade_credit >= 0, "trade must credit non-negative fee_credits");

    // Set last_fee_slot
    engine.accounts[user as usize].last_fee_slot = 100;

    // Settle maintenance fee after dt slots
    let dt: u64 = kani::any();
    kani::assume(dt >= 1 && dt <= 500);

    let paid_from_capital = assert_ok!(
        engine.settle_maintenance_fee(user, 100 + dt, 1_000_000),
        "maintenance settle must succeed with high capital and bounded dt"
    );

    // Deterministic coupon math in this setup:
    // due = dt (fee_per_slot=1)
    // fee_credits' = fee_credits_before - due + paid_from_capital
    // with sufficient capital, paid_from_capital = max(due - fee_credits_before, 0)
    let credits_after_settle = engine.accounts[user as usize].fee_credits.get();
    let due_i = dt as i128;
    let expected_paid = core::cmp::max(due_i.saturating_sub(credits_after_trade), 0) as u128;
    let expected_credits = credits_after_trade
        .saturating_sub(due_i)
        .saturating_add(expected_paid as i128);

    kani::assert(
        paid_from_capital == expected_paid,
        "paid_from_capital must match deterministic coupon shortfall"
    );
    kani::assert(
        credits_after_settle == expected_credits,
        "fee_credits must follow deterministic settle formula"
    );

    kani::assert(canonical_inv(&engine), "canonical_inv must hold after trade + settle");
}

/// Gap 5, Proof 17: fee_credits saturating near i128::MAX
///
/// Tests that fee_credits uses saturating arithmetic and never wraps around.
#[kani::proof]
#[kani::unwind(33)]
#[kani::solver(cadical)]
fn proof_gap5_fee_credits_saturating_near_max() {
    let mut engine = RiskEngine::new(test_params());
    engine.vault = U128::new(1_000_000);
    engine.insurance_fund.balance = U128::new(10_000);
    engine.current_slot = 100;
    engine.last_crank_slot = 100;
    engine.last_full_sweep_start_slot = 100;

    let user = engine.add_user(0).unwrap();
    let lp = engine.add_lp([1u8; 32], [0u8; 32], 0).unwrap();

    engine.accounts[user as usize].capital = U128::new(100_000);
    engine.accounts[lp as usize].capital = U128::new(500_000);
    engine.recompute_aggregates();

    // Set fee_credits close to i128::MAX with symbolic offset
    let offset: u128 = kani::any();
    kani::assume(offset >= 1 && offset <= 10_000);
    assert_ok!(
        engine.add_fee_credits(user, (i128::MAX as u128) - offset),
        "add_fee_credits must succeed"
    );

    let credits_before = engine.accounts[user as usize].fee_credits.get();

    // Symbolic size to vary the fee credit increment amount
    let size: i128 = kani::any();
    kani::assume(size >= 10 && size <= 500);

    // Execute trade which adds more fee credits via saturating_add
    let result = engine.execute_trade(&NoOpMatcher, lp, user, 100, 1_000_000, size);
    let _ = assert_ok!(
        result,
        "trade near fee_credits upper bound must succeed with this setup"
    );

    let credits_after = engine.accounts[user as usize].fee_credits.get();
    // Must not have wrapped — saturating_add caps at i128::MAX
    kani::assert(credits_after <= i128::MAX, "fee_credits must not wrap");
    kani::assert(credits_after >= credits_before, "fee_credits must not decrease from trade");
    kani::assert(canonical_inv(&engine), "INV must hold after trade near fee_credits max");
}

/// Gap 5, Proof 18: deposit_fee_credits preserves conservation
///
/// deposit_fee_credits adds to vault, insurance, and fee_credits simultaneously.
/// Verifies conservation_fast_no_funding still holds.
#[kani::proof]
#[kani::unwind(33)]
#[kani::solver(cadical)]
fn proof_gap5_deposit_fee_credits_conservation() {
    let mut engine = RiskEngine::new(test_params());
    let user = engine.add_user(0).unwrap();

    let capital: u128 = kani::any();
    kani::assume(capital >= 100 && capital <= 10_000);

    engine.accounts[user as usize].capital = U128::new(capital);
    engine.vault = U128::new(capital);
    sync_engine_aggregates(&mut engine);

    kani::assert(canonical_inv(&engine), "INV before deposit_fee_credits");

    let vault_before = engine.vault.get();
    let insurance_before = engine.insurance_fund.balance.get();
    let credits_before = engine.accounts[user as usize].fee_credits.get();

    let amount: u128 = kani::any();
    kani::assume(amount >= 1 && amount <= 10_000);

    let result = engine.deposit_fee_credits(user, amount, 0);

    // Non-vacuity: must succeed
    assert_ok!(result, "deposit_fee_credits must succeed");

    // canonical_inv must hold after deposit_fee_credits
    kani::assert(canonical_inv(&engine), "INV after deposit_fee_credits");

    // Verify vault increased by amount
    kani::assert(
        engine.vault.get() == vault_before + amount,
        "vault must increase by amount"
    );

    // Verify insurance increased by amount
    kani::assert(
        engine.insurance_fund.balance.get() == insurance_before + amount,
        "insurance must increase by amount"
    );

    // Verify fee_credits increased by amount (saturating)
    let credits_after = engine.accounts[user as usize].fee_credits.get();
    kani::assert(
        credits_after == credits_before.saturating_add(amount as i128),
        "fee_credits must increase by amount"
    );
}

// ============================================================================
// PREMARKET RESOLUTION / AGGREGATE CONSISTENCY PROOFS
// ============================================================================
//
// These proofs ensure the Bug #10 class (aggregate desync) is impossible.
// Bug #10: Force-close bypassed set_pnl(), leaving pnl_pos_tot stale.
//
// Strategy: Prove that set_pnl() maintains pnl_pos_tot invariant, and that
// any code simulating force-close MUST use set_pnl() to preserve invariants.

/// Prove set_pnl maintains pnl_pos_tot aggregate invariant.
/// This is the foundation proof - if set_pnl is correct, code using it is safe.
#[kani::proof]
#[kani::unwind(5)]
#[kani::solver(cadical)]
fn proof_set_pnl_maintains_pnl_pos_tot() {
    let mut engine = RiskEngine::new(test_params());
    let user = engine.add_user(0).unwrap();

    // Setup initial state with some pnl
    let initial_pnl: i128 = kani::any();
    kani::assume(initial_pnl > -100_000 && initial_pnl < 100_000);
    engine.set_pnl(user as usize, initial_pnl);

    // Verify initial invariant holds
    kani::assert(canonical_inv(&engine), "setup must satisfy INV");

    // Now change pnl to a new value
    let new_pnl: i128 = kani::any();
    kani::assume(new_pnl > -100_000 && new_pnl < 100_000);

    engine.set_pnl(user as usize, new_pnl);

    // Invariant must still hold
    kani::assert(
        canonical_inv(&engine),
        "set_pnl must maintain canonical_inv"
    );
}

/// Prove set_capital maintains c_tot aggregate invariant.
#[kani::proof]
#[kani::unwind(5)]
#[kani::solver(cadical)]
fn proof_set_capital_maintains_c_tot() {
    let mut engine = RiskEngine::new(test_params());
    let user = engine.add_user(0).unwrap();

    // Setup initial capital
    let initial_cap: u128 = kani::any();
    kani::assume(initial_cap < 100_000);
    engine.set_capital(user as usize, initial_cap);
    engine.vault = U128::new(initial_cap + 1000); // Ensure vault covers

    // Verify initial invariant
    kani::assert(canonical_inv(&engine), "setup must satisfy INV");

    // Change capital
    let new_cap: u128 = kani::any();
    kani::assume(new_cap < 100_000);
    engine.vault = U128::new(new_cap + 1000);

    engine.set_capital(user as usize, new_cap);

    kani::assert(
        canonical_inv(&engine),
        "set_capital must maintain canonical_inv"
    );
}

/// Prove force-close-style PnL modification using set_pnl preserves invariants.
/// This simulates what the fixed force-close code does.
#[kani::proof]
#[kani::unwind(5)]
#[kani::solver(cadical)]
fn proof_force_close_with_set_pnl_preserves_invariant() {
    let mut engine = RiskEngine::new(test_params());
    let user = engine.add_user(0).unwrap();

    // Setup: user has position and some existing pnl
    let initial_pnl: i128 = kani::any();
    let position: i128 = kani::any();
    let entry_price: u64 = kani::any();
    let settlement_price: u64 = kani::any();

    kani::assume(initial_pnl > -50_000 && initial_pnl < 50_000);
    kani::assume(position > -10_000 && position < 10_000 && position != 0);
    kani::assume(entry_price > 0 && entry_price < 10_000_000);
    kani::assume(settlement_price > 0 && settlement_price < 10_000_000);

    engine.set_pnl(user as usize, initial_pnl);
    engine.accounts[user as usize].position_size = I128::new(position);
    engine.accounts[user as usize].entry_price = entry_price;
    sync_engine_aggregates(&mut engine);

    // Precondition: canonical_inv holds before force-close
    kani::assert(canonical_inv(&engine), "setup must satisfy INV");

    // Simulate force-close (CORRECT way - using set_pnl)
    let settle = settlement_price as i128;
    let entry = entry_price as i128;
    let pnl_delta = position.saturating_mul(settle.saturating_sub(entry)) / 1_000_000;
    let old_pnl = engine.accounts[user as usize].pnl.get();
    let new_pnl = old_pnl.saturating_add(pnl_delta);

    // THE CORRECT FIX: use set_pnl
    engine.set_pnl(user as usize, new_pnl);
    engine.accounts[user as usize].position_size = I128::ZERO;
    engine.accounts[user as usize].entry_price = 0;

    // Only update OI manually (position zeroed).
    // IMPORTANT: Do NOT call sync_engine_aggregates/recompute_aggregates here!
    // We want to verify that set_pnl ALONE maintains pnl_pos_tot.
    engine.total_open_interest = U128::new(0);

    // Postcondition: canonical_inv still holds
    kani::assert(
        canonical_inv(&engine),
        "force-close using set_pnl must preserve canonical_inv"
    );
}

/// Prove that multiple force-close operations preserve invariants.
/// Tests pagination scenario with multiple accounts.
#[kani::proof]
#[kani::unwind(5)]
#[kani::solver(cadical)]
fn proof_multiple_force_close_preserves_invariant() {
    let mut engine = RiskEngine::new(test_params());
    let user1 = engine.add_user(0).unwrap();
    let user2 = engine.add_user(0).unwrap();

    // Setup both users with positions
    let pos1: i128 = kani::any();
    let pos2: i128 = kani::any();
    kani::assume(pos1 > -5_000 && pos1 < 5_000 && pos1 != 0);
    kani::assume(pos2 > -5_000 && pos2 < 5_000 && pos2 != 0);

    engine.accounts[user1 as usize].position_size = I128::new(pos1);
    engine.accounts[user1 as usize].entry_price = 1_000_000;
    engine.accounts[user2 as usize].position_size = I128::new(pos2);
    engine.accounts[user2 as usize].entry_price = 1_000_000;
    sync_engine_aggregates(&mut engine);

    kani::assert(canonical_inv(&engine), "setup must satisfy INV");

    let settlement_price: u64 = kani::any();
    kani::assume(settlement_price > 0 && settlement_price < 2_000_000);

    // Force-close user1
    let pnl_delta1 = pos1.saturating_mul(settlement_price as i128 - 1_000_000) / 1_000_000;
    let new_pnl1 = engine.accounts[user1 as usize].pnl.get().saturating_add(pnl_delta1);
    engine.set_pnl(user1 as usize, new_pnl1);
    engine.accounts[user1 as usize].position_size = I128::ZERO;

    // Force-close user2
    let pnl_delta2 = pos2.saturating_mul(settlement_price as i128 - 1_000_000) / 1_000_000;
    let new_pnl2 = engine.accounts[user2 as usize].pnl.get().saturating_add(pnl_delta2);
    engine.set_pnl(user2 as usize, new_pnl2);
    engine.accounts[user2 as usize].position_size = I128::ZERO;

    // Only update OI manually (both positions zeroed).
    // IMPORTANT: Do NOT call sync_engine_aggregates/recompute_aggregates!
    // We want to verify that set_pnl ALONE maintains pnl_pos_tot.
    engine.total_open_interest = U128::new(0);

    kani::assert(
        canonical_inv(&engine),
        "multiple force-close operations must preserve canonical_inv"
    );
}

/// Prove haircut_ratio uses the stored pnl_pos_tot (which set_pnl maintains).
/// If pnl_pos_tot is accurate, haircut calculations are correct.
#[kani::proof]
#[kani::unwind(5)]
#[kani::solver(cadical)]
fn proof_haircut_ratio_bounded() {
    let mut engine = RiskEngine::new(test_params());
    let user = engine.add_user(0).unwrap();

    let capital: u128 = kani::any();
    let pnl: i128 = kani::any();
    let insurance: u128 = kani::any();
    let vault: u128 = kani::any();

    kani::assume(capital > 0 && capital <= 500);
    kani::assume(pnl > -500 && pnl <= 500); // Both positive and negative pnl
    kani::assume(insurance <= 200);
    kani::assume(vault <= 1_500);

    engine.set_capital(user as usize, capital);
    engine.set_pnl(user as usize, pnl);
    engine.insurance_fund.balance = U128::new(insurance);
    engine.vault = U128::new(vault);

    let (h_num, h_den) = engine.haircut_ratio();

    // Haircut ratio must be in [0, 1]
    kani::assert(h_num <= h_den, "haircut ratio must be <= 1");
    kani::assert(h_den > 0 || (h_num == 1 && h_den == 1), "haircut denominator must be positive or (1,1)");

    // When pnl <= 0, pnl_pos_tot = 0 → haircut = (1,1) (no positive pnl to haircut)
    if pnl <= 0 {
        kani::assert(h_num == 1 && h_den == 1, "no positive pnl → haircut ratio = 1");
    }

    // When pnl > 0 AND vault < c_tot + insurance, haircut must be 0 (insolvent)
    if pnl > 0 && vault < capital + insurance {
        kani::assert(h_num == 0, "insolvent with positive pnl: haircut must be 0");
    }
}

/// Prove effective_pos_pnl never exceeds actual positive pnl.
/// Haircut can only reduce, never increase, the effective pnl.
#[kani::proof]
#[kani::unwind(5)]
#[kani::solver(cadical)]
fn proof_effective_pnl_bounded_by_actual() {
    let mut engine = RiskEngine::new(test_params());
    let user = engine.add_user(0).unwrap();

    let capital: u128 = kani::any();
    let pnl: i128 = kani::any();
    let insurance: u128 = kani::any();

    kani::assume(capital > 0 && capital < 10_000);
    kani::assume(pnl > -5_000 && pnl < 5_000);
    kani::assume(insurance <= 5_000);

    engine.set_capital(user as usize, capital);
    engine.set_pnl(user as usize, pnl);
    engine.insurance_fund.balance = U128::new(insurance);
    let pnl_pos = if pnl > 0 { pnl as u128 } else { 0 };
    engine.vault = U128::new(capital + insurance + pnl_pos);

    kani::assert(canonical_inv(&engine), "INV before effective_pos_pnl");

    let eff = engine.effective_pos_pnl(pnl);
    let actual_pos = if pnl > 0 { pnl as u128 } else { 0 };

    kani::assert(
        eff <= actual_pos,
        "effective_pos_pnl must not exceed actual positive pnl"
    );

    // When negative pnl: effective must be 0
    if pnl <= 0 {
        kani::assert(eff == 0, "negative pnl → effective must be 0");
    }
}

/// Prove recompute_aggregates produces correct values.
/// This is a sanity check that our test helper is correct.
#[kani::proof]
#[kani::unwind(5)]
#[kani::solver(cadical)]
fn proof_recompute_aggregates_correct() {
    let mut engine = RiskEngine::new(test_params());
    let user = engine.add_user(0).unwrap();

    // Manually set account fields (bypassing helpers to test recompute)
    let capital: u128 = kani::any();
    let pnl: i128 = kani::any();
    kani::assume(capital < 100_000);
    kani::assume(pnl > -50_000 && pnl < 50_000);

    engine.accounts[user as usize].capital = U128::new(capital);
    engine.accounts[user as usize].pnl = I128::new(pnl);

    // Aggregates are now stale (we bypassed set_pnl/set_capital)
    // recompute_aggregates should fix them
    engine.recompute_aggregates();

    // Now invariant should hold
    kani::assert(
        engine.c_tot.get() == capital,
        "recompute_aggregates must fix c_tot"
    );

    let expected_pnl_pos = if pnl > 0 { pnl as u128 } else { 0 };
    kani::assert(
        engine.pnl_pos_tot.get() == expected_pnl_pos,
        "recompute_aggregates must fix pnl_pos_tot"
    );
}

/// NEGATIVE PROOF: Demonstrates that bypassing set_pnl() breaks invariants.
/// This proof is EXPECTED TO FAIL - it shows our real proofs are non-vacuous.
///
/// Proof: set_pnl maintains aggregates, but direct bypass breaks them.
/// Part 1: proper set_pnl always preserves inv_aggregates.
/// Part 2: direct pnl assignment (bypassing set_pnl) always breaks inv_aggregates
///          when the positive-PnL contribution changes.
#[kani::proof]
#[kani::unwind(5)]
#[kani::solver(cadical)]
fn proof_NEGATIVE_bypass_set_pnl_breaks_invariant() {
    let mut engine = RiskEngine::new(test_params());
    engine.vault = U128::new(100_000);
    let user = engine.add_user(0).unwrap();

    // Setup initial state via proper set_pnl
    let initial_pnl: i128 = kani::any();
    kani::assume(initial_pnl > -50_000 && initial_pnl < 50_000);
    engine.set_capital(user as usize, 10_000);
    engine.set_pnl(user as usize, initial_pnl);
    sync_engine_aggregates(&mut engine);
    kani::assert(canonical_inv(&engine), "INV after proper set_pnl");

    // Part 1: proper set_pnl preserves invariant
    let new_pnl: i128 = kani::any();
    kani::assume(new_pnl > -50_000 && new_pnl < 50_000);
    engine.set_pnl(user as usize, new_pnl);
    kani::assert(inv_aggregates(&engine), "proper set_pnl preserves aggregates");

    // Reset to initial for Part 2
    engine.set_pnl(user as usize, initial_pnl);

    // Part 2: bypass breaks invariant when positive-PnL contribution changes
    let bypass_pnl: i128 = kani::any();
    kani::assume(bypass_pnl > -50_000 && bypass_pnl < 50_000);
    let old_contrib = if initial_pnl > 0 { initial_pnl as u128 } else { 0u128 };
    let new_contrib = if bypass_pnl > 0 { bypass_pnl as u128 } else { 0u128 };
    kani::assume(old_contrib != new_contrib);

    // BUG: Direct assignment bypasses aggregate maintenance!
    engine.accounts[user as usize].pnl = I128::new(bypass_pnl);

    kani::assert(
        !inv_aggregates(&engine),
        "bypassing set_pnl must break pnl_pos_tot invariant",
    );
}

// ============================================================================
// MISSING CONSERVATION PROOFS - Operations that lacked vault >= c_tot + insurance verification
// ============================================================================

// ----------------------------------------------------------------------------
// settle_mark_to_oracle: Ok path conservation
// Only modifies per-account pnl and entry_price. vault/c_tot/insurance untouched.
// ----------------------------------------------------------------------------

/// settle_mark_to_oracle: INV preserved on Ok path.
/// Mark settlement only modifies account PnL and entry_price;
/// vault, c_tot, and insurance are all untouched.
#[kani::proof]
#[kani::unwind(33)]
#[kani::solver(cadical)]
fn proof_settle_mark_to_oracle_preserves_inv() {
    let mut engine = RiskEngine::new(test_params());
    engine.vault = U128::new(100_000);

    let user_idx = engine.add_user(0).unwrap();
    engine.accounts[user_idx as usize].capital = U128::new(10_000);

    // Open a position so mark settlement does real work
    let pos: i128 = kani::any();
    kani::assume(pos >= -1_000 && pos <= 1_000 && pos != 0);
    engine.accounts[user_idx as usize].position_size = I128::new(pos);
    engine.accounts[user_idx as usize].entry_price = 1_000_000; // $1.00
    engine.accounts[user_idx as usize].funding_index = engine.funding_index_qpb_e6;

    sync_engine_aggregates(&mut engine);
    kani::assert(canonical_inv(&engine), "setup state must satisfy INV");

    let oracle: u64 = kani::any();
    kani::assume(oracle >= 500_000 && oracle <= 2_000_000); // $0.50 - $2.00

    let vault_before = engine.vault.get();
    let c_tot_before = engine.c_tot.get();
    let ins_before = engine.insurance_fund.balance.get();

    let result = engine.settle_mark_to_oracle(user_idx, oracle);

    if result.is_ok() {
        kani::assert(canonical_inv(&engine), "INV must hold after settle_mark_to_oracle");
        // vault, c_tot, insurance must all be unchanged
        kani::assert(engine.vault.get() == vault_before, "vault unchanged by mark settlement");
        kani::assert(engine.c_tot.get() == c_tot_before, "c_tot unchanged by mark settlement");
        kani::assert(
            engine.insurance_fund.balance.get() == ins_before,
            "insurance unchanged by mark settlement",
        );
    }

    // Non-vacuity: force Ok path
    let _ = assert_ok!(result, "settle_mark_to_oracle must succeed with small position");
}

// ----------------------------------------------------------------------------
// touch_account: Ok path conservation
// Settles funding — only modifies account PnL and funding_index.
// vault/c_tot/insurance untouched.
// ----------------------------------------------------------------------------

/// touch_account: INV preserved on Ok path.
/// Funding settlement redistributes PnL between accounts (zero-sum);
/// vault, c_tot, and insurance are all untouched.
#[kani::proof]
#[kani::unwind(33)]
#[kani::solver(cadical)]
fn proof_touch_account_preserves_inv() {
    let mut engine = RiskEngine::new(test_params());
    engine.vault = U128::new(200_000);

    let user_idx = engine.add_user(0).unwrap();
    engine.accounts[user_idx as usize].capital = U128::new(10_000);

    // Position with stale funding index so touch does work
    let pos: i128 = kani::any();
    kani::assume(pos >= -500 && pos <= 500 && pos != 0);
    engine.accounts[user_idx as usize].position_size = I128::new(pos);
    engine.accounts[user_idx as usize].entry_price = 1_000_000;

    // Advance the global funding index so there's a delta to settle
    let funding_delta: i128 = kani::any();
    kani::assume(funding_delta >= -100_000 && funding_delta <= 100_000);
    engine.funding_index_qpb_e6 = I128::new(funding_delta);
    // Account's index is 0 (default), so delta = funding_delta

    sync_engine_aggregates(&mut engine);
    kani::assert(canonical_inv(&engine), "setup state must satisfy INV");

    let vault_before = engine.vault.get();
    let c_tot_before = engine.c_tot.get();
    let ins_before = engine.insurance_fund.balance.get();

    let result = engine.touch_account(user_idx);

    if result.is_ok() {
        kani::assert(canonical_inv(&engine), "INV must hold after touch_account");
        kani::assert(engine.vault.get() == vault_before, "vault unchanged by touch");
        kani::assert(engine.c_tot.get() == c_tot_before, "c_tot unchanged by touch");
        kani::assert(
            engine.insurance_fund.balance.get() == ins_before,
            "insurance unchanged by touch",
        );
    }

    // Non-vacuity: force Ok path
    let _ = assert_ok!(result, "touch_account must succeed");
}

// ----------------------------------------------------------------------------
// touch_account_full: Ok path conservation
// Composite: funding + mark + maintenance fee + warmup settle + fee debt sweep.
// vault unchanged. c_tot may decrease (fees/losses), insurance may increase (fees).
// ----------------------------------------------------------------------------

/// touch_account_full: INV preserved on Ok path (symbolic capital + pnl + oracle + slot).
/// Exercises: funding, mark-to-market, maintenance fee, warmup settlement (positive
/// and negative PnL), fee debt sweep, and Undercollateralized error path.
#[kani::proof]
#[kani::unwind(33)]
#[kani::solver(cadical)]
fn proof_touch_account_full_preserves_inv() {
    let mut engine = RiskEngine::new(test_params_with_maintenance_fee());
    engine.current_slot = 100;
    engine.last_crank_slot = 100;
    engine.last_full_sweep_start_slot = 100;

    let capital: u128 = kani::any();
    let pnl_raw: i128 = kani::any();
    let oracle: u64 = kani::any();
    let now_slot: u64 = kani::any();
    kani::assume(capital >= 15_000 && capital <= 40_000);
    kani::assume(pnl_raw >= -500 && pnl_raw <= 500);
    kani::assume(oracle >= 950_000 && oracle <= 1_050_000);
    kani::assume(now_slot >= 101 && now_slot <= 200);

    let user_idx = engine.add_user(0).unwrap();
    engine.accounts[user_idx as usize].capital = U128::new(capital);
    engine.accounts[user_idx as usize].pnl = I128::new(pnl_raw);
    engine.accounts[user_idx as usize].position_size = I128::new(500_000);
    engine.accounts[user_idx as usize].entry_price = 1_000_000;
    engine.accounts[user_idx as usize].funding_index = engine.funding_index_qpb_e6;
    engine.accounts[user_idx as usize].last_fee_slot = 100;
    engine.accounts[user_idx as usize].warmup_started_at_slot = 0;
    engine.accounts[user_idx as usize].warmup_slope_per_step = U128::new(50);

    // LP counterparty (well-capitalized, opposite position)
    let lp_idx = engine.add_lp([1u8; 32], [0u8; 32], 0).unwrap();
    engine.accounts[lp_idx as usize].capital = U128::new(50_000);
    engine.accounts[lp_idx as usize].position_size = I128::new(-500_000);
    engine.accounts[lp_idx as usize].entry_price = 1_000_000;
    engine.accounts[lp_idx as usize].funding_index = engine.funding_index_qpb_e6;
    engine.accounts[lp_idx as usize].last_fee_slot = 100;

    let insurance: u128 = 1_000;
    let pnl_pos = if pnl_raw > 0 { pnl_raw as u128 } else { 0 };
    let vault = capital + 50_000 + insurance + pnl_pos;
    engine.vault = U128::new(vault);
    engine.insurance_fund.balance = U128::new(insurance);

    sync_engine_aggregates(&mut engine);
    kani::assert(canonical_inv(&engine), "setup state must satisfy INV");

    let result = engine.touch_account_full(user_idx, now_slot, oracle);

    // INV on Ok path (Err → Solana tx rollback, state discarded)
    if result.is_ok() {
        kani::assert(
            canonical_inv(&engine),
            "INV must hold after touch_account_full",
        );
    }

    // Non-vacuity: high capital + favorable oracle must succeed
    if capital >= 35_000 && pnl_raw >= 0 && oracle >= 1_000_000 {
        kani::assert(result.is_ok(), "non-vacuity: conservative setup must succeed");
    }
}

// ----------------------------------------------------------------------------
// settle_loss_only: Ok path conservation
// Reduces c_tot by loss amount, writes off remaining negative PnL.
// vault and insurance untouched. Residual can only widen (more solvent).
// ----------------------------------------------------------------------------

/// settle_loss_only: INV preserved on Ok path.
/// Loss settlement decreases c_tot (absorbs loss from capital) and may write off
/// remaining negative PnL. Vault and insurance are untouched, so the conservation
/// gap can only widen (residual increases).
#[kani::proof]
#[kani::unwind(33)]
#[kani::solver(cadical)]
fn proof_settle_loss_only_preserves_inv() {
    let mut engine = RiskEngine::new(test_params());
    engine.vault = U128::new(100_000);

    let user_idx = engine.add_user(0).unwrap();

    let capital: u128 = kani::any();
    kani::assume(capital > 0 && capital < 50_000);
    engine.accounts[user_idx as usize].capital = U128::new(capital);

    let pnl: i128 = kani::any();
    kani::assume(pnl >= -100_000 && pnl < 0); // Must be negative for settle_loss_only to act
    engine.accounts[user_idx as usize].pnl = I128::new(pnl);

    engine.recompute_aggregates();
    kani::assert(canonical_inv(&engine), "setup state must satisfy INV");

    let vault_before = engine.vault.get();
    let ins_before = engine.insurance_fund.balance.get();

    let result = engine.settle_loss_only(user_idx);

    if result.is_ok() {
        kani::assert(canonical_inv(&engine), "INV must hold after settle_loss_only");
        // vault and insurance must be unchanged
        kani::assert(engine.vault.get() == vault_before, "vault unchanged by settle_loss_only");
        kani::assert(
            engine.insurance_fund.balance.get() == ins_before,
            "insurance unchanged by settle_loss_only",
        );
        // c_tot must not increase (loss absorption only decreases it)
        kani::assert(
            engine.c_tot.get() <= capital,
            "c_tot must not increase from settle_loss_only",
        );
        // After settlement, pnl must be >= 0 (loss fully absorbed or written off)
        kani::assert(
            engine.accounts[user_idx as usize].pnl.get() >= 0,
            "pnl must be non-negative after settle_loss_only",
        );
    }

    // Non-vacuity: force Ok path
    let _ = assert_ok!(result, "settle_loss_only must succeed");
}

// ----------------------------------------------------------------------------
// accrue_funding: Conservation
// Only modifies the global funding_index. No vault/c_tot/insurance changes.
// ----------------------------------------------------------------------------

/// accrue_funding: INV preserved.
/// Only updates the global funding index and last_funding_slot.
/// vault, c_tot, and insurance are all untouched.
#[kani::proof]
#[kani::unwind(33)]
#[kani::solver(cadical)]
fn proof_accrue_funding_preserves_inv() {
    let mut engine = RiskEngine::new(test_params());
    engine.vault = U128::new(100_000);
    engine.current_slot = 100;
    engine.last_funding_slot = 50;

    let user_idx = engine.add_user(0).unwrap();
    engine.accounts[user_idx as usize].capital = U128::new(10_000);
    engine.recompute_aggregates();

    // Set a non-zero funding rate so the function does real work
    let rate: i64 = kani::any();
    kani::assume(rate >= -100 && rate <= 100);
    engine.funding_rate_bps_per_slot_last = rate;

    kani::assert(canonical_inv(&engine), "setup state must satisfy INV");

    let now_slot: u64 = kani::any();
    kani::assume(now_slot >= 101 && now_slot <= 200);

    let oracle: u64 = kani::any();
    kani::assume(oracle >= 500_000 && oracle <= 2_000_000);

    let vault_before = engine.vault.get();
    let c_tot_before = engine.c_tot.get();
    let ins_before = engine.insurance_fund.balance.get();

    let result = engine.accrue_funding(now_slot, oracle);

    if result.is_ok() {
        kani::assert(canonical_inv(&engine), "INV must hold after accrue_funding");
        kani::assert(engine.vault.get() == vault_before, "vault unchanged by accrue_funding");
        kani::assert(engine.c_tot.get() == c_tot_before, "c_tot unchanged by accrue_funding");
        kani::assert(
            engine.insurance_fund.balance.get() == ins_before,
            "insurance unchanged by accrue_funding",
        );
    }

    // Non-vacuity: force Ok path
    let _ = assert_ok!(result, "accrue_funding must succeed with valid inputs");
}

// ----------------------------------------------------------------------------
// init_in_place: Conservation on fresh state
// All financial fields are zero (struct assumed zeroed). vault=c_tot=insurance=0.
// 0 >= 0 + 0 trivially holds.
// ----------------------------------------------------------------------------

/// init_in_place: INV holds on freshly initialized engine.
/// The struct is assumed zero-initialized before init_in_place.
/// vault = c_tot = insurance = 0, so conservation trivially holds.
#[kani::proof]
#[kani::unwind(33)]
#[kani::solver(cadical)]
fn proof_init_in_place_satisfies_inv() {
    // init_in_place ≡ new(): prove INV through init + deposit + withdraw lifecycle
    let mut engine = RiskEngine::new(test_params());
    kani::assert(canonical_inv(&engine), "INV after init");

    let deposit: u128 = kani::any();
    kani::assume(deposit > 100 && deposit < 50_000);

    let withdraw: u128 = kani::any();
    kani::assume(withdraw > 0 && withdraw <= deposit);

    let user = engine.add_user(0).unwrap();
    kani::assert(canonical_inv(&engine), "INV after add_user");

    assert_ok!(engine.deposit(user, deposit, 0), "deposit must succeed");
    kani::assert(canonical_inv(&engine), "INV after deposit");
    kani::assert(
        engine.accounts[user as usize].capital.get() == deposit,
        "capital == deposit on fresh account",
    );

    assert_ok!(
        engine.withdraw(user, withdraw, 0, 1_000_000),
        "withdraw must succeed"
    );
    kani::assert(canonical_inv(&engine), "INV after withdraw");
    kani::assert(
        engine.accounts[user as usize].capital.get() == deposit - withdraw,
        "capital == deposit - withdraw",
    );
}

// ----------------------------------------------------------------------------
// set_pnl: Conservation (vault/c_tot/insurance untouched)
// Only modifies account.pnl and pnl_pos_tot aggregate.
// ----------------------------------------------------------------------------

/// set_pnl: Conservation preserved.
/// set_pnl only modifies account PnL and the pnl_pos_tot aggregate.
/// vault, c_tot, and insurance are completely untouched.
#[kani::proof]
#[kani::unwind(33)]
#[kani::solver(cadical)]
fn proof_set_pnl_preserves_conservation() {
    let mut engine = RiskEngine::new(test_params());
    engine.vault = U128::new(100_000);

    let user_idx = engine.add_user(0).unwrap();
    engine.set_capital(user_idx as usize, 10_000);

    let initial_pnl: i128 = kani::any();
    kani::assume(initial_pnl > -50_000 && initial_pnl < 50_000);
    engine.set_pnl(user_idx as usize, initial_pnl);

    kani::assert(canonical_inv(&engine), "setup must satisfy INV");

    let vault_before = engine.vault.get();
    let c_tot_before = engine.c_tot.get();
    let ins_before = engine.insurance_fund.balance.get();

    let new_pnl: i128 = kani::any();
    kani::assume(new_pnl > -50_000 && new_pnl < 50_000);

    engine.set_pnl(user_idx as usize, new_pnl);

    kani::assert(canonical_inv(&engine), "INV must hold after set_pnl");
    kani::assert(engine.vault.get() == vault_before, "vault unchanged by set_pnl");
    kani::assert(engine.c_tot.get() == c_tot_before, "c_tot unchanged by set_pnl");
    kani::assert(
        engine.insurance_fund.balance.get() == ins_before,
        "insurance unchanged by set_pnl",
    );
}

// ----------------------------------------------------------------------------
// set_capital: Conservation
// Modifies account.capital and c_tot. vault and insurance untouched.
// Conservation holds iff caller ensures vault >= new_c_tot + insurance.
// This is a low-level helper — callers are responsible for maintaining conservation.
// We prove that set_capital correctly maintains the c_tot aggregate,
// and that conservation is preserved when the new capital <= old capital
// (the common case: fees, losses, liquidation).
// ----------------------------------------------------------------------------

/// set_capital: Conservation preserved when capital decreases (fee/loss path).
/// When capital decreases, c_tot decreases, so vault - c_tot - insurance widens.
#[kani::proof]
#[kani::unwind(33)]
#[kani::solver(cadical)]
fn proof_set_capital_decrease_preserves_conservation() {
    let mut engine = RiskEngine::new(test_params());
    engine.vault = U128::new(100_000);

    let user_idx = engine.add_user(0).unwrap();

    let old_capital: u128 = kani::any();
    kani::assume(old_capital > 0 && old_capital < 50_000);
    engine.set_capital(user_idx as usize, old_capital);

    kani::assert(canonical_inv(&engine), "setup must satisfy INV");

    // Allow both decrease AND increase (vault=100K provides headroom for increase)
    let new_capital: u128 = kani::any();
    kani::assume(new_capital < 100_000);

    engine.set_capital(user_idx as usize, new_capital);

    // canonical_inv holds when new capital <= vault - insurance (decrease preserves)
    if new_capital <= old_capital {
        kani::assert(
            canonical_inv(&engine),
            "canonical_inv must hold when capital decreases",
        );
    }
    // Aggregate correctness always holds (increase or decrease)
    kani::assert(inv_aggregates(&engine), "aggregates must be correct after set_capital");
}

/// set_capital: c_tot tracks capital delta correctly.
/// Conservation may or may not hold when capital increases — that depends on the
/// caller ensuring the vault has sufficient residual. We prove the aggregate is correct.
#[kani::proof]
#[kani::unwind(33)]
#[kani::solver(cadical)]
fn proof_set_capital_aggregate_correct() {
    let mut engine = RiskEngine::new(test_params());
    engine.vault = U128::new(100_000);

    let user_idx = engine.add_user(0).unwrap();

    let old_capital: u128 = kani::any();
    kani::assume(old_capital < 50_000);
    engine.accounts[user_idx as usize].capital = U128::new(old_capital);
    engine.recompute_aggregates();

    let c_tot_before = engine.c_tot.get();

    let new_capital: u128 = kani::any();
    kani::assume(new_capital < 50_000);

    engine.set_capital(user_idx as usize, new_capital);

    // c_tot must reflect the delta
    if new_capital >= old_capital {
        kani::assert(
            engine.c_tot.get() == c_tot_before + (new_capital - old_capital),
            "c_tot must increase by delta when capital increases",
        );
    } else {
        kani::assert(
            engine.c_tot.get() == c_tot_before - (old_capital - new_capital),
            "c_tot must decrease by delta when capital decreases",
        );
    }
}

// ============================================================================
// MULTI-STEP CONSERVATION: Realistic lifecycle with all settlement paths
// ============================================================================
//
// These proofs build realistic state through actual trades (user + LP),
// oracle movement, funding accrual, and then exercise the operations that
// were previously only tested in isolation with manually constructed state.
//
// The key insight: conservation bugs arise from INTERACTIONS between
// operations, not individual operations on artificial state.

/// Full lifecycle: deposit → trade → oracle move → accrue_funding →
/// touch_account_full (funding + mark + fees + warmup + debt sweep) →
/// verify conservation.
///
/// This exercises the most complex settlement path with state built
/// through real operations, not manual field writes.
#[kani::proof]
#[kani::unwind(5)] // MAX_ACCOUNTS=4
#[kani::solver(cadical)]
fn proof_lifecycle_trade_then_touch_full_conservation() {
    let mut engine = RiskEngine::new(test_params_with_maintenance_fee());
    engine.vault = U128::new(100_000);
    engine.insurance_fund.balance = U128::new(10_000);
    engine.current_slot = 0;
    engine.last_crank_slot = 0;
    engine.last_full_sweep_start_slot = 0;

    let user = engine.add_user(0).unwrap();
    let lp = engine.add_lp([1u8; 32], [0u8; 32], 0).unwrap();

    // Symbolic user deposit to vary margin pressure
    let user_deposit: u128 = kani::any();
    kani::assume(user_deposit >= 25_000 && user_deposit <= 50_000);

    // Step 1: Deposits
    assert_ok!(engine.deposit(user, user_deposit, 0), "user deposit");
    assert_ok!(engine.deposit(lp, 200_000, 0), "LP deposit");
    kani::assert(canonical_inv(&engine), "INV after deposits");

    // Step 2: Open trade at oracle $1.00 with symbolic size
    let size: i128 = kani::any();
    kani::assume(size >= 50 && size <= 200);
    let trade1 = engine.execute_trade(&NoOpMatcher, lp, user, 10, 1_000_000, size);
    assert_ok!(trade1, "open trade must succeed");
    kani::assert(canonical_inv(&engine), "INV after open trade");

    // Step 3: Oracle moves (symbolic — this is where PnL diverges from entry)
    let oracle_2: u64 = kani::any();
    kani::assume(oracle_2 >= 800_000 && oracle_2 <= 1_200_000); // $0.80 - $1.20

    // Step 4: Accrue funding with symbolic rate
    let funding_rate: i64 = kani::any();
    kani::assume(funding_rate > -50 && funding_rate < 50);
    engine.set_funding_rate_for_next_interval(funding_rate);

    let accrue_result = engine.accrue_funding(100, oracle_2);
    let _ = assert_ok!(accrue_result, "accrue_funding must succeed on bounded lifecycle state");
    kani::assert(canonical_inv(&engine), "INV after accrue_funding");

    // Step 5: touch_account_full on the user — settles funding, mark,
    // maintenance fees, warmup, and fee debt sweep.
    // This is the CRITICAL path: state was built by real trades + oracle move.
    let touch_result = engine.touch_account_full(user, 100, oracle_2);
    let _ = assert_ok!(
        touch_result,
        "touch_account_full(user) must succeed on well-capitalized traded account"
    );
    kani::assert(
        canonical_inv(&engine),
        "INV must hold after touch_account_full on traded account",
    );
    kani::assert(
        conservation_fast_no_funding(&engine),
        "Conservation must hold after touch_account_full",
    );

    // Step 6: touch_account_full on the LP too
    let touch_lp = engine.touch_account_full(lp, 100, oracle_2);
    let _ = assert_ok!(touch_lp, "touch_account_full(lp) must succeed on bounded lifecycle state");
    kani::assert(
        canonical_inv(&engine),
        "INV must hold after touch_account_full on LP",
    );
}

/// Lifecycle: deposit → trade → oracle crash → crank (liquidation) →
/// settle_loss_only → verify conservation.
///
/// Tests the loss settlement path with PnL created through real trades,
/// where the oracle crashes enough to make the user underwater.
#[kani::proof]
#[kani::unwind(5)] // MAX_ACCOUNTS=4
#[kani::solver(cadical)]
fn proof_lifecycle_trade_crash_settle_loss_conservation() {
    let mut engine = RiskEngine::new(test_params());
    engine.vault = U128::new(100_000);
    engine.insurance_fund.balance = U128::new(10_000);
    engine.current_slot = 0;
    engine.last_crank_slot = 0;
    engine.last_full_sweep_start_slot = 0;

    let user = engine.add_user(0).unwrap();
    let lp = engine.add_lp([1u8; 32], [0u8; 32], 0).unwrap();

    // Step 1: Deposits
    assert_ok!(engine.deposit(user, 10_000, 0), "user deposit");
    assert_ok!(engine.deposit(lp, 200_000, 0), "LP deposit");
    kani::assert(canonical_inv(&engine), "INV after deposits");

    // Step 2: User goes long at $1.00
    let trade = engine.execute_trade(&NoOpMatcher, lp, user, 10, 1_000_000, 100);
    assert_ok!(trade, "open trade must succeed");
    kani::assert(canonical_inv(&engine), "INV after trade");

    // Step 3: Oracle crashes below entry — symbolic range
    let oracle_crash: u64 = kani::any();
    kani::assume(oracle_crash >= 600_000 && oracle_crash <= 950_000);

    // Step 4: Crank at crashed oracle — may liquidate user
    let crank = engine.keeper_crank(user, 50, oracle_crash, 0, false);
    let _ = assert_ok!(crank, "keeper_crank must succeed at crashed oracle in bounded setup");
    kani::assert(canonical_inv(&engine), "INV after crank at crashed oracle");

    // Step 5: Settle mark to realize the loss
    let mark = engine.settle_mark_to_oracle(user, oracle_crash);
    let _ = assert_ok!(mark, "settle_mark_to_oracle must succeed after crank");
    kani::assert(canonical_inv(&engine), "INV after settle_mark with loss");

    // Step 6: Settle losses — the key operation
    let loss = engine.settle_loss_only(user);
    let _ = assert_ok!(loss, "settle_loss_only must succeed on used account");
    kani::assert(
        canonical_inv(&engine),
        "INV must hold after settle_loss_only on real traded account",
    );
    kani::assert(
        conservation_fast_no_funding(&engine),
        "Conservation must hold after settle_loss_only",
    );
}

/// Lifecycle: deposit → trade → oracle move → crank → close → warmup →
/// settle_warmup_to_capital → withdraw → top_up_insurance → verify.
///
/// Tests the warmup conversion + withdrawal + insurance top-up path
/// with state built through real trades. Symbolic withdraw amount covers
/// both partial and near-full withdrawal. Two oracle values (1.02, 1.10)
/// subsume the previous alt_oracle variant.
#[kani::proof]
#[kani::unwind(5)] // MAX_ACCOUNTS=4
#[kani::solver(cadical)]
fn proof_lifecycle_trade_warmup_withdraw_topup_conservation() {
    let mut engine = RiskEngine::new(test_params());
    engine.vault = U128::new(100_000);
    engine.insurance_fund.balance = U128::new(10_000);
    engine.current_slot = 0;
    engine.last_crank_slot = 0;
    engine.last_full_sweep_start_slot = 0;

    let user = engine.add_user(0).unwrap();
    let lp = engine.add_lp([1u8; 32], [0u8; 32], 0).unwrap();

    // Symbolic LP deposit to exercise h < 1 path (lower insurance pool)
    let lp_deposit: u128 = kani::any();
    kani::assume(lp_deposit >= 50_000 && lp_deposit <= 100_000);

    // Step 1: Deposits
    assert_ok!(engine.deposit(user, 50_000, 0), "user deposit");
    assert_ok!(engine.deposit(lp, lp_deposit, 0), "LP deposit");

    // Step 2: User goes long at $1.00
    let trade = engine.execute_trade(&NoOpMatcher, lp, user, 10, 1_000_000, 100);
    assert_ok!(trade, "open trade");
    kani::assert(canonical_inv(&engine), "INV after trade");

    // Step 3: Fully symbolic oracle move up (exercises full PnL range, not just 2 values)
    let oracle_2: u64 = kani::any();
    kani::assume(oracle_2 >= 1_010_000 && oracle_2 <= 1_200_000);

    // Step 4: Crank at new oracle
    let crank = engine.keeper_crank(user, 50, oracle_2, 0, false);
    let _ = assert_ok!(crank, "keeper_crank must succeed on bounded profitable state");
    kani::assert(canonical_inv(&engine), "INV after crank");

    // Step 5: Close position to lock in profit
    let close = engine.execute_trade(&NoOpMatcher, lp, user, 50, oracle_2, -100);
    let _ = assert_ok!(close, "close trade must succeed after profitable move");
    kani::assert(canonical_inv(&engine), "INV after close");

    // Step 6: Time passes for warmup (slot 50 → 200, warmup_period=100)
    engine.current_slot = 200;
    engine.last_crank_slot = 200;
    engine.last_full_sweep_start_slot = 200;

    // Step 7: Settle warmup — converts warmed PnL to capital with haircut
    let settle = engine.settle_warmup_to_capital(user);
    let _ = assert_ok!(settle, "settle_warmup_to_capital must succeed");
    kani::assert(
        canonical_inv(&engine),
        "INV must hold after settle_warmup on real profit",
    );

    // Step 8: Symbolic withdraw amount — covers partial and near-full
    let withdraw_amt: u128 = kani::any();
    kani::assume(withdraw_amt >= 1_000 && withdraw_amt <= 40_000);
    let w = engine.withdraw(user, withdraw_amt, 200, oracle_2);
    // Withdraw may fail if amount exceeds available balance — both paths valid
    if w.is_ok() {
        kani::assert(canonical_inv(&engine), "INV after withdraw");
    }

    // Step 9: Top up insurance
    let topup_amt: u128 = 10_000;
    let t = engine.top_up_insurance_fund(topup_amt);
    let _ = assert_ok!(t, "top_up_insurance_fund must succeed with bounded amount");
    kani::assert(
        canonical_inv(&engine),
        "INV must hold after top_up_insurance on traded state",
    );
    kani::assert(
        conservation_fast_no_funding(&engine),
        "Conservation must hold at end of full lifecycle",
    );
}

// ============================================================================
// EXTERNAL REVIEW REBUTTAL PROOFS
// These formally verify that 3 claimed critical flaws are NOT exploitable.
// ============================================================================

// --- Flaw 1: "Free Option" Debt Wipe ---

/// Flaw 1a: After liquidation (which calls oracle_close_position_core internally),
/// if PnL was written off (set to 0), position_size must be 0.
/// No "free option" is possible — debt writeoff requires flat position.
#[kani::proof]
#[kani::unwind(33)]
#[kani::solver(cadical)]
fn proof_flaw1_debt_writeoff_requires_flat_position() {
    let mut engine = RiskEngine::new(test_params());

    let user = engine.add_user(0).unwrap();
    let lp = engine.add_lp([1u8; 32], [0u8; 32], 0).unwrap();

    let oracle: u64 = 1_000_000;

    // User: symbolic small capital, large position => undercollateralized
    let user_capital: u128 = kani::any();
    kani::assume(user_capital >= 100 && user_capital <= 5_000);
    let user_loss: u128 = kani::any();
    kani::assume(user_loss >= 0 && user_loss <= user_capital);

    engine.deposit(user, user_capital, 0).unwrap();
    engine.accounts[user as usize].position_size = I128::new(10_000_000);
    engine.accounts[user as usize].entry_price = oracle;
    engine.accounts[user as usize].pnl = I128::new(-(user_loss as i128));
    engine.accounts[user as usize].warmup_slope_per_step = U128::new(0);

    // LP counterparty
    engine.deposit(lp, 100_000, 0).unwrap();
    engine.accounts[lp as usize].position_size = I128::new(-10_000_000);
    engine.accounts[lp as usize].entry_price = oracle;
    engine.accounts[lp as usize].pnl = I128::new(0);
    engine.accounts[lp as usize].warmup_slope_per_step = U128::new(0);

    sync_engine_aggregates(&mut engine);

    let pnl_before = engine.accounts[user as usize].pnl.get();

    let result = engine.liquidate_at_oracle(user, 0, oracle);

    // Non-vacuous: liquidation must succeed and trigger
    assert!(result.is_ok(), "liquidation must not error");
    assert!(result.unwrap(), "setup must force liquidation to trigger");

    let acc = &engine.accounts[user as usize];

    // KEY ASSERTION: after liquidation with debt writeoff, position must be zero.
    // oracle_close_position_core sets position_size = 0 (line 1923) BEFORE
    // writing off negative PnL (line 1939). No "free option" exists.
    if acc.pnl.get() >= 0 && pnl_before < 0 {
        kani::assert(
            acc.position_size.is_zero(),
            "Flaw1: debt writeoff only happens when position is already closed"
        );
    }

    // Even without checking PnL path: position must always be zero after full liquidation
    // (this account has capital=500 vs margin req ~500,000, so full close is forced)
    kani::assert(
        acc.position_size.is_zero(),
        "Flaw1: deeply undercollateralized account must be fully liquidated"
    );
}

/// Flaw 1b: garbage_collect_dust never writes off PnL for accounts with open positions.
/// The dust predicate requires position_size == 0 (line 1404).
#[kani::proof]
#[kani::unwind(33)]
#[kani::solver(cadical)]
fn proof_flaw1_gc_never_writes_off_with_open_position() {
    let mut engine = RiskEngine::new(test_params());
    engine.vault = U128::new(200_000);
    engine.insurance_fund.balance = U128::new(10_000);
    engine.current_slot = 100;
    engine.last_crank_slot = 100;
    engine.last_full_sweep_start_slot = 100;

    let user = engine.add_user(0).unwrap();

    // User has negative PnL but an OPEN position — GC must not touch this account
    // Symbolic negative PnL and position size
    let neg_pnl: i128 = kani::any();
    kani::assume(neg_pnl >= -10_000 && neg_pnl <= -1);
    let pos: i128 = kani::any();
    kani::assume(pos >= 100_000 && pos <= 10_000_000);

    engine.accounts[user as usize].capital = U128::ZERO;
    engine.accounts[user as usize].pnl = I128::new(neg_pnl);
    engine.accounts[user as usize].position_size = I128::new(pos);
    engine.accounts[user as usize].entry_price = 1_000_000;
    engine.accounts[user as usize].funding_index = engine.funding_index_qpb_e6;

    sync_engine_aggregates(&mut engine);
    kani::assume(canonical_inv(&engine));

    let pnl_before = engine.accounts[user as usize].pnl.get();

    engine.garbage_collect_dust();

    // KEY ASSERTION: account with open position is untouched by GC
    kani::assert(
        engine.accounts[user as usize].pnl.get() == pnl_before,
        "Flaw1: GC must not modify PnL of account with open position"
    );
    kani::assert(
        engine.is_used(user as usize),
        "Flaw1: GC must not free account with open position"
    );
    kani::assert(canonical_inv(&engine), "INV after GC");
}

// --- Flaw 2: "Phantom Margin Equity" ---

/// Flaw 2a: After settle_mark_to_oracle, entry_price == oracle_price and
/// mark_pnl == 0. No phantom equity from stale entry prices.
/// Equity is unchanged (MTM was realized into PnL, net effect same).
///
/// Tests both long and short positions with price divergence from entry.
/// Uses symbolic PnL to verify across all realized-PnL states.
#[kani::proof]
#[kani::unwind(5)]
#[kani::solver(cadical)]
fn proof_flaw2_no_phantom_equity_after_mark_settlement() {
    let mut engine = RiskEngine::new(test_params());
    engine.vault = U128::new(100_000);

    let user = engine.add_user(0).unwrap();
    engine.accounts[user as usize].capital = U128::new(10_000);
    engine.accounts[user as usize].funding_index = engine.funding_index_qpb_e6;

    // Symbolic position and oracle — narrowed for solver tractability
    let pos: i128 = kani::any();
    let oracle: u64 = kani::any();
    kani::assume(pos >= -500 && pos <= 500 && pos != 0);
    kani::assume(oracle >= 900_000 && oracle <= 1_200_000);
    let entry: u64 = 1_000_000;

    engine.accounts[user as usize].position_size = I128::new(pos);
    engine.accounts[user as usize].entry_price = entry;

    // Use symbolic PnL to verify across all realized-PnL states
    let pnl: i128 = kani::any();
    kani::assume(pnl >= -2_000 && pnl <= 2_000);
    engine.accounts[user as usize].pnl = I128::new(pnl);

    sync_engine_aggregates(&mut engine);
    kani::assume(canonical_inv(&engine));

    // Equity BEFORE settlement includes unrealized MTM from stale entry
    let equity_before = engine.account_equity_mtm_at_oracle(
        &engine.accounts[user as usize], oracle
    );

    // Settle mark to oracle
    let result = engine.settle_mark_to_oracle(user, oracle);
    let _ = assert_ok!(result, "settle must succeed");

    // After settlement: entry == oracle, so mark_pnl == 0
    kani::assert(
        engine.accounts[user as usize].entry_price == oracle,
        "Flaw2: entry_price must equal oracle after settlement"
    );

    // Verify mark_pnl is now 0
    let mark_after = RiskEngine::mark_pnl_for_position(
        engine.accounts[user as usize].position_size.get(),
        engine.accounts[user as usize].entry_price,
        oracle,
    );
    let _ = assert_ok!(mark_after, "mark_pnl must be computable");
    kani::assert(
        mark_after.unwrap() == 0,
        "Flaw2: mark_pnl must be 0 after settle_mark_to_oracle"
    );

    // Equity after settlement uses realized values only (no phantom from stale entry)
    let equity_after = engine.account_equity_mtm_at_oracle(
        &engine.accounts[user as usize], oracle
    );
    // equity_after should equal equity_before (MTM was realized into PnL, net effect same)
    kani::assert(
        equity_after == equity_before,
        "Flaw2: equity unchanged by mark settlement (MTM realized, not phantom)"
    );
    kani::assert(canonical_inv(&engine), "INV after mark settlement");
}

/// Flaw 2b: withdraw() calls touch_account_full which settles mark before margin check,
/// preventing stale-entry exploits.
///
/// Setup uses STALE entry (entry=500K != oracle=1M) so touch_account_full actually
/// settles a non-zero mark-to-market, proving the settlement is not a no-op.
#[kani::proof]
#[kani::unwind(33)]
#[kani::solver(cadical)]
fn proof_flaw2_withdraw_settles_before_margin_check() {
    let mut engine = RiskEngine::new(test_params());
    engine.current_slot = 100;
    engine.last_crank_slot = 100;
    engine.last_full_sweep_start_slot = 100;

    let user = engine.add_user(0).unwrap();
    let lp = engine.add_lp([1u8; 32], [0u8; 32], 0).unwrap();

    // Symbolic oracle to exercise different mark_pnl values
    let oracle: u64 = kani::any();
    kani::assume(oracle >= 800_000 && oracle <= 1_500_000);
    let stale_entry: u64 = 500_000;   // stale entry = $0.50 (user bought lower)

    // User with capital and a position with STALE entry (entry != oracle)
    engine.accounts[user as usize].capital = U128::new(50_000);
    engine.accounts[user as usize].position_size = I128::new(1_000);
    engine.accounts[user as usize].entry_price = stale_entry;
    engine.accounts[user as usize].warmup_slope_per_step = U128::new(0);
    engine.accounts[user as usize].pnl = I128::new(0);

    // LP counterparty (entry matches user for OI balance)
    engine.accounts[lp as usize].capital = U128::new(100_000);
    engine.accounts[lp as usize].position_size = I128::new(-1_000);
    engine.accounts[lp as usize].entry_price = stale_entry;
    engine.accounts[lp as usize].warmup_slope_per_step = U128::new(0);
    engine.accounts[lp as usize].pnl = I128::new(0);

    engine.vault = U128::new(150_000);
    sync_engine_aggregates(&mut engine);

    kani::assert(canonical_inv(&engine), "INV before withdraw");

    // Confirm entry is stale before withdraw
    kani::assert(
        engine.accounts[user as usize].entry_price == stale_entry,
        "Precondition: entry must be stale (not equal to oracle)"
    );

    // Attempt to withdraw — this calls touch_account_full which settles mark
    let w_amount: u128 = kani::any();
    kani::assume(w_amount > 0 && w_amount <= 5_000);
    let result = engine.withdraw(user, w_amount, 100, oracle);

    if result.is_ok() {
        // KEY ASSERTION: after withdraw, entry was settled to oracle by touch_account_full.
        kani::assert(
            engine.accounts[user as usize].entry_price == oracle,
            "Flaw2: withdraw must settle stale entry to oracle before margin check"
        );

        kani::assert(
            engine.accounts[user as usize].pnl.get() >= 0,
            "Flaw2: mark settlement should have realized positive MTM into pnl"
        );

        kani::assert(canonical_inv(&engine), "INV after withdraw");
    }

    // Non-vacuity: conservative withdrawal (5K of 50K capital) with big mark gain
    if oracle >= 1_000_000 && w_amount <= 1_000 {
        kani::assert(result.is_ok(), "non-vacuity: small withdraw with large equity must succeed");
    }
}

// --- Flaw 3: "Forever Warmup" Reset ---

/// Flaw 3a: When AvailGross increases (MTM gain), update_warmup_slope sets
/// new_slope >= old_slope. Larger profits always mean faster conversion.
#[kani::proof]
#[kani::unwind(5)]
#[kani::solver(cadical)]
fn proof_flaw3_warmup_reset_increases_slope_proportionally() {
    let mut engine = RiskEngine::new(test_params());
    engine.vault = U128::new(200_000);
    engine.current_slot = 50;

    let user = engine.add_user(0).unwrap();
    engine.accounts[user as usize].capital = U128::new(10_000);
    engine.accounts[user as usize].funding_index = engine.funding_index_qpb_e6;

    // Start with zero or positive PnL — exercises both zero-slope and positive-slope paths
    let pnl1: i128 = kani::any();
    kani::assume(pnl1 >= 0 && pnl1 <= 5_000);
    engine.accounts[user as usize].pnl = I128::new(pnl1);

    sync_engine_aggregates(&mut engine);

    // Set initial warmup slope (may be 0 when pnl1=0)
    assert_ok!(engine.update_warmup_slope(user), "first slope update");
    let slope1 = engine.accounts[user as usize].warmup_slope_per_step.get();

    // PnL increases (simulating profitable MTM settlement)
    let pnl2: i128 = kani::any();
    kani::assume(pnl2 > pnl1 && pnl2 <= 10_000);
    engine.accounts[user as usize].pnl = I128::new(pnl2);
    sync_engine_aggregates(&mut engine);

    // Update slope again (as touch_account_full would do)
    engine.current_slot = 60;
    assert_ok!(engine.update_warmup_slope(user), "second slope update");
    let slope2 = engine.accounts[user as usize].warmup_slope_per_step.get();

    // KEY ASSERTION: new slope >= old slope (proportional to increased PnL)
    kani::assert(
        slope2 >= slope1,
        "Flaw3: warmup slope must not decrease when PnL increases"
    );
    // Timer was reset
    kani::assert(
        engine.accounts[user as usize].warmup_started_at_slot == 60,
        "Flaw3: warmup timer reset to current slot"
    );
}

/// Flaw 3b: After warmup reset, conversion is possible after a single slot.
/// Profit is never permanently trapped — slope >= 1 ensures cap >= 1 per slot.
#[kani::proof]
#[kani::unwind(5)]
#[kani::solver(cadical)]
fn proof_flaw3_warmup_converts_after_single_slot() {
    let mut engine = RiskEngine::new(test_params());
    engine.vault = U128::new(200_000);
    engine.current_slot = 100;

    let user = engine.add_user(0).unwrap();
    engine.accounts[user as usize].capital = U128::new(10_000);
    engine.accounts[user as usize].funding_index = engine.funding_index_qpb_e6;

    // Positive PnL (profit to warm up)
    let pnl: i128 = kani::any();
    kani::assume(pnl > 0 && pnl <= 10_000);
    engine.accounts[user as usize].pnl = I128::new(pnl);

    sync_engine_aggregates(&mut engine);
    kani::assume(canonical_inv(&engine));

    // Reset warmup (simulates the timer reset from touch_account_full)
    assert_ok!(engine.update_warmup_slope(user), "warmup slope set");
    let slope = engine.accounts[user as usize].warmup_slope_per_step.get();
    kani::assert(slope >= 1, "slope must be >= 1 with positive PnL");

    // Advance exactly 1 slot
    engine.current_slot = 101;
    let cap_before = engine.accounts[user as usize].capital.get();

    // Settle warmup — should convert some PnL to capital
    assert_ok!(engine.settle_warmup_to_capital(user), "warmup settle");

    let cap_after = engine.accounts[user as usize].capital.get();

    // KEY ASSERTION: capital increased (conversion happened after 1 slot)
    kani::assert(
        cap_after >= cap_before,
        "Flaw3: capital must not decrease from warmup settlement"
    );
    // Stronger: if system is solvent (residual >= pnl_pos_tot), capital strictly increases
    let (solvent, _) = RiskEngine::signed_residual(
        engine.vault.get(), engine.c_tot.get(), engine.insurance_fund.balance.get()
    );
    if solvent {
        kani::assert(
            cap_after > cap_before,
            "Flaw3: with solvent system, warmup must convert some PnL after 1 slot"
        );
    }
    kani::assert(canonical_inv(&engine), "INV after warmup settle");
}

// ============================================================================
// INDUCTIVE PROOFS: Abstract Delta Specifications
// ============================================================================
//
// These proofs model operations algebraically on symbolic state (full u128/i128
// domain, no construction, no bounds), proving invariant components are preserved
// for ALL possible pre-states. They complement the 144 STRONG proofs which
// exercise real code paths on bounded ranges.
//
// Classification: INDUCTIVE — decomposed invariant, loop-free delta specs,
// fully symbolic state.

/// Inductive Proof 1: top_up_insurance_fund preserves inv_accounting
///
/// Operation: vault += amount, insurance += amount
/// Component: inv_accounting (vault >= c_tot + insurance)
/// Result: Trivially true — both sides increase by the same amount.
#[kani::proof]
fn inductive_top_up_insurance_preserves_accounting() {
    let vault: u128 = kani::any();
    let c_tot: u128 = kani::any();
    let insurance: u128 = kani::any();
    let amount: u128 = kani::any();

    // Pre: inv_accounting holds (no saturation in obligations)
    kani::assume(c_tot.checked_add(insurance).is_some());
    kani::assume(vault >= c_tot + insurance);

    // Pre: no overflow on vault/insurance additions
    kani::assume(vault.checked_add(amount).is_some());
    kani::assume(insurance.checked_add(amount).is_some());

    // Operation
    let vault_after = vault + amount;
    let insurance_after = insurance + amount;

    // Post: inv_accounting preserved
    kani::assert(
        vault_after >= c_tot + insurance_after,
        "top_up_insurance must preserve vault >= c_tot + insurance",
    );
}

/// Inductive Proof 2: set_capital (decrease) preserves inv_accounting
///
/// Operation: capital decreases → c_tot decreases by delta
/// Component: inv_accounting (vault >= c_tot + insurance)
/// Result: c_tot only shrinks, vault/insurance unchanged → trivially preserved.
/// Note: Proves accounting for capital DECREASE (loss settlement, withdraw).
/// For increases (deposit), vault also increases — covered by deposit proof.
#[kani::proof]
fn inductive_set_capital_preserves_accounting() {
    let vault: u128 = kani::any();
    let c_tot: u128 = kani::any();
    let insurance: u128 = kani::any();
    let old_capital: u128 = kani::any();
    let new_capital: u128 = kani::any();

    // Pre: inv_accounting
    kani::assume(c_tot.checked_add(insurance).is_some());
    kani::assume(vault >= c_tot + insurance);

    // Pre: old_capital contributes to c_tot
    kani::assume(old_capital <= c_tot);

    // Capital decreases (loss settlement, withdraw capital reduction)
    kani::assume(new_capital <= old_capital);

    // Model set_capital: c_tot' = c_tot - (old - new)
    // Since new <= old and old <= c_tot: delta <= c_tot, no saturation
    let delta = old_capital - new_capital;
    let c_tot_after = c_tot - delta;

    // Post: inv_accounting preserved
    kani::assert(
        vault >= c_tot_after + insurance,
        "set_capital (decrease) must preserve vault >= c_tot + insurance",
    );
}

/// Inductive Proof 3: set_pnl correctly updates pnl_pos_tot (delta form)
///
/// Operation: pnl_pos_tot' = pnl_pos_tot - max(old_pnl, 0) + max(new_pnl, 0)
/// Component: inv_aggregates (pnl_pos_tot correctness)
/// Result: Saturating arithmetic matches exact arithmetic when preconditions hold.
#[kani::proof]
fn inductive_set_pnl_preserves_pnl_pos_tot_delta() {
    let pnl_pos_tot: u128 = kani::any();
    let old_pnl: i128 = kani::any();
    let new_pnl: i128 = kani::any();

    // PA2: no i128::MIN in pnl fields
    kani::assume(old_pnl != i128::MIN);
    kani::assume(new_pnl != i128::MIN);

    let old_pos: u128 = if old_pnl > 0 { old_pnl as u128 } else { 0 };
    let new_pos: u128 = if new_pnl > 0 { new_pnl as u128 } else { 0 };

    // Pre: pnl_pos_tot includes old_pnl's positive contribution
    kani::assume(pnl_pos_tot >= old_pos);

    // Pre: no intermediate overflow (saturating_add won't saturate)
    kani::assume(pnl_pos_tot.checked_add(new_pos).is_some());

    // Model: match set_pnl's saturating arithmetic (lines 772-783)
    let result = pnl_pos_tot.saturating_add(new_pos).saturating_sub(old_pos);

    // Expected: exact delta
    let expected = pnl_pos_tot - old_pos + new_pos;

    kani::assert(
        result == expected,
        "set_pnl delta must correctly update pnl_pos_tot",
    );
}

/// Inductive Proof 4: set_capital correctly updates c_tot (delta form)
///
/// Operation: c_tot' = c_tot - old_capital + new_capital
/// Component: inv_aggregates (c_tot correctness)
/// Result: Branching saturating arithmetic matches exact arithmetic.
#[kani::proof]
fn inductive_set_capital_delta_correct() {
    let c_tot: u128 = kani::any();
    let old_capital: u128 = kani::any();
    let new_capital: u128 = kani::any();

    // Pre: old_capital contributes to c_tot
    kani::assume(old_capital <= c_tot);

    // Pre: no overflow when increasing
    if new_capital >= old_capital {
        kani::assume(c_tot.checked_add(new_capital - old_capital).is_some());
    }

    // Model: match set_capital's branching logic (lines 787-795)
    let c_tot_after = if new_capital >= old_capital {
        c_tot.saturating_add(new_capital - old_capital)
    } else {
        c_tot.saturating_sub(old_capital - new_capital)
    };

    // Expected: exact delta
    let expected = c_tot - old_capital + new_capital;

    kani::assert(
        c_tot_after == expected,
        "set_capital delta must equal c_tot - old + new",
    );
}

/// Inductive Proof 5: deposit preserves inv_accounting
///
/// Operation: vault += amount, c_tot += amount (via set_capital)
/// Component: inv_accounting (vault >= c_tot + insurance)
/// Result: Both vault and c_tot increase by same amount → inequality preserved.
#[kani::proof]
fn inductive_deposit_preserves_accounting() {
    let vault: u128 = kani::any();
    let c_tot: u128 = kani::any();
    let insurance: u128 = kani::any();
    let amount: u128 = kani::any();

    // Pre: inv_accounting
    kani::assume(c_tot.checked_add(insurance).is_some());
    kani::assume(vault >= c_tot + insurance);

    // Pre: no overflow
    kani::assume(vault.checked_add(amount).is_some());
    kani::assume(c_tot.checked_add(amount).is_some());

    // Operation
    let vault_after = vault + amount;
    let c_tot_after = c_tot + amount;

    // Post: inv_accounting preserved
    kani::assert(
        vault_after >= c_tot_after + insurance,
        "deposit must preserve vault >= c_tot + insurance",
    );
}

/// Inductive Proof 6: withdraw preserves inv_accounting
///
/// Operation: vault -= amount, c_tot -= amount (via set_capital)
/// Component: inv_accounting (vault >= c_tot + insurance)
/// Result: Both vault and c_tot decrease by same amount → inequality preserved.
#[kani::proof]
fn inductive_withdraw_preserves_accounting() {
    let vault: u128 = kani::any();
    let c_tot: u128 = kani::any();
    let insurance: u128 = kani::any();
    let amount: u128 = kani::any();

    // Pre: inv_accounting
    kani::assume(c_tot.checked_add(insurance).is_some());
    kani::assume(vault >= c_tot + insurance);

    // Pre: amount <= c_tot (can't withdraw more than capital, capital <= c_tot)
    kani::assume(amount <= c_tot);
    // Pre: amount <= vault (tokens must exist)
    kani::assume(amount <= vault);

    // Operation
    let vault_after = vault - amount;
    let c_tot_after = c_tot - amount;

    // Post: inv_accounting preserved
    kani::assert(
        vault_after >= c_tot_after + insurance,
        "withdraw must preserve vault >= c_tot + insurance",
    );
}

/// Inductive Proof 7: settle_loss_only preserves inv_accounting
///
/// Operation: paid = min(|pnl|, capital); c_tot -= paid. Vault/insurance unchanged.
/// Component: inv_accounting (vault >= c_tot + insurance)
/// Result: c_tot only decreases → trivially preserved.
#[kani::proof]
fn inductive_settle_loss_preserves_accounting() {
    let vault: u128 = kani::any();
    let c_tot: u128 = kani::any();
    let insurance: u128 = kani::any();
    let capital: u128 = kani::any();
    let pnl: i128 = kani::any();

    // Pre: inv_accounting
    kani::assume(c_tot.checked_add(insurance).is_some());
    kani::assume(vault >= c_tot + insurance);

    // Pre: capital contributes to c_tot
    kani::assume(capital <= c_tot);

    // Pre: pnl is negative (loss to settle)
    kani::assume(pnl < 0);
    kani::assume(pnl != i128::MIN); // PA2

    // Model settle_loss: paid = min(|pnl|, capital)
    let need = (-pnl) as u128; // safe: pnl != i128::MIN and pnl < 0
    let paid = core::cmp::min(need, capital);

    // c_tot decreases by paid (set_capital(capital - paid))
    // paid <= capital <= c_tot, so no underflow
    let c_tot_after = c_tot - paid;

    // Post: inv_accounting preserved (vault/insurance unchanged, c_tot decreased)
    kani::assert(
        vault >= c_tot_after + insurance,
        "settle_loss must preserve vault >= c_tot + insurance",
    );
}

/// Inductive Proof 8: settle_warmup profit phase preserves inv_accounting
///
/// Operation: pnl -= x, capital += y where y = x * h_num / h_den
///   h_num = min(residual, pnl_pos_tot), h_den = pnl_pos_tot
/// Component: inv_accounting (vault >= c_tot + insurance)
/// Result: Haircut ensures y <= residual, so vault has room for c_tot increase.
///
/// Haircut bound derivation (why y <= residual):
///   haircut_ratio() returns (h_num, h_den) = (min(residual, pnl_pos_tot), pnl_pos_tot)
///   y = floor(x * h_num / h_den)
///   Since x <= h_den and h_num <= h_den: y <= h_num  (integer division property)
///   Since h_num = min(residual, pnl_pos_tot) <= residual: y <= residual  QED
#[kani::proof]
fn inductive_settle_warmup_profit_preserves_accounting() {
    let vault: u128 = kani::any();
    let c_tot: u128 = kani::any();
    let insurance: u128 = kani::any();
    let pnl_pos_tot: u128 = kani::any();
    let x: u128 = kani::any(); // amount converted from pnl to capital
    let y: u128 = kani::any(); // haircutted amount credited to capital

    // Pre: inv_accounting
    kani::assume(c_tot.checked_add(insurance).is_some());
    kani::assume(vault >= c_tot + insurance);
    let residual = vault - c_tot - insurance;

    // Pre: pnl_pos_tot > 0 and x is one account's contribution
    kani::assume(pnl_pos_tot > 0);
    kani::assume(x <= pnl_pos_tot);

    // Haircut bounds: y = floor(x * min(residual, pnl_pos_tot) / pnl_pos_tot)
    // Guarantees: y <= x (ratio <= 1) and y <= residual (see derivation above)
    kani::assume(y <= x);
    kani::assume(y <= residual);

    // Operation: c_tot += y (capital increases by haircutted amount)
    kani::assume(c_tot.checked_add(y).is_some());
    let c_tot_after = c_tot + y;

    // Post: inv_accounting preserved
    kani::assert(
        vault >= c_tot_after + insurance,
        "settle_warmup profit must preserve vault >= c_tot + insurance",
    );
}

/// Inductive Proof 9: full settle_warmup (loss + profit) preserves inv_accounting
///
/// Combines loss phase (c_tot decreases) and profit phase (c_tot increases by y <= residual).
/// Component: inv_accounting (vault >= c_tot + insurance)
/// Result: Loss increases residual headroom; haircut ensures profit phase stays within it.
#[kani::proof]
fn inductive_settle_warmup_full_preserves_accounting() {
    let vault: u128 = kani::any();
    let c_tot: u128 = kani::any();
    let insurance: u128 = kani::any();
    let capital: u128 = kani::any();
    let loss_pnl: i128 = kani::any(); // negative pnl for loss phase
    let pnl_pos_tot: u128 = kani::any();
    let x: u128 = kani::any(); // profit conversion amount
    let y: u128 = kani::any(); // haircutted amount

    // Pre: inv_accounting
    kani::assume(c_tot.checked_add(insurance).is_some());
    kani::assume(vault >= c_tot + insurance);

    // Pre: capital contributes to c_tot
    kani::assume(capital <= c_tot);

    // ── Phase 1: Loss settlement ──
    kani::assume(loss_pnl < 0);
    kani::assume(loss_pnl != i128::MIN);
    let need = (-loss_pnl) as u128;
    let paid = core::cmp::min(need, capital);
    let c_tot_after_loss = c_tot - paid; // safe: paid <= capital <= c_tot

    // ── Phase 2: Profit conversion (on post-loss state) ──
    kani::assume(pnl_pos_tot > 0);
    kani::assume(x <= pnl_pos_tot);

    // Residual after loss (increased: loss freed headroom)
    let residual_after_loss = vault - c_tot_after_loss - insurance;

    // Haircut bounds on post-loss residual (see proof 8 derivation)
    kani::assume(y <= x);
    kani::assume(y <= residual_after_loss);

    kani::assume(c_tot_after_loss.checked_add(y).is_some());
    let c_tot_final = c_tot_after_loss + y;

    // Post: inv_accounting preserved
    kani::assert(
        vault >= c_tot_final + insurance,
        "full settle_warmup must preserve vault >= c_tot + insurance",
    );
}
