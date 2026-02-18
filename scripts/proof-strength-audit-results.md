# Kani Proof Strength Audit Results

Generated: 2026-02-18 (final strengthening pass)

146 proof harnesses across `/home/anatoly/percolator/tests/kani.rs`.

---

## Classification Summary

| Classification | Count | Description |
|---|---|---|
| STRONG | 139 | Symbolic inputs exercise key branches, canonical_inv or appropriate invariant, non-vacuous |
| WEAK | 0 | — |
| UNIT TEST | 7 | Intentional: 3 base cases, 1 stdlib test, 1 meta-test, 1 error-path guard, 1 regression |
| VACUOUS | 0 | All proofs have non-vacuity assertions or are trivially reachable |

Previous audit: 94 STRONG, 23 WEAK, 30 UNIT TEST across 147 proofs.
Delta: +45 STRONG, −23 WEAK, −23 UNIT TEST, −1 proof (merged lifecycle alt_oracle).

---

## All Proofs by Section

### Core Properties (I2, I5, I7, I8)

| Proof | Line | Class | Key Inputs |
|---|---|---|---|
| `fast_i2_deposit_preserves_conservation` | 583 | **STRONG** | Symbolic amount; canonical_inv + conservation |
| `fast_i2_withdraw_preserves_conservation` | 605 | **STRONG** | Symbolic deposit+withdraw; canonical_inv |
| `i5_warmup_determinism` | 637 | **STRONG** | 4 symbolic: pnl (pos+neg), reserved, slope, slots |
| `i5_warmup_monotonicity` | 671 | **STRONG** | Symbolic pnl (pos+neg), slope, reserved; two slots |
| `i5_warmup_bounded_by_pnl` | 711 | **STRONG** | Symbolic pnl, reserved, slope, slots |
| `i7_user_isolation_deposit` | 747 | **STRONG** | 3 symbolic amounts; frame isolation |
| `i7_user_isolation_withdrawal` | 784 | **STRONG** | Symbolic deposit+withdraw; frame isolation |
| `i8_equity_with_positive_pnl` | 825 | **STRONG** | Symbolic principal+pnl |
| `i8_equity_with_negative_pnl` | 847 | **STRONG** | Symbolic principal+pnl; both equity branches |

### Withdrawal / PnL Settlement

| Proof | Line | Class | Key Inputs |
|---|---|---|---|
| `withdrawal_requires_sufficient_balance` | 883 | **STRONG** | Symbolic principal/withdraw; canonical_inv pre+post |
| `pnl_withdrawal_requires_warmup` | 917 | **STRONG** | Symbolic pnl/withdraw; canonical_inv pre+post |
| `saturating_arithmetic_prevents_overflow` | 961 | **UNIT TEST** | Tests stdlib, not percolator |
| `zero_pnl_withdrawable_is_zero` | 984 | **STRONG** | Symbolic slot+reserved_pnl; pnl=0 always yields 0 |
| `negative_pnl_withdrawable_is_zero` | 999 | **STRONG** | Symbolic negative pnl |

### Funding Properties (P1-P5)

| Proof | Line | Class | Key Inputs |
|---|---|---|---|
| `funding_p1_settlement_idempotent` | 1021 | **STRONG** | Symbolic position, pnl, funding_index |
| `funding_p2_never_touches_principal` | 1068 | **STRONG** | Symbolic principal, position, funding_delta |
| `funding_p3_bounded_drift` | 1103 | **STRONG** | Symbolic position [10K], delta [10K] |
| `funding_p4_settle_before_position_change` | 1155 | **STRONG** | Symbolic pos, delta1, delta2, new_pos |
| `funding_p5_bounded_operations_no_overflow` | 1231 | **STRONG** | Symbolic price, rate, dt |
| `funding_p5_invalid_bounds_return_overflow` | 1278 | **UNIT TEST** | Symbolic bool selects 2 concrete error paths |
| `funding_zero_position_no_change` | 1301 | **STRONG** | Symbolic pnl, funding_delta |

