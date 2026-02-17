# Kani Proof Strength Audit Results

Generated: 2026-02-17 (re-audit after strengthening 8 proofs)

~157 proof harnesses analyzed across `/home/anatoly/percolator/tests/kani.rs`.

---

## Classification Summary

| Classification | Count | Description |
|---|---|---|
| STRONG | ~73 | Symbolic inputs exercise key branches, canonical_inv or appropriate invariant, non-vacuous |
| WEAK | ~40 | Symbolic but misses branches, uses weaker invariant, or has symbolic collapse |
| UNIT TEST | ~44 | Concrete inputs, single execution path (some intentionally so) |
| VACUOUS | 0 | All proofs have non-vacuity assertions or are trivially reachable |

---

## Changes Since Last Audit

8 proofs were strengthened in the previous session:

| Proof | Before | After | Key Improvement |
|---|---|---|---|
| `proof_settle_warmup_preserves_inv` | UNIT TEST | **STRONG** | 8 symbolic inputs, all positive-PnL branches |
| `proof_settle_warmup_negative_pnl_immediate` | UNIT TEST | **STRONG** | 3 symbolic inputs, insolvency/writeoff paths |
| `proof_liquidate_preserves_inv` | UNIT TEST | **STRONG** | Symbolic capital+oracle, above/below margin |
| `proof_withdraw_preserves_inv` | WEAK | **STRONG** | Position added, symbolic capital/amount/oracle |
| `proof_deposit_preserves_inv` | WEAK | **STRONG** | Symbolic capital/pnl/amount/slot + maintenance fees |
| `proof_keeper_crank_preserves_inv` | WEAK | **WEAK** | Added positions/LP/fees/symbolic capital+slot, but oracle still concrete |
| `proof_touch_account_full_preserves_inv` | WEAK | **STRONG** | Symbolic capital/pnl/oracle/slot, Undercollateralized reachable |
| `proof_lq7_symbolic_oracle_liquidation` | (new) | **STRONG** | Symbolic oracle exercises mark PnL during liquidation |

---

## Remaining Findings (sorted by severity)

### Tier 1: WEAK proofs on critical functions

| Proof | Line | Issue |
|---|---|---|
| `proof_keeper_crank_preserves_inv` | ~4533 | Oracle concrete at 1M (=entry), so mark_pnl always 0. Funding_rate=0, so funding accrual is no-op. Force-realize path locked to inactive. Good: symbolic capital [100,60K] spans above/below margin, positions + LP + fees present. |
| `proof_execute_trade_margin_enforcement` | ~4087 | Capital 50K/100K is over-provisioned relative to tiny positions (|delta|<100). Margin assertion is trivially true. Cannot exercise Undercollateralized rejection. |
| `proof_crank_with_funding_preserves_inv` | ~4889 | Only funding_rate is symbolic. Oracle/positions/capitals all concrete. Funding payments too small to trigger liquidation or interesting edge cases. |
| `proof_gap1_crank_with_fees_preserves_inv` | ~6065 | Only fee_credits symbolic. Oracle, funding_rate, position size all concrete. Liquidation and force-realize paths locked off. |

### Tier 2: `valid_state()` used instead of `canonical_inv()`

These 6 proofs use the weaker `valid_state()` which omits: freelist acyclicity, aggregate coherence (c_tot, pnl_pos_tot, OI sums), accounting conservation (vault >= c_tot + insurance), and PA2 (i128::MIN guard).

| Proof | Line | Function tested |
|---|---|---|
| `fast_valid_preserved_by_deposit` | ~1670 | deposit |
| `fast_valid_preserved_by_withdraw` | ~1690 | withdraw |
| `fast_valid_preserved_by_execute_trade` | ~1715 | execute_trade |
| `fast_valid_preserved_by_settle_warmup_to_capital` | ~1747 | settle_warmup_to_capital |
| `fast_valid_preserved_by_top_up_insurance_fund` | ~1792 | top_up_insurance_fund |
| `fast_valid_preserved_by_garbage_collect_dust` | ~3572 | garbage_collect_dust |

