# Kani Proof Strength Audit Results

Generated: 2026-02-18 (re-audit after strengthening 32 proofs, consolidating 9)

~149 proof harnesses across `/home/anatoly/percolator/tests/kani.rs`.

---

## Classification Summary

| Classification | Count | Description |
|---|---|---|
| STRONG | ~98 | Symbolic inputs exercise key branches, canonical_inv or appropriate invariant, non-vacuous |
| WEAK | ~25 | Symbolic but misses branches, uses weaker invariant, or has symbolic collapse |
| UNIT TEST | ~26 | Concrete inputs, single execution path (some intentionally so) |
| VACUOUS | 0 | All proofs have non-vacuity assertions or are trivially reachable |

---

## Changes Since Last Audit

### Session 1 (8 proofs)

| Proof | Before | After | Key Improvement |
|---|---|---|---|
| `proof_settle_warmup_preserves_inv` | UNIT TEST | **STRONG** | 8 symbolic inputs, all positive-PnL branches |
| `proof_settle_warmup_negative_pnl_immediate` | UNIT TEST | **STRONG** | 3 symbolic inputs, insolvency/writeoff paths |
| `proof_liquidate_preserves_inv` | UNIT TEST | **STRONG** | Symbolic capital+oracle, above/below margin |
| `proof_withdraw_preserves_inv` | WEAK | **STRONG** | Position added, symbolic capital/amount/oracle |
| `proof_deposit_preserves_inv` | WEAK | **STRONG** | Symbolic capital/pnl/amount/slot + maintenance fees |
| `proof_keeper_crank_preserves_inv` | WEAK | **WEAK** | Added positions/LP/fees/symbolic capital+slot (oracle still concrete) |
| `proof_touch_account_full_preserves_inv` | WEAK | **STRONG** | Symbolic capital/pnl/oracle/slot, undercollateralized reachable |
| `proof_lq7_symbolic_oracle_liquidation` | (new) | **STRONG** | Symbolic oracle exercises mark PnL during liquidation |

### Session 2: 15 WEAK → STRONG

| Proof | Key Improvement |
|---|---|
| `i5_warmup_determinism` | Symbolic negative PnL, PA1 constraint (reserved <= pnl_pos) |
| `funding_p3_bounded_drift` | Position widened to 10K, delta to 10K (was 100/1K) |
| `gc_never_frees_account_with_positive_value` | Added sync_engine_aggregates + canonical_inv check |
| `i7_user_isolation_deposit` | Symbolic operation amount |
| `i7_user_isolation_withdrawal` | Symbolic withdrawal amount |
| `proof_top_up_insurance_preserves_inv` | test_params_with_floor, symbolic capital/insurance, threshold verified |
| `fast_frame_deposit_only_mutates_one_account` | Non-fresh account with PnL, warmup slope, fee history |
| `fast_frame_withdraw_only_mutates_one_account` | Position (500 long), LP counterparty, margin exercised |
| `fast_frame_execute_trade_only_mutates_two_accounts` | Wider delta [-500,500], canonical_inv on Ok, non-vacuity |
| `fast_maintenance_margin_uses_equity_including_negative_pnl` | Tighter vault → haircut < 1 exercised |
| `proof_keeper_crank_preserves_inv` | Oracle 1.05M (mark_pnl≠0), symbolic funding_rate [-50,50] |
| `proof_crank_with_funding_preserves_inv` | Symbolic user_cap/size, oracle 1.05M |
| `proof_gap5_fee_settle_margin_or_err` | Added canonical_inv before and after |
| `proof_gap5_fee_credits_trade_then_settle_bounded` | Symbolic user_cap [10K,50K], size [50,500] |
| `proof_gap3_multi_step_lifecycle_conservation` | Oracle_2 widened to [800K,1.2M], symbolic size [50,200] |

### Session 2: 8 UNIT TEST → STRONG