### Warmup Slope

| Proof | Line | Class | Key Inputs |
|---|---|---|---|
| `proof_warmup_slope_nonzero_when_positive_pnl` | 1339 | **STRONG** | Symbolic positive_pnl |

### Frame Proofs (mutation isolation)

| Proof | Line | Class | Key Inputs |
|---|---|---|---|
| `fast_frame_touch_account_only_mutates_one_account` | 1375 | **STRONG** | Symbolic position, funding_delta; frame isolation |
| `fast_frame_deposit_only_mutates_one_account` | 1439 | **STRONG** | Symbolic amount+pnl; fee history; frame isolation |
| `fast_frame_withdraw_only_mutates_one_account` | 1493 | **STRONG** | Symbolic capital/position/withdraw; canonical_inv pre+post |
| `fast_frame_execute_trade_only_mutates_two_accounts` | 1554 | **STRONG** | Symbolic user_cap+delta; canonical_inv on Ok |
| `fast_frame_settle_warmup_only_mutates_one_account` | 1629 | **STRONG** | Symbolic capital/pnl/slope/slots; canonical_inv; N1 boundary |
| `fast_frame_update_warmup_slope_only_mutates_one_account` | 1687 | **STRONG** | Symbolic pnl+capital; canonical_inv; slope correctness |

### INV Preservation (canonical_inv)

| Proof | Line | Class | Key Inputs |
|---|---|---|---|
| `fast_valid_preserved_by_deposit` | 1759 | **STRONG** | Symbolic amount; canonical_inv pre+post |
| `fast_valid_preserved_by_withdraw` | 1779 | **STRONG** | Symbolic deposit+withdraw; canonical_inv |
| `fast_valid_preserved_by_execute_trade` | 1804 | **STRONG** | Symbolic delta; canonical_inv |
| `fast_valid_preserved_by_settle_warmup_to_capital` | 1833 | **STRONG** | 5 symbolic inputs; both pnl signs; canonical_inv |
| `fast_valid_preserved_by_top_up_insurance_fund` | 1878 | **STRONG** | Symbolic amount; canonical_inv |

### N1 Boundary / Negative PnL Settlement

| Proof | Line | Class | Key Inputs |
|---|---|---|---|
| `fast_neg_pnl_settles_into_capital_independent_of_warm_cap` | 1905 | **STRONG** | Symbolic capital+loss; exact formula |
| `fast_withdraw_cannot_bypass_losses_when_position_zero` | 1947 | **STRONG** | Symbolic capital+loss |
| `fast_neg_pnl_after_settle_implies_zero_capital` | 1985 | **STRONG** | Symbolic capital+loss+slope |
| `neg_pnl_settlement_does_not_depend_on_elapsed_or_slope` | 2019 | **STRONG** | 4 symbolic dimensions |
| `withdraw_calls_settle_enforces_pnl_or_zero_capital_post` | 2065 | **STRONG** | Symbolic capital+loss+withdraw; N1 unconditional |

### Margin / Equity

| Proof | Line | Class | Key Inputs |
|---|---|---|---|
| `fast_maintenance_margin_uses_equity_including_negative_pnl` | 2109 | **STRONG** | Symbolic capital/pnl/position/vault_margin; haircut < 1 |
| `fast_account_equity_computes_correctly` | 2163 | **STRONG** | Symbolic capital+pnl; formula verification |
| `withdraw_im_check_blocks_when_equity_after_withdraw_below_im` | 2215 | **STRONG** | Symbolic capital/position/withdraw; dual IM+MM |

### Fee Settlement

| Proof | Line | Class | Key Inputs |
|---|---|---|---|
| `neg_pnl_is_realized_immediately_by_settle` | 2267 | **STRONG** | Symbolic capital+loss; N1 boundary; pnl write-off |
| `proof_fee_credits_never_inflate_from_settle` | 2316 | **STRONG** | Symbolic capital+slot |
| `proof_settle_maintenance_deducts_correctly` | 2349 | **STRONG** | 3 symbolic: capital/slot/fee_credits |