### Tier 3: Liquidation proofs are all concrete (except LQ7)

Every liquidation proof except the new LQ7 uses near-identical concrete setup: position at entry=oracle=1M, pnl=0. None exercise mark PnL settlement during liquidation.

| Proof | Line | Specific limitation |
|---|---|---|
| `proof_lq1_liquidation_reduces_oi_and_enforces_safety` | ~2847 | Single undercollateralized scenario |
| `proof_lq2_liquidation_preserves_conservation` | ~2912 | Single matching-position scenario |
| `proof_lq3a_profit_routes_through_adl` | ~2967 | Single ADL scenario |
| `proof_lq4_liquidation_fee_paid_to_insurance` | ~3040 | Single fee-accounting scenario |
| `proof_lq6_n1_boundary_after_liquidation` | ~3127 | Identical to LQ1 setup |
| `proof_liq_partial_1_safety_after_liquidation` | ~3245 | Single partial-fill scenario |
| `proof_liq_partial_2_dust_elimination` | ~3289 | Same as partial_1 |
| `proof_liq_partial_3_routing_is_complete` | ~3335 | Two concrete accounts |
| `proof_liq_partial_4_conservation_preservation` | ~3411 | Non-zero PnL but still single path |
| `proof_liq_partial_deterministic` | ~3462 | Same as partial_1 |

### Tier 4: `inv_aggregates` used instead of `canonical_inv`

These proofs verify aggregate coherence only, missing conservation, structural integrity, and per-account invariants.

| Proof | Line | Function tested |
|---|---|---|
| `proof_set_pnl_maintains_pnl_pos_tot` | ~6893 | set_pnl |
| `proof_set_capital_maintains_c_tot` | ~6922 | set_capital |
| `proof_force_close_with_set_pnl_preserves_invariant` | ~6953 | set_pnl (force-close pattern) |
| `proof_multiple_force_close_preserves_invariant` | ~7006 | set_pnl (two sequential) |

### Tier 5: Frame proofs with branch gaps

| Proof | Line | Issue |
|---|---|---|
| `fast_frame_settle_warmup_only_mutates_one_account` | ~1571 | pnl>0 constraint locks out negative-PnL settlement path |
| `fast_frame_deposit_only_mutates_one_account` | ~1419 | Fresh accounts bypass fee settlement, warmup settlement |
| `fast_frame_withdraw_only_mutates_one_account` | ~1464 | Fresh accounts, no position — margin check skipped |
| `fast_frame_update_warmup_slope_only_mutates_one_account` | ~1616 | Only positive pnl. avail_gross==0 path locked. |

### Tier 6: Other notable weaknesses

| Proof | Line | Issue |
|---|---|---|
| `proof_haircut_ratio_bounded` | ~7057 | vault always >= obligations, so `!solvent` branch never tested |
| `proof_profit_conversion_payout_formula` | ~5620 | Warmup slope forces `cap >> avail_gross` — partial conversion never exercised |
| `proof_liveness_after_loss_writeoff` | ~5760 | Account B has no position; margin check in withdraw never reached |
| `proof_lifecycle_trade_crash_settle_loss_conservation` | ~7714 | Binary bool gives exactly 2 oracle prices, not a symbolic range |
| `proof_flaw2_no_phantom_equity_after_mark_settlement` | ~8027 | Position/entry/oracle all concrete; only pnl symbolic. mark_pnl fixed at 250 |
| `proof_flaw2_withdraw_settles_before_margin_check` | ~8099 | Only withdrawal amount symbolic. Position/entry/oracle concrete. |
| `proof_flaw3_warmup_reset_increases_slope_proportionally` | ~8170 | Zero-PnL and instant-warmup branches locked out |
| `proof_sequence_deposit_crank_withdraw` | ~4783 | No position — crank/withdraw are near-no-ops |
| `proof_trade_creates_funding_settled_positions` | ~4833 | Only positive delta — short user position never tested |
| `proof_top_up_insurance_preserves_inv` | ~4704 | Only `amount` symbolic; above_threshold locked to true |
| `proof_set_capital_decrease_preserves_conservation` | ~7563 | Capital-increase branch locked out |
| `proof_net_extraction_bounded_with_fee_credits` | ~2767 | NoOpMatcher gives zero trade PnL; extraction claim is trivially true |
| `proof_gap5_fee_credits_trade_then_settle_bounded` | ~6715 | Only dt symbolic; formula verified for one specific setup |
| `proof_gap2_execute_trade_err_preserves_inv` | ~6194 | Err occurs before any state mutation; INV trivially preserved |
| `proof_gap3_multi_step_lifecycle_conservation` | ~6361 | oracle_1 and size concrete; oracle_2 narrow [950K,1050K] |