| Proof | Key Improvement |
|---|---|
| `withdraw_im_check_blocks_when_equity_after_withdraw_below_im` | Symbolic capital/position/withdraw, dual IM+MM assertions |
| `proof_fee_credits_never_inflate_from_settle` | Symbolic capital [100,50K], slot [100,100K] |
| `proof_keeper_crank_advances_slot_monotonically` | Symbolic slot [101,10K] |
| `proof_keeper_crank_best_effort_settle` | Symbolic capital [10,500] |
| `proof_close_account_rejects_positive_pnl` | Symbolic capital [100,10K], pnl [1,5K] |
| `proof_close_account_includes_warmed_pnl` | Symbolic capital [100,10K], pnl [1,5K], fast warmup |
| `proof_close_account_negative_pnl_written_off` | Symbolic loss [1,10K] |
| `proof_sequence_deposit_trade_liquidate` | Symbolic user_cap [500,5K], size [100,1M] |

### Session 2: Liquidation consolidation

Deleted 9 concrete liquidation proofs (LQ1-LQ3a, LQ6, PARTIAL-1 through PARTIAL-5) that were subsumed by:
- `proof_lq7_symbolic_oracle_liquidation` (full close: symbolic capital [100,1K], oracle [950K,1.05M])
- `proof_liq_partial_symbolic` **(new)**: symbolic capital [100K,400K], oracle [950K,1M]. Exercises both partial fills and full closes. Checks canonical_inv, OI decrease, dust rule, N1, maintenance margin (when capital suffices).

Also converted `proof_keeper_crank_best_effort_liquidation` to symbolic capital [100,5K].

---

## Remaining Findings (sorted by severity)

### Tier 1: `valid_state()` used instead of `canonical_inv()`

These 6 proofs use the weaker `valid_state()` which omits: freelist acyclicity, aggregate coherence (c_tot, pnl_pos_tot, OI sums), accounting conservation (vault >= c_tot + insurance), and PA2 (i128::MIN guard).

| Proof | Function tested |
|---|---|
| `fast_valid_preserved_by_deposit` | deposit |
| `fast_valid_preserved_by_withdraw` | withdraw |
| `fast_valid_preserved_by_execute_trade` | execute_trade |
| `fast_valid_preserved_by_settle_warmup_to_capital` | settle_warmup_to_capital |
| `fast_valid_preserved_by_top_up_insurance_fund` | top_up_insurance_fund |
| `fast_valid_preserved_by_garbage_collect_dust` | garbage_collect_dust |

### Tier 2: `inv_aggregates` used instead of `canonical_inv`

| Proof | Function tested |
|---|---|
| `proof_set_pnl_maintains_pnl_pos_tot` | set_pnl |
| `proof_set_capital_maintains_c_tot` | set_capital |
| `proof_force_close_with_set_pnl_preserves_invariant` | set_pnl (force-close) |
| `proof_multiple_force_close_preserves_invariant` | set_pnl (two sequential) |

### Tier 3: Other WEAK proofs

| Proof | Issue |
|---|---|
| `proof_execute_trade_margin_enforcement` | Capital over-provisioned; margin trivially satisfied |
| `proof_execute_trade_conservation` | conservation_fast_no_funding not canonical_inv; concrete vault |
| `proof_gap1_crank_with_fees_preserves_inv` | Only fee_credits symbolic; oracle/funding/position concrete |
| `proof_gap2_execute_trade_err_preserves_inv` | Err before mutation; INV trivially preserved |
| `proof_haircut_ratio_bounded` | !solvent branch locked out |
| `proof_profit_conversion_payout_formula` | Partial warmup never exercised |
| `proof_liveness_after_loss_writeoff` | No position on B; margin check unreached |
| `proof_lifecycle_trade_crash_settle_loss_conservation` | Bool → 2 concrete crash values, not symbolic range |
| `proof_flaw2_no_phantom_equity_after_mark_settlement` | Only pnl symbolic; position/entry/oracle concrete |
| `proof_flaw2_withdraw_settles_before_margin_check` | Only w_amount symbolic |
| `proof_flaw3_warmup_reset_increases_slope_proportionally` | Zero-PnL branch locked |
| `proof_set_pnl_preserves_conservation` | conservation_fast_no_funding not canonical_inv |
| `proof_set_capital_decrease_preserves_conservation` | Capital-increase branch locked |
| `proof_net_extraction_bounded_with_fee_credits` | NoOpMatcher → zero trade PnL; claim trivially true |
| `proof_sequence_deposit_crank_withdraw` | No position; crank/withdraw near-no-ops |
| `proof_trade_creates_funding_settled_positions` | Only positive delta; short never tested |