### Keeper Crank

| Proof | Line | Class | Key Inputs |
|---|---|---|---|
| `proof_keeper_crank_advances_slot_monotonically` | 2395 | **STRONG** | Symbolic capital+slot; canonical_inv; both advancing+non-advancing |
| `proof_keeper_crank_best_effort_settle` | 2446 | **STRONG** | Symbolic capital; LP counterparty; canonical_inv; capital non-increase |
| `proof_keeper_crank_forgives_half_slots` | 2860 | **STRONG** | Symbolic now_slot; forgiveness formula verified |

### Account Lifecycle

| Proof | Line | Class | Key Inputs |
|---|---|---|---|
| `proof_close_account_requires_flat_and_paid` | 2496 | **STRONG** | 3 symbolic bools; all 8 combinations |
| `proof_total_open_interest_initial` | 2551 | **UNIT TEST** | Trivial base case |
| `proof_require_fresh_crank_gates_stale` | 2565 | **STRONG** | Symbolic slot; both fresh+stale branches |
| `proof_stale_crank_blocks_withdraw` | 2597 | **STRONG** | Symbolic slot; canonical_inv; both stale+fresh paths |
| `proof_stale_crank_blocks_execute_trade` | 2632 | **STRONG** | Symbolic slot; canonical_inv; both stale+fresh paths |
| `proof_close_account_rejects_positive_pnl` | 2673 | **STRONG** | Symbolic capital+pnl |
| `proof_close_account_includes_warmed_pnl` | 2705 | **STRONG** | Symbolic capital+pnl; full warmup lifecycle |
| `proof_close_account_negative_pnl_written_off` | 2758 | **STRONG** | Symbolic loss |
| `proof_set_risk_reduction_threshold_updates` | 2792 | **STRONG** | Symbolic threshold |
| `proof_trading_credits_fee_to_user` | 2815 | **STRONG** | Symbolic size [100,5M]; ceiling-div fee formula; canonical_inv |

### Net Extraction

| Proof | Line | Class | Key Inputs |
|---|---|---|---|
| `proof_net_extraction_bounded_with_fee_credits` | 2931 | **STRONG** | 5 symbolic dims; multi-step sequence |

### Liquidation Proofs

| Proof | Line | Class | Key Inputs |
|---|---|---|---|
| `proof_lq4_liquidation_fee_paid_to_insurance` | 3009 | **STRONG** | Symbolic capital [50K,200K]; fee cap verified; canonical_inv |
| `proof_keeper_crank_best_effort_liquidation` | 3067 | **STRONG** | Symbolic capital+oracle; canonical_inv |
| `proof_lq7_symbolic_oracle_liquidation` | 3113 | **STRONG** | Symbolic capital+oracle; canonical_inv, OI, dust, N1 |
| `proof_liq_partial_symbolic` | 3182 | **STRONG** | Symbolic capital+oracle; partial+full close; MM guarded |

### Garbage Collection

| Proof | Line | Class | Key Inputs |
|---|---|---|---|
| `gc_never_frees_account_with_positive_value` | 3266 | **STRONG** | Symbolic bool+capital/pnl; canonical_inv |
| `fast_valid_preserved_by_garbage_collect_dust` | 3328 | **STRONG** | Symbolic live_capital; canonical_inv pre+post |
| `gc_respects_full_dust_predicate` | 3375 | **STRONG** | Symbolic blocker [0,2]; all 3 blockers tested |
| `crank_bounds_respected` | 3440 | **STRONG** | Symbolic capital+slot; budget enforcement |
| `gc_frees_only_true_dust` | 3504 | **STRONG** | Symbolic reserved_val+pnl_val; 3 accounts |

### Withdrawal Margin