---

## Complete Proof Table

### Lines 1-1500

| proof_name | classification | notes |
|---|---|---|
| `fast_i2_deposit_preserves_conservation` | WEAK | conservation_fast_no_funding, fresh account, no fees/position |
| `fast_i2_withdraw_preserves_conservation` | WEAK | Same weaker invariant, margin unreachable (no position) |
| `i5_warmup_determinism` | WEAK | Proves language-level determinism, not safety property; pnl>0 only |
| `i5_warmup_monotonicity` | STRONG | Symbolic pnl, slope, slots1<slots2 |
| `i5_warmup_bounded_by_pnl` | STRONG | Symbolic pnl, reserved, slope, slots |
| `i7_user_isolation_deposit` | WEAK | Concrete op, only capital/pnl checked for other user |
| `i7_user_isolation_withdrawal` | WEAK | Same partial isolation check |
| `i8_equity_with_positive_pnl` | WEAK | eq_i<=0 branch locked out (deprecated function) |
| `i8_equity_with_negative_pnl` | STRONG | Both sides of max(0,...) reachable |
| `withdrawal_requires_sufficient_balance` | STRONG | Symbolic principal/withdraw, correct error |
| `pnl_withdrawal_requires_warmup` | WEAK | PnlNotWarmedUp never reached; InsufficientBalance triggers first |
| `saturating_arithmetic_prevents_overflow` | UNIT TEST | Tests standard library, not percolator |
| `zero_pnl_withdrawable_is_zero` | UNIT TEST | All concrete |
| `negative_pnl_withdrawable_is_zero` | STRONG | Symbolic negative pnl |
| `funding_p1_settlement_idempotent` | STRONG | Symbolic position, pnl, index |
| `funding_p2_never_touches_principal` | STRONG | Symbolic principal, position, funding_delta |
| `funding_p3_bounded_drift` | WEAK | position<100 → payment truncates to 0 in most cases |
| `funding_p4_settle_before_position_change` | STRONG | Symbolic initial_pos, delta1, new_pos, delta2 |
| `funding_p5_bounded_operations_no_overflow` | STRONG | Symbolic price, rate, dt |
| `funding_p5_invalid_bounds_return_overflow` | UNIT TEST | Two concrete error paths via bool |
| `funding_zero_position_no_change` | STRONG | Symbolic pnl and delta |
| `proof_warmup_slope_nonzero_when_positive_pnl` | STRONG | Prevents zero-forever warmup bug |
| `fast_frame_touch_account_only_mutates_one_account` | STRONG | Symbolic position and funding_delta |
| `fast_frame_deposit_only_mutates_one_account` | WEAK | Fresh accounts bypass fee/warmup settlement |
| `fast_frame_withdraw_only_mutates_one_account` | WEAK | Fresh accounts, no position, margin skipped |

### Lines 1500-2800