### Tier 4: Remaining UNIT TEST proofs (intentionally concrete)

| Proof | Reason to keep concrete |
|---|---|
| `saturating_arithmetic_prevents_overflow` | Tests stdlib, not percolator |
| `zero_pnl_withdrawable_is_zero` | Trivial boundary test |
| `funding_p5_invalid_bounds_return_overflow` | Two specific error paths |
| `proof_total_open_interest_initial` | Trivial base case |
| `proof_set_risk_reduction_threshold_updates` | Trivial setter test |
| `proof_trading_credits_fee_to_user` | Fee crediting regression |
| `neg_pnl_is_realized_immediately_by_settle` | Redundant with other N1 proofs |
| `proof_settle_maintenance_deducts_correctly` | Exact arithmetic check |
| `proof_add_user_structural_integrity` | inv_structural base case |
| `proof_close_account_structural_integrity` | inv_structural base case |
| `proof_gc_dust_preserves_inv` | Concrete GC test |
| `proof_gc_dust_structural_integrity` | inv_structural |
| `proof_close_account_preserves_inv` | canonical_inv on Ok |
| `proof_lq4_liquidation_fee_paid_to_insurance` | Exact fee arithmetic with custom params |
| `gc_frees_only_true_dust` | Concrete GC boundary |
| `withdrawal_rejects_if_below_initial_margin_at_oracle` | Concrete margin boundary |
| `proof_init_in_place_satisfies_inv` | Base case |
| `proof_lifecycle_trade_warmup_withdraw_topup_conservation` | Concrete lifecycle |
| `proof_lifecycle_...alt_oracle` | Duplicate lifecycle, different oracle |
| `kani_no_teleport_cross_lp_close` | Regression test |
| `kani_rejects_invalid_matcher_output` | Negative test |
| `kani_cross_lp_close_no_pnl_teleport` | Regression test |
| `proof_flaw1_debt_writeoff_requires_flat_position` | Concrete rebuttal |
| `proof_flaw1_gc_never_writes_off_with_open_position` | Concrete rebuttal |
| `proof_gap4_trade_extreme_price_no_panic` | 3 concrete extreme prices |
| `proof_gap4_trade_extreme_size_no_panic` | 3 concrete extreme sizes |
| `proof_gap4_margin_extreme_values_no_panic` | Concrete extreme values |
| `proof_gap5_fee_credits_saturating_near_max` | Concrete near-max test |

---

## Recommended Strengthening Priority

**P0 — High-value, directly actionable:**
1. `fast_valid_preserved_by_*` (6 proofs) → upgrade to `canonical_inv`. Largest category of WEAK proofs.
2. `proof_set_pnl/capital_maintains_*` (4 proofs) → upgrade from `inv_aggregates` to `canonical_inv`.
3. `proof_execute_trade_margin_enforcement` → reduce capital to near margin boundary.

**P1 — Medium effort, good coverage gain:**
4. `proof_haircut_ratio_bounded` → add companion for deficit (vault < c_tot + insurance) branch.
5. `proof_lifecycle_trade_crash_settle_loss_conservation` → replace `bool` with symbolic oracle range.
6. Flaw rebuttal proofs (flaw2, flaw3) → make key inputs symbolic.
7. `proof_profit_conversion_payout_formula` → exercise partial warmup path.

**P2 — Lower priority:**
8. `proof_execute_trade_conservation` → upgrade to canonical_inv.
9. `proof_set_pnl/capital_preserves_conservation` → upgrade to canonical_inv.
10. Remaining frame proofs with branch gaps → widen constraints.