| Proof | Line | Class | Key Inputs |
|---|---|---|---|
| `withdrawal_maintains_margin_above_maintenance` | 3572 | **STRONG** | 5 symbolic inputs; MTM equity, mark_pnl |
| `withdrawal_rejects_if_below_initial_margin_at_oracle` | 3636 | **STRONG** | Symbolic capital [5K,20K]+withdraw; both accept+reject paths |

### Engine INV Preservation

| Proof | Line | Class | Key Inputs |
|---|---|---|---|
| `proof_inv_holds_for_new_engine` | 3666 | **UNIT TEST** | Trivial base case |
| `proof_inv_preserved_by_add_user` | 3692 | **STRONG** | Symbolic fee; freelist recycling |
| `proof_inv_preserved_by_add_lp` | 3725 | **STRONG** | Symbolic fee; freelist recycling |
| `proof_execute_trade_preserves_inv` | 3768 | **STRONG** | Symbolic delta+oracle; canonical_inv |
| `proof_execute_trade_conservation` | 3834 | **STRONG** | Symbolic user_cap/lp_cap/delta; canonical_inv+conservation |
| `proof_execute_trade_margin_enforcement` | 3876 | **STRONG** | Symbolic capital [500,2K] near boundary; delta [-15K,15K] |
| `proof_deposit_preserves_inv` | 3946 | **STRONG** | Symbolic capital/pnl/amount/slot; maintenance fees |
| `proof_withdraw_preserves_inv` | 3997 | **STRONG** | Symbolic capital/amount/oracle; position+LP |
| `proof_add_user_structural_integrity` | 4052 | **STRONG** | Symbolic deposit_amt+fee; freelist recycling; inv_structural |
| `proof_close_account_structural_integrity` | 4090 | **STRONG** | Symbolic deposit; inv_structural; popcount |
| `proof_liquidate_preserves_inv` | 4145 | **STRONG** | Symbolic capital+oracle; both triggered/not paths |
| `proof_settle_warmup_preserves_inv` | 4208 | **STRONG** | 8 symbolic inputs; all §6.2 branches; canonical_inv |
| `proof_settle_warmup_negative_pnl_immediate` | 4265 | **STRONG** | 3 symbolic; insolvency/writeoff paths; N1 boundary |
| `proof_keeper_crank_preserves_inv` | 4324 | **STRONG** | Symbolic capital/slot/funding_rate; oracle 1.05M |
| `proof_gc_dust_preserves_inv` | 4385 | **STRONG** | Symbolic live_capital; canonical_inv pre+post |
| `proof_gc_dust_structural_integrity` | 4429 | **STRONG** | Symbolic live_capital; inv_structural pre+post |
| `proof_close_account_preserves_inv` | 4469 | **STRONG** | Symbolic deposit_amt; deposit+withdraw+close lifecycle |

### Sequence Proofs

| Proof | Line | Class | Key Inputs |
|---|---|---|---|
| `proof_top_up_insurance_preserves_inv` | 4517 | **STRONG** | Symbolic capital/insurance/amount; threshold both ways |
| `proof_sequence_deposit_trade_liquidate` | 4576 | **STRONG** | Symbolic user_cap+trade size; INV on both Ok+Err paths |
| `proof_sequence_deposit_crank_withdraw` | 4622 | **STRONG** | 4 symbolic: deposit/trade_size/funding_rate/withdraw |
| `proof_trade_creates_funding_settled_positions` | 4683 | **STRONG** | Symbolic delta [-200,200]; both long+short |
| `proof_crank_with_funding_preserves_inv` | 4739 | **STRONG** | Symbolic user_cap/size/funding_rate; oracle 1.05M |

### Variation Margin / Zero-Sum