| proof_name | classification | notes |
|---|---|---|
| `fast_frame_execute_trade_only_mutates_two_accounts` | WEAK | All error paths locked; no INV check |
| `fast_frame_settle_warmup_only_mutates_one_account` | WEAK | Negative PnL branch locked |
| `fast_frame_update_warmup_slope_only_mutates_one_account` | WEAK | Only positive pnl |
| `fast_valid_preserved_by_deposit` | WEAK | valid_state not canonical_inv; fee paths locked |
| `fast_valid_preserved_by_withdraw` | WEAK | valid_state not canonical_inv; no position |
| `fast_valid_preserved_by_execute_trade` | WEAK | valid_state not canonical_inv |
| `fast_valid_preserved_by_settle_warmup_to_capital` | WEAK | valid_state not canonical_inv (best of the valid_state proofs) |
| `fast_valid_preserved_by_top_up_insurance_fund` | WEAK | valid_state not canonical_inv |
| `fast_neg_pnl_settles_into_capital_independent_of_warm_cap` | STRONG | Exact functional correctness |
| `fast_withdraw_cannot_bypass_losses_when_position_zero` | STRONG | Security property verified |
| `fast_neg_pnl_after_settle_implies_zero_capital` | STRONG | N1 boundary verified |
| `neg_pnl_settlement_does_not_depend_on_elapsed_or_slope` | STRONG | Independence proof |
| `withdraw_calls_settle_enforces_pnl_or_zero_capital_post` | STRONG | N1 unconditionally verified |
| `fast_maintenance_margin_uses_equity_including_negative_pnl` | WEAK | Haircut locked to 1; mark_pnl locked to 0 |
| `fast_account_equity_computes_correctly` | STRONG | Exact math equivalence |
| `withdraw_im_check_blocks_when_equity_after_withdraw_below_im` | UNIT TEST | All concrete |
| `neg_pnl_is_realized_immediately_by_settle` | UNIT TEST | All concrete, redundant |
| `proof_fee_credits_never_inflate_from_settle` | UNIT TEST | All concrete |
| `proof_settle_maintenance_deducts_correctly` | UNIT TEST | All concrete |
| `proof_keeper_crank_advances_slot_monotonically` | UNIT TEST | All concrete |
| `proof_keeper_crank_best_effort_settle` | UNIT TEST | All concrete |
| `proof_close_account_requires_flat_and_paid` | STRONG | Exhaustive boolean enumeration |
| `proof_total_open_interest_initial` | UNIT TEST | Trivial |
| `proof_require_fresh_crank_gates_stale` | STRONG | IFF equivalence |
| `proof_stale_crank_blocks_withdraw` | STRONG | Security gate |
| `proof_stale_crank_blocks_execute_trade` | STRONG | Security gate |
| `proof_close_account_rejects_positive_pnl` | UNIT TEST | All concrete |
| `proof_close_account_includes_warmed_pnl` | UNIT TEST | All concrete |
| `proof_close_account_negative_pnl_written_off` | UNIT TEST | All concrete |
| `proof_set_risk_reduction_threshold_updates` | UNIT TEST | Trivial function |
| `proof_trading_credits_fee_to_user` | UNIT TEST | All concrete |
| `proof_keeper_crank_forgives_half_slots` | STRONG | Symbolic slot, exact accounting |
| `proof_net_extraction_bounded_with_fee_credits` | WEAK | NoOpMatcher → zero trade PnL; claim trivially true |

### Lines 2800-4200

| proof_name | classification | notes |
|---|---|---|
| `proof_lq1_liquidation_reduces_oi_and_enforces_safety` | UNIT TEST | All concrete |
| `proof_lq2_liquidation_preserves_conservation` | UNIT TEST | All concrete |
| `proof_lq3a_profit_routes_through_adl` | UNIT TEST | All concrete |
| `proof_lq4_liquidation_fee_paid_to_insurance` | UNIT TEST | All concrete |
| `proof_keeper_crank_best_effort_liquidation` | UNIT TEST | All concrete; only checks Ok |
| `proof_lq6_n1_boundary_after_liquidation` | UNIT TEST | All concrete |
| `proof_lq7_symbolic_oracle_liquidation` | **STRONG** | Symbolic capital+oracle; canonical_inv; OI/dust/N1 |
| `proof_liq_partial_1_safety_after_liquidation` | UNIT TEST | All concrete |
| `proof_liq_partial_2_dust_elimination` | UNIT TEST | All concrete |
| `proof_liq_partial_3_routing_is_complete` | UNIT TEST | All concrete (most comprehensive) |
| `proof_liq_partial_4_conservation_preservation` | UNIT TEST | All concrete |
| `proof_liq_partial_deterministic` | UNIT TEST | All concrete |
| `gc_never_frees_account_with_positive_value` | WEAK | Symbolic bool but no INV check post-GC |
| `fast_valid_preserved_by_garbage_collect_dust` | WEAK | valid_state not canonical_inv |
| `gc_respects_full_dust_predicate` | WEAK | No INV check; no non-vacuity for GC execution |
| `crank_bounds_respected` | WEAK | No position, so liq/force-realize untested |
| `gc_frees_only_true_dust` | UNIT TEST | All concrete |
| `withdrawal_maintains_margin_above_maintenance` | STRONG | Multiple symbolic inputs; margin exercised |
| `withdrawal_rejects_if_below_initial_margin_at_oracle` | UNIT TEST | All concrete |
| `proof_inv_holds_for_new_engine` | STRONG | Base case, canonical_inv |
| `proof_inv_preserved_by_add_user` | STRONG | Symbolic fee; canonical_inv |
| `proof_inv_preserved_by_add_lp` | STRONG | Symmetric; canonical_inv |
| `proof_execute_trade_preserves_inv` | STRONG | Symbolic delta+oracle; canonical_inv |
| `proof_execute_trade_conservation` | WEAK | conservation_fast_no_funding not canonical_inv; concrete vault |
| `proof_execute_trade_margin_enforcement` | WEAK | Capital over-provisioned; margin trivially satisfied |

### Lines 4200-5500

| proof_name | classification | notes |
|---|---|---|
| `proof_deposit_preserves_inv` | **STRONG** | Symbolic capital/pnl/amount/slot; maintenance fees; canonical_inv |
| `proof_withdraw_preserves_inv` | **STRONG** | Symbolic capital/amount/oracle; position+margin; canonical_inv |
| `proof_add_user_structural_integrity` | UNIT TEST | All concrete; inv_structural only |
| `proof_close_account_structural_integrity` | UNIT TEST | All concrete; inv_structural only |
| `proof_liquidate_preserves_inv` | **STRONG** | Symbolic capital/oracle; above/below margin; canonical_inv |
| `proof_settle_warmup_preserves_inv` | **STRONG** | 8 symbolic inputs; all positive-PnL branches; canonical_inv |
| `proof_settle_warmup_negative_pnl_immediate` | **STRONG** | 3 symbolic inputs; insolvency/writeoff; canonical_inv |
| `proof_keeper_crank_preserves_inv` | WEAK | Symbolic capital/slot; positions+LP+fees present. But oracle=entry (mark=0), funding=0. |
| `proof_gc_dust_preserves_inv` | UNIT TEST | All concrete; canonical_inv checked |
| `proof_gc_dust_structural_integrity` | UNIT TEST | All concrete; inv_structural only |
| `proof_close_account_preserves_inv` | UNIT TEST | All concrete; canonical_inv on Ok |
| `proof_top_up_insurance_preserves_inv` | WEAK | Only amount symbolic; above_threshold locked |
| `proof_sequence_deposit_trade_liquidate` | UNIT TEST | All concrete; liquidation never triggers |
| `proof_sequence_deposit_crank_withdraw` | WEAK | No position; crank/withdraw near-no-ops |
| `proof_trade_creates_funding_settled_positions` | WEAK | Only positive delta; oracle/capitals concrete |
| `proof_crank_with_funding_preserves_inv` | WEAK | Only funding_rate symbolic; payment too small |
| `proof_variation_margin_no_pnl_teleport` | STRONG | 3 symbolic inputs; core security property |
| `proof_trade_pnl_zero_sum` | STRONG | Symbolic oracle/size; strong fee zero-sum |
| `kani_no_teleport_cross_lp_close` | UNIT TEST | All concrete; regression test |
| `kani_rejects_invalid_matcher_output` | UNIT TEST | All concrete; negative test |
| `kani_cross_lp_close_no_pnl_teleport` | UNIT TEST | All concrete; regression test |
| `proof_haircut_ratio_formula_correctness` | STRONG | 4 symbolic inputs; all 3 haircut regimes |
| `proof_effective_equity_with_haircut` | STRONG | 6 symbolic inputs; formula verification |