| Proof | Line | Class | Key Inputs |
|---|---|---|---|
| `proof_variation_margin_no_pnl_teleport` | 4808 | **STRONG** | Symbolic open/close price + size |
| `proof_trade_pnl_zero_sum` | 4902 | **STRONG** | Symbolic oracle+size; exact zero-sum |
| `kani_no_teleport_cross_lp_close` | 4979 | **STRONG** | Symbolic oracle [500K,2M]+size [1K,10M]; conservation+zero-sum |
| `kani_rejects_invalid_matcher_output` | 5091 | **STRONG** | Symbolic oracle+size; BadMatcherOppositeSign rejected |
| `kani_cross_lp_close_no_pnl_teleport` | 5190 | **UNIT TEST** | Concrete cross-LP regression |

### Haircut / Effective Equity

| Proof | Line | Class | Key Inputs |
|---|---|---|---|
| `proof_haircut_ratio_formula_correctness` | 5254 | **STRONG** | 4 symbolic; 7 properties verified |
| `proof_effective_equity_with_haircut` | 5327 | **STRONG** | 6 symbolic; haircutted vs unhaircutted |
| `proof_principal_protection_across_accounts` | 5401 | **STRONG** | 4 symbolic; cross-account isolation |
| `proof_profit_conversion_payout_formula` | 5473 | **STRONG** | 5 symbolic; exact payout formula |
| `proof_rounding_slack_bound` | 5554 | **STRONG** | 5 symbolic; slack < K verified |

### Liveness / Loss Writeoff

| Proof | Line | Class | Key Inputs |
|---|---|---|---|
| `proof_liveness_after_loss_writeoff` | 5617 | **STRONG** | Symbolic b_capital+withdraw; canonical_inv |

### Gap 1: Error-Path Non-Mutation

| Proof | Line | Class | Key Inputs |
|---|---|---|---|
| `proof_gap1_touch_account_err_no_mutation` | 5822 | **STRONG** | Symbolic position+capital+delta; overflow range; snapshot unchanged |
| `proof_gap1_settle_mark_err_no_mutation` | 5869 | **STRONG** | Symbolic position+capital+pnl_offset; overflow; mutation-freedom |
| `proof_gap1_crank_with_fees_preserves_inv` | 5918 | **STRONG** | Symbolic fee_credits+crank_slot; canonical_inv+conservation |

### Gap 2: Matcher Trust Boundary

| Proof | Line | Class | Key Inputs |
|---|---|---|---|
| `proof_gap2_rejects_overfill_matcher` | 5972 | **STRONG** | Symbolic oracle+size; OverfillMatcher always rejected |
| `proof_gap2_rejects_zero_price_matcher` | 5997 | **STRONG** | Symbolic oracle+size; ZeroPriceMatcher always rejected |
| `proof_gap2_rejects_max_price_exceeded_matcher` | 6022 | **STRONG** | Symbolic oracle+size; MaxPricePlusOneMatcher always rejected |
| `proof_gap2_execute_trade_err_preserves_inv` | 6050 | **STRONG** | Symbolic capital/size; canonical_inv on Err path |

### Gap 3: Multi-Step Conservation

| Proof | Line | Class | Key Inputs |
|---|---|---|---|
| `proof_gap3_conservation_trade_entry_neq_oracle` | 6105 | **STRONG** | Symbolic oracle pair+size; mark PnL exercised |
| `proof_gap3_conservation_crank_funding_positions` | 6165 | **STRONG** | Symbolic oracle+funding_rate; funding settlement |
| `proof_gap3_multi_step_lifecycle_conservation` | 6221 | **STRONG** | 3 symbolic; 4-step lifecycle; canonical_inv at every step |

### Gap 4: Extreme Values

| Proof | Line | Class | Key Inputs |
|---|---|---|---|
| `proof_gap4_trade_extreme_price_no_panic` | 6285 | **STRONG** | Symbolic oracle [1,MAX_ORACLE_PRICE]; canonical_inv |
| `proof_gap4_trade_extreme_size_no_panic` | 6349 | **STRONG** | Symbolic size [1,MAX_POSITION_ABS]; canonical_inv |
| `proof_gap4_trade_partial_fill_diff_price_no_panic` | 6410 | **STRONG** | Symbolic oracle+size; non-zero trade PnL |
| `proof_gap4_margin_extreme_values_no_panic` | 6448 | **STRONG** | Symbolic pos/capital/pnl/oracle; full valid ranges |