### Lines 5500-6900

| proof_name | classification | notes |
|---|---|---|
| `proof_principal_protection_across_accounts` | STRONG | 4 symbolic inputs |
| `proof_profit_conversion_payout_formula` | WEAK | Partial warmup never exercised |
| `proof_rounding_slack_bound` | STRONG | 5 symbolic inputs |
| `proof_liveness_after_loss_writeoff` | WEAK | No position on B; margin check unreached |
| `proof_gap1_touch_account_err_no_mutation` | UNIT TEST | Targeted Err-path test |
| `proof_gap1_settle_mark_err_no_mutation` | UNIT TEST | Targeted Err-path test |
| `proof_gap1_crank_with_fees_preserves_inv` | WEAK | Only fee_credits symbolic |
| `proof_gap2_rejects_overfill_matcher` | UNIT TEST | Targeted boundary test |
| `proof_gap2_rejects_zero_price_matcher` | UNIT TEST | Targeted boundary test |
| `proof_gap2_rejects_max_price_exceeded_matcher` | UNIT TEST | Targeted boundary test |
| `proof_gap2_execute_trade_err_preserves_inv` | WEAK | Err before mutation; INV trivially preserved |
| `proof_gap3_conservation_trade_entry_neq_oracle` | STRONG | Symbolic oracle pair and size |
| `proof_gap3_conservation_crank_funding_positions` | STRONG | Symbolic oracle and funding_rate |
| `proof_gap3_multi_step_lifecycle_conservation` | WEAK | Narrow symbolic range; oracle_1 concrete |
| `proof_gap4_trade_extreme_price_no_panic` | UNIT TEST | 3 concrete prices |
| `proof_gap4_trade_extreme_size_no_panic` | UNIT TEST | 3 concrete sizes |
| `proof_gap4_trade_partial_fill_diff_price_no_panic` | STRONG | Symbolic oracle and size |
| `proof_gap4_margin_extreme_values_no_panic` | UNIT TEST | Concrete extreme values |
| `proof_gap5_fee_settle_margin_or_err` | WEAK | No canonical_inv; oracle concrete |
| `proof_gap5_fee_credits_trade_then_settle_bounded` | WEAK | Only dt symbolic |
| `proof_gap5_fee_credits_saturating_near_max` | UNIT TEST | All concrete |
| `proof_gap5_deposit_fee_credits_conservation` | STRONG | Symbolic amount |
| `proof_set_pnl_maintains_pnl_pos_tot` | WEAK | inv_aggregates not canonical_inv |

### Lines 6900-8268