### Gap 5: Fee Credits

| Proof | Line | Class | Key Inputs |
|---|---|---|---|
| `proof_gap5_fee_settle_margin_or_err` | 6503 | **STRONG** | 4 symbolic; both Ok+Err branches |
| `proof_gap5_fee_credits_trade_then_settle_bounded` | 6579 | **STRONG** | 3 symbolic; deterministic fee arithmetic |
| `proof_gap5_fee_credits_saturating_near_max` | 6653 | **STRONG** | Symbolic offset [1,10K] from i128::MAX; no wrap; canonical_inv |
| `proof_gap5_deposit_fee_credits_conservation` | 6698 | **STRONG** | Symbolic capital+amount; canonical_inv pre+post |

### Set PnL / Capital Aggregate Proofs

| Proof | Line | Class | Key Inputs |
|---|---|---|---|
| `proof_set_pnl_maintains_pnl_pos_tot` | 6761 | **STRONG** | 2 symbolic pnl; canonical_inv; all 4 sign quadrants |
| `proof_set_capital_maintains_c_tot` | 6790 | **STRONG** | 2 symbolic capital; canonical_inv; increase+decrease |
| `proof_force_close_with_set_pnl_preserves_invariant` | 6821 | **STRONG** | 4 symbolic; force-close pattern; canonical_inv |
| `proof_multiple_force_close_preserves_invariant` | 6873 | **STRONG** | 3 symbolic; composability; canonical_inv |

### Haircut / Effective PnL

| Proof | Line | Class | Key Inputs |
|---|---|---|---|
| `proof_haircut_ratio_bounded` | 6923 | **STRONG** | Symbolic capital/pnl (pos+neg)/insurance/vault; all branches |
| `proof_effective_pnl_bounded_by_actual` | 6964 | **STRONG** | Symbolic capital/pnl/insurance; canonical_inv; neg→0 |
| `proof_recompute_aggregates_correct` | 7003 | **STRONG** | Symbolic capital+pnl; direct value verification |
| `proof_NEGATIVE_bypass_set_pnl_breaks_invariant` | 7043 | **UNIT TEST** | Negative should_panic meta-test |

### Mark / Touch / Funding Preservation

| Proof | Line | Class | Key Inputs |
|---|---|---|---|
| `proof_settle_mark_to_oracle_preserves_inv` | 7086 | **STRONG** | Symbolic position+oracle; canonical_inv; vault unchanged |
| `proof_touch_account_preserves_inv` | 7139 | **STRONG** | Symbolic position+funding_delta; canonical_inv |
| `proof_touch_account_full_preserves_inv` | 7193 | **STRONG** | 4 symbolic: capital/pnl/oracle/slot; full settlement chain |
| `proof_settle_loss_only_preserves_inv` | 7264 | **STRONG** | Symbolic capital+pnl; both absorption+writeoff |
| `proof_accrue_funding_preserves_inv` | 7321 | **STRONG** | Symbolic rate/slot/oracle; canonical_inv |

### Conservation / Aggregate Proofs

| Proof | Line | Class | Key Inputs |
|---|---|---|---|
| `proof_init_in_place_satisfies_inv` | 7376 | **UNIT TEST** | Constructor base case |
| `proof_set_pnl_preserves_conservation` | 7402 | **STRONG** | 2 symbolic pnl; canonical_inv; vault unchanged |
| `proof_set_capital_decrease_preserves_conservation` | 7448 | **STRONG** | Symbolic old/new capital; canonical_inv on decrease |
| `proof_set_capital_aggregate_correct` | 7483 | **STRONG** | 2 symbolic capital; both increase+decrease |

### Lifecycle Proofs

| Proof | Line | Class | Key Inputs |
|---|---|---|---|
| `proof_lifecycle_trade_then_touch_full_conservation` | 7535 | **STRONG** | Symbolic oracle+funding_rate; canonical_inv at every step |
| `proof_lifecycle_trade_crash_settle_loss_conservation` | 7604 | **STRONG** | Symbolic oracle crash [600K,950K]; canonical_inv at every step |
| `proof_lifecycle_trade_warmup_withdraw_topup_conservation` | 7662 | **STRONG** | Symbolic bool oracle + withdraw_amt; 9-step lifecycle |

### Flaw Rebuttal Proofs

| Proof | Line | Class | Key Inputs |
|---|---|---|---|
| `proof_flaw1_debt_writeoff_requires_flat_position` | 7746 | **STRONG** | Symbolic capital [100,5K]+loss; liquidation forces flat position |
| `proof_flaw1_gc_never_writes_off_with_open_position` | 7803 | **STRONG** | Symbolic neg_pnl+position; GC cannot touch open position |
| `proof_flaw2_no_phantom_equity_after_mark_settlement` | 7850 | **STRONG** | Symbolic position/oracle/pnl; canonical_inv |
| `proof_flaw2_withdraw_settles_before_margin_check` | 7923 | **STRONG** | Symbolic oracle+withdraw; canonical_inv; stale entry |
| `proof_flaw3_warmup_reset_increases_slope_proportionally` | 7995 | **STRONG** | Symbolic pnl1+pnl2; slope monotonicity |
| `proof_flaw3_warmup_converts_after_single_slot` | 8043 | **STRONG** | Symbolic pnl; canonical_inv; solvent+insolvent paths |

---

## UNIT TEST Proofs (7)

These are intentional: base cases, meta-tests, and tests that cannot meaningfully benefit from symbolic inputs.

| Proof | Reason |
|---|---|
| `saturating_arithmetic_prevents_overflow` | Tests stdlib saturating arithmetic, not percolator |
| `funding_p5_invalid_bounds_return_overflow` | Symbolic bool selects 2 concrete error paths (guard check) |
| `proof_total_open_interest_initial` | Trivial base case: new engine has OI == 0 |
| `proof_inv_holds_for_new_engine` | Constructor base case: new engine satisfies INV |
| `kani_cross_lp_close_no_pnl_teleport` | Specific regression with custom matchers (P90k, AtOracle) |
| `proof_NEGATIVE_bypass_set_pnl_breaks_invariant` | Negative should_panic meta-test |
| `proof_init_in_place_satisfies_inv` | Constructor base case: init_in_place satisfies INV |

---

## Strengthening Summary

All 23 previously-WEAK proofs were strengthened to STRONG:

| Proof | Fix Applied |
|---|---|
| `i5_warmup_monotonicity` | Added symbolic reserved_pnl (u64), expanded pnl to include negative values |
| `withdrawal_requires_sufficient_balance` | Added canonical_inv pre+post, crank freshness |
| `pnl_withdrawal_requires_warmup` | Added canonical_inv, fixed vault accounting |
| `fast_frame_withdraw_only_mutates_one_account` | Made capital/position symbolic; canonical_inv |
| `fast_frame_settle_warmup_only_mutates_one_account` | Added canonical_inv; N1 boundary postcondition |
| `fast_frame_update_warmup_slope_only_mutates_one_account` | Symbolic capital; canonical_inv; slope correctness |
| `proof_keeper_crank_advances_slot_monotonically` | Symbolic capital; both advancing+non-advancing paths |
| `proof_keeper_crank_best_effort_settle` | LP counterparty; crank slot setup; canonical_inv |
| `proof_stale_crank_blocks_withdraw` | Both stale+fresh paths; canonical_inv |
| `proof_stale_crank_blocks_execute_trade` | Both stale+fresh paths; canonical_inv |
| `proof_inv_preserved_by_add_user` | Freelist recycling via close+reopen cycle |
| `proof_inv_preserved_by_add_lp` | Freelist recycling via close+reopen cycle |
| `fast_valid_preserved_by_garbage_collect_dust` | Symbolic live_capital; canonical_inv |
| `proof_keeper_crank_best_effort_liquidation` | Symbolic oracle; LP counterparty; canonical_inv |
| `proof_close_account_preserves_inv` | Symbolic deposit+withdraw lifecycle |
| `proof_sequence_deposit_trade_liquidate` | INV on both Ok+Err paths |
| `proof_sequence_deposit_crank_withdraw` | Symbolic trade size + funding rate |
| `proof_gap2_execute_trade_err_preserves_inv` | Accounts with pre-existing positions |
| `proof_gap5_deposit_fee_credits_conservation` | Upgraded to canonical_inv |
| `proof_haircut_ratio_bounded` | Include negative pnl; all branches |
| `proof_effective_pnl_bounded_by_actual` | canonical_inv + symbolic insurance |
| `proof_set_capital_decrease_preserves_conservation` | Upgraded to canonical_inv on decrease |
| `proof_flaw2_withdraw_settles_before_margin_check` | canonical_inv on setup; conditional non-vacuity |