| proof_name | classification | notes |
|---|---|---|
| `proof_set_capital_maintains_c_tot` | WEAK | inv_aggregates not canonical_inv |
| `proof_force_close_with_set_pnl_preserves_invariant` | WEAK | inv_aggregates not canonical_inv |
| `proof_multiple_force_close_preserves_invariant` | WEAK | inv_aggregates not canonical_inv; concrete entry |
| `proof_haircut_ratio_bounded` | WEAK | !solvent branch locked out |
| `proof_effective_pnl_bounded_by_actual` | STRONG | Both haircut regimes explored |
| `proof_recompute_aggregates_correct` | STRONG | Direct value assertions |
| `proof_NEGATIVE_bypass_set_pnl_breaks_invariant` | STRONG | Negative test (should_panic) |
| `proof_settle_mark_to_oracle_preserves_inv` | STRONG | Symbolic pos/oracle; canonical_inv |
| `proof_touch_account_preserves_inv` | STRONG | Symbolic pos/funding_delta; canonical_inv |
| `proof_touch_account_full_preserves_inv` | **STRONG** | Symbolic capital/pnl/oracle/slot; canonical_inv; Undercollateralized reachable |
| `proof_settle_loss_only_preserves_inv` | STRONG | Symbolic capital/loss; canonical_inv |
| `proof_accrue_funding_preserves_inv` | STRONG | Symbolic rate/slot/oracle; canonical_inv |
| `proof_init_in_place_satisfies_inv` | UNIT TEST | Valid base case |
| `proof_set_pnl_preserves_conservation` | WEAK | conservation_fast_no_funding not canonical_inv |
| `proof_set_capital_decrease_preserves_conservation` | WEAK | Capital-increase branch locked out |
| `proof_set_capital_aggregate_correct` | STRONG | Both directions, precise assertions |
| `proof_lifecycle_trade_then_touch_full_conservation` | STRONG | Symbolic oracle_2/funding_rate; canonical_inv |
| `proof_lifecycle_trade_crash_settle_loss_conservation` | WEAK | Bool → 2 concrete crash values, not symbolic |
| `proof_lifecycle_trade_warmup_withdraw_topup_conservation` | UNIT TEST | All concrete |
| `proof_lifecycle_...alt_oracle` | UNIT TEST | Duplicate of above with different oracle |
| `proof_flaw1_debt_writeoff_requires_flat_position` | UNIT TEST | All concrete rebuttal |
| `proof_flaw1_gc_never_writes_off_with_open_position` | UNIT TEST | All concrete rebuttal |
| `proof_flaw2_no_phantom_equity_after_mark_settlement` | WEAK | Only pnl symbolic; position/entry/oracle concrete |
| `proof_flaw2_withdraw_settles_before_margin_check` | WEAK | Only w_amount symbolic |
| `proof_flaw3_warmup_reset_increases_slope_proportionally` | WEAK | Zero-PnL branch locked |
| `proof_flaw3_warmup_converts_after_single_slot` | STRONG | Symbolic pnl; canonical_inv |

---

## Recommended Strengthening Priority

**P0 — High-value, directly actionable:**
1. `proof_keeper_crank_preserves_inv` → make oracle symbolic (exercises mark PnL during crank). Currently times out with symbolic oracle; may need reduced position size or narrower ranges.
2. `proof_execute_trade_margin_enforcement` → reduce capital to near margin boundary so solver can find both margin-pass and margin-fail.
3. LQ1-LQ6 and LIQ-PARTIAL-* (10 concrete liquidation proofs) → consolidate into 1-2 symbolic proofs with capital+oracle. LQ7 shows the pattern.

**P1 — Medium effort, good coverage gain:**
4. `fast_valid_preserved_by_*` (6 proofs) → upgrade to `canonical_inv`. This is the largest category of WEAK proofs.
5. `proof_set_pnl/capital_maintains_*` (4 proofs) → upgrade from `inv_aggregates` to `canonical_inv`.
6. `proof_haircut_ratio_bounded` → add companion for deficit (vault < c_tot + insurance) branch.
7. `proof_lifecycle_trade_crash_settle_loss_conservation` → replace `bool` with symbolic oracle range [600K, 950K].

**P2 — Lower priority but systematic:**
8. Frame proofs for deposit/withdraw/settle_warmup → use non-fresh accounts with fees/positions.
9. Flaw rebuttal proofs (flaw1, flaw2, flaw3) → make key inputs symbolic to strengthen from unit tests to proofs.
10. Merge duplicate lifecycle proofs (warmup/withdraw/topup + alt_oracle) into one symbolic variant.