17 additional UNIT TEST proofs were upgraded to STRONG:

| Proof | Fix Applied |
|---|---|
| `zero_pnl_withdrawable_is_zero` | Symbolic slot+reserved_pnl |
| `proof_trading_credits_fee_to_user` | Symbolic size [100,5M]; fix ceiling-div fee formula |
| `proof_lq4_liquidation_fee_paid_to_insurance` | Symbolic capital [50K,200K]; fee cap |
| `withdrawal_rejects_if_below_initial_margin_at_oracle` | Symbolic capital [5K,20K]+withdraw; both accept+reject |
| `proof_add_user_structural_integrity` | Symbolic deposit_amt+fee |
| `kani_no_teleport_cross_lp_close` | Symbolic oracle [500K,2M]+size [1K,10M] |
| `kani_rejects_invalid_matcher_output` | Symbolic oracle+size |
| `proof_gap1_touch_account_err_no_mutation` | Symbolic position [MAX/2,MAX]+capital+delta |
| `proof_gap1_settle_mark_err_no_mutation` | Symbolic position+capital+pnl_offset |
| `proof_gap2_rejects_overfill_matcher` | Symbolic oracle+size |
| `proof_gap2_rejects_zero_price_matcher` | Symbolic oracle+size |
| `proof_gap2_rejects_max_price_exceeded_matcher` | Symbolic oracle+size |
| `proof_gap4_trade_extreme_price_no_panic` | Symbolic oracle [1,MAX_ORACLE_PRICE] (replaces 3 concrete) |
| `proof_gap4_trade_extreme_size_no_panic` | Symbolic size [1,MAX_POSITION_ABS] (replaces 3 concrete) |
| `proof_gap4_margin_extreme_values_no_panic` | Symbolic pos/capital/pnl/oracle; full valid ranges |
| `proof_gap5_fee_credits_saturating_near_max` | Symbolic offset [1,10K] from i128::MAX |
| `proof_flaw1_debt_writeoff_requires_flat_position` | Symbolic capital [100,5K]+loss |
| `proof_flaw1_gc_never_writes_off_with_open_position` | Symbolic neg_pnl+position |

6 previously-UNIT TEST proofs were upgraded to STRONG:

| Proof | Fix Applied |
|---|---|
| `neg_pnl_is_realized_immediately_by_settle` | Symbolic capital+loss; N1 boundary |
| `gc_frees_only_true_dust` | Symbolic blocker values (reserved_val, pnl_val) |
| `proof_gc_dust_preserves_inv` | Symbolic live account alongside dust |
| `proof_gc_dust_structural_integrity` | Symbolic live account state |
| `proof_close_account_structural_integrity` | Symbolic deposit+withdraw lifecycle |
| `proof_lifecycle_trade_warmup_withdraw_topup_conservation` | Merged alt_oracle; symbolic bool oracle + withdraw_amt |
