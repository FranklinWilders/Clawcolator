# Kani Proof Strength Audit Results

Generated: 2026-02-18 (fresh re-audit of all proofs)

147 proof harnesses across `/home/anatoly/percolator/tests/kani.rs`.

---

## Classification Summary

| Classification | Count | Description |
|---|---|---|
| STRONG | 94 | Symbolic inputs exercise key branches, canonical_inv or appropriate invariant, non-vacuous |
| WEAK | 23 | Symbolic but misses branches, uses weaker invariant, or has symbolic collapse |
| UNIT TEST | 30 | Concrete inputs, single execution path (some intentionally so) |
| VACUOUS | 0 | All proofs have non-vacuity assertions or are trivially reachable |

---

## All Proofs by Section

### Core Properties (I2, I5, I7, I8)

| Proof | Line | Class | Key Inputs |
|---|---|---|---|
| `fast_i2_deposit_preserves_conservation` | 580 | **STRONG** | Symbolic amount; canonical_inv + conservation |
| `fast_i2_withdraw_preserves_conservation` | 602 | **STRONG** | Symbolic deposit+withdraw; canonical_inv |
| `i5_warmup_determinism` | 634 | **STRONG** | 4 symbolic: pnl, reserved, slope, slots |
| `i5_warmup_monotonicity` | 668 | **WEAK** | Only positive pnl; reserved_pnl=0 |
| `i5_warmup_bounded_by_pnl` | 701 | **STRONG** | Symbolic pnl, reserved, slope, slots |
| `i7_user_isolation_deposit` | 737 | **STRONG** | 3 symbolic amounts; frame isolation |
| `i7_user_isolation_withdrawal` | 774 | **STRONG** | Symbolic deposit+withdraw; frame isolation |
| `i8_equity_with_positive_pnl` | 815 | **STRONG** | Symbolic principal+pnl |
| `i8_equity_with_negative_pnl` | 837 | **STRONG** | Symbolic principal+pnl; both equity branches |

### Withdrawal / PnL Settlement

| Proof | Line | Class | Key Inputs |
|---|---|---|---|
| `withdrawal_requires_sufficient_balance` | 873 | **WEAK** | Bypasses deposit flow; no canonical_inv |
| `pnl_withdrawal_requires_warmup` | 899 | **WEAK** | No complementary Ok path; no canonical_inv |
| `saturating_arithmetic_prevents_overflow` | 943 | **UNIT TEST** | Tests stdlib, not percolator |
| `zero_pnl_withdrawable_is_zero` | 966 | **UNIT TEST** | Concrete pnl=0, slot=1000 |
| `negative_pnl_withdrawable_is_zero` | 981 | **STRONG** | Symbolic negative pnl |

### Funding Properties (P1-P5)

| Proof | Line | Class | Key Inputs |
|---|---|---|---|
| `funding_p1_settlement_idempotent` | 1003 | **STRONG** | Symbolic position, pnl, funding_index |
| `funding_p2_never_touches_principal` | 1050 | **STRONG** | Symbolic principal, position, funding_delta |
| `funding_p3_bounded_drift` | 1085 | **STRONG** | Symbolic position [10K], delta [10K] |
| `funding_p4_settle_before_position_change` | 1137 | **STRONG** | Symbolic pos, delta1, delta2, new_pos |
| `funding_p5_bounded_operations_no_overflow` | 1213 | **STRONG** | Symbolic price, rate, dt |
| `funding_p5_invalid_bounds_return_overflow` | 1260 | **UNIT TEST** | 2 concrete error paths via bool selector |
| `funding_zero_position_no_change` | 1283 | **STRONG** | Symbolic pnl, funding_delta |

### Warmup Slope

| Proof | Line | Class | Key Inputs |
|---|---|---|---|
| `proof_warmup_slope_nonzero_when_positive_pnl` | 1321 | **STRONG** | Symbolic positive_pnl |

### Frame Proofs (mutation isolation)

| Proof | Line | Class | Key Inputs |
|---|---|---|---|
| `fast_frame_touch_account_only_mutates_one_account` | 1357 | **STRONG** | Symbolic position, funding_delta |
| `fast_frame_deposit_only_mutates_one_account` | 1421 | **STRONG** | Symbolic amount+pnl; fee history |
| `fast_frame_withdraw_only_mutates_one_account` | 1475 | **WEAK** | Symbolic withdraw only; capital/position concrete |
| `fast_frame_execute_trade_only_mutates_two_accounts` | 1534 | **STRONG** | Symbolic user_cap+delta; canonical_inv on Ok |
| `fast_frame_settle_warmup_only_mutates_one_account` | 1609 | **WEAK** | Symbolic but no canonical_inv; no postconditions |
| `fast_frame_update_warmup_slope_only_mutates_one_account` | 1655 | **WEAK** | Symbolic pnl only; no canonical_inv |

### INV Preservation (valid_state / canonical_inv)

| Proof | Line | Class | Key Inputs |
|---|---|---|---|
| `fast_valid_preserved_by_deposit` | 1710 | **STRONG** | Symbolic amount; canonical_inv pre+post |
| `fast_valid_preserved_by_withdraw` | 1730 | **STRONG** | Symbolic deposit+withdraw; canonical_inv |
| `fast_valid_preserved_by_execute_trade` | 1755 | **STRONG** | Symbolic delta; canonical_inv |
| `fast_valid_preserved_by_settle_warmup_to_capital` | 1784 | **STRONG** | 5 symbolic inputs; both pnl signs; canonical_inv |
| `fast_valid_preserved_by_top_up_insurance_fund` | 1829 | **STRONG** | Symbolic amount; canonical_inv |

### N1 Boundary / Negative PnL Settlement

| Proof | Line | Class | Key Inputs |
|---|---|---|---|
| `fast_neg_pnl_settles_into_capital_independent_of_warm_cap` | 1856 | **STRONG** | Symbolic capital+loss; exact formula |
| `fast_withdraw_cannot_bypass_losses_when_position_zero` | 1898 | **STRONG** | Symbolic capital+loss |
| `fast_neg_pnl_after_settle_implies_zero_capital` | 1936 | **STRONG** | Symbolic capital+loss+slope (unbounded slope) |
| `neg_pnl_settlement_does_not_depend_on_elapsed_or_slope` | 1970 | **STRONG** | 4 symbolic dimensions |
| `withdraw_calls_settle_enforces_pnl_or_zero_capital_post` | 2016 | **STRONG** | Symbolic capital+loss+withdraw; N1 unconditional |

### Margin / Equity

| Proof | Line | Class | Key Inputs |
|---|---|---|---|
| `fast_maintenance_margin_uses_equity_including_negative_pnl` | 2060 | **STRONG** | Symbolic capital/pnl/position/vault_margin; haircut < 1 exercised |
| `fast_account_equity_computes_correctly` | 2114 | **STRONG** | Symbolic capital+pnl; formula verification |
| `withdraw_im_check_blocks_when_equity_after_withdraw_below_im` | 2166 | **STRONG** | Symbolic capital/position/withdraw; dual IM+MM |

### Fee Settlement

| Proof | Line | Class | Key Inputs |
|---|---|---|---|
| `neg_pnl_is_realized_immediately_by_settle` | 2218 | **UNIT TEST** | Concrete capital=10K, loss=3K |
| `proof_fee_credits_never_inflate_from_settle` | 2263 | **STRONG** | Symbolic capital+slot |
| `proof_settle_maintenance_deducts_correctly` | 2296 | **STRONG** | 3 symbolic: capital/slot/fee_credits |

### Keeper Crank

| Proof | Line | Class | Key Inputs |
|---|---|---|---|
| `proof_keeper_crank_advances_slot_monotonically` | 2342 | **WEAK** | Symbolic slot but concrete engine; only advanced=true path |
| `proof_keeper_crank_best_effort_settle` | 2396 | **WEAK** | Symbolic capital; no postconditions, no canonical_inv |
| `proof_keeper_crank_forgives_half_slots` | 2772 | **STRONG** | Symbolic now_slot; forgiveness formula verified |

### Account Lifecycle

| Proof | Line | Class | Key Inputs |
|---|---|---|---|
| `proof_close_account_requires_flat_and_paid` | 2428 | **STRONG** | 3 symbolic bools; all 8 combinations |
| `proof_total_open_interest_initial` | 2483 | **UNIT TEST** | Trivial base case |
| `proof_require_fresh_crank_gates_stale` | 2497 | **STRONG** | Symbolic slot; both fresh+stale branches |
| `proof_stale_crank_blocks_withdraw` | 2529 | **WEAK** | Only stale path; no fresh-path coverage |
| `proof_stale_crank_blocks_execute_trade` | 2552 | **WEAK** | Only stale path; no fresh-path coverage |
| `proof_close_account_rejects_positive_pnl` | 2581 | **STRONG** | Symbolic capital+pnl |
| `proof_close_account_includes_warmed_pnl` | 2613 | **STRONG** | Symbolic capital+pnl; full warmup lifecycle |
| `proof_close_account_negative_pnl_written_off` | 2666 | **STRONG** | Symbolic loss |
| `proof_set_risk_reduction_threshold_updates` | 2700 | **UNIT TEST** | Trivial setter |
| `proof_trading_credits_fee_to_user` | 2723 | **UNIT TEST** | Concrete fee arithmetic |

### Net Extraction

| Proof | Line | Class | Key Inputs |
|---|---|---|---|
| `proof_net_extraction_bounded_with_fee_credits` | 2843 | **STRONG** | 5 symbolic dims; multi-step sequence |

### Liquidation Proofs

| Proof | Line | Class | Key Inputs |
|---|---|---|---|
| `proof_lq4_liquidation_fee_paid_to_insurance` | 2921 | **UNIT TEST** | Concrete fee arithmetic |
| `proof_keeper_crank_best_effort_liquidation` | 2978 | **WEAK** | Symbolic capital but concrete oracle=entry; mark_pnl=0 |
| `proof_lq7_symbolic_oracle_liquidation` | 3012 | **STRONG** | Symbolic capital+oracle; canonical_inv, OI, dust, N1 |
| `proof_liq_partial_symbolic` | 3081 | **STRONG** | Symbolic capital+oracle; partial+full close; MM guarded |

### Garbage Collection

| Proof | Line | Class | Key Inputs |
|---|---|---|---|
| `gc_never_frees_account_with_positive_value` | 3165 | **STRONG** | Symbolic bool+capital/pnl; canonical_inv |
| `fast_valid_preserved_by_garbage_collect_dust` | 3227 | **WEAK** | Concrete single dust account |
| `gc_respects_full_dust_predicate` | 3259 | **STRONG** | Symbolic blocker [0,2]; all 3 blockers tested |
| `crank_bounds_respected` | 3324 | **STRONG** | Symbolic capital+slot; budget enforcement |
| `gc_frees_only_true_dust` | 3388 | **UNIT TEST** | Concrete 3 accounts |

### Withdrawal Margin

| Proof | Line | Class | Key Inputs |
|---|---|---|---|
| `withdrawal_maintains_margin_above_maintenance` | 3450 | **STRONG** | 5 symbolic inputs; MTM equity, mark_pnl |
| `withdrawal_rejects_if_below_initial_margin_at_oracle` | 3514 | **UNIT TEST** | Concrete regression (Bug 5) |

### Engine INV Preservation

| Proof | Line | Class | Key Inputs |
|---|---|---|---|
| `proof_inv_holds_for_new_engine` | 3544 | **UNIT TEST** | Trivial base case |
| `proof_inv_preserved_by_add_user` | 3570 | **WEAK** | Symbolic fee but fresh engine; no freelist recycling |
| `proof_inv_preserved_by_add_lp` | 3600 | **WEAK** | Symbolic fee but fresh engine; no freelist recycling |
| `proof_execute_trade_preserves_inv` | 3633 | **STRONG** | Symbolic delta+oracle; canonical_inv |
| `proof_execute_trade_conservation` | 3699 | **STRONG** | Symbolic user_cap/lp_cap/delta; canonical_inv+conservation |
| `proof_execute_trade_margin_enforcement` | 3741 | **STRONG** | Symbolic capital [500,2K] near boundary; delta [-15K,15K] |
| `proof_deposit_preserves_inv` | 3811 | **STRONG** | Symbolic capital/pnl/amount/slot; maintenance fees |
| `proof_withdraw_preserves_inv` | 3862 | **STRONG** | Symbolic capital/amount/oracle; position+LP |
| `proof_add_user_structural_integrity` | 3917 | **UNIT TEST** | Concrete fresh engine |
| `proof_close_account_structural_integrity` | 3951 | **UNIT TEST** | Concrete fresh engine |
| `proof_liquidate_preserves_inv` | 4011 | **STRONG** | Symbolic capital+oracle; both triggered/not paths |
| `proof_settle_warmup_preserves_inv` | 4074 | **STRONG** | 8 symbolic inputs; all positive-PnL branches |
| `proof_settle_warmup_negative_pnl_immediate` | 4131 | **STRONG** | 3 symbolic; insolvency/writeoff paths |
| `proof_keeper_crank_preserves_inv` | 4190 | **STRONG** | Symbolic capital/slot/funding_rate; oracle 1.05M |
| `proof_gc_dust_preserves_inv` | 4251 | **UNIT TEST** | Concrete single dust account |
| `proof_gc_dust_structural_integrity` | 4287 | **UNIT TEST** | Concrete; inv_structural only |
| `proof_close_account_preserves_inv` | 4317 | **WEAK** | Concrete zero-state account; no symbolic variation |

### Sequence Proofs

| Proof | Line | Class | Key Inputs |
|---|---|---|---|
| `proof_top_up_insurance_preserves_inv` | 4363 | **STRONG** | Symbolic capital/insurance/amount; threshold both ways |
| `proof_sequence_deposit_trade_liquidate` | 4422 | **WEAK** | INV gated behind if trade_result.is_ok() |
| `proof_sequence_deposit_crank_withdraw` | 4466 | **WEAK** | Concrete trade size/oracle/funding; withdraw conservative |
| `proof_trade_creates_funding_settled_positions` | 4524 | **STRONG** | Symbolic delta [-200,200]; both long+short |
| `proof_crank_with_funding_preserves_inv` | 4580 | **STRONG** | Symbolic user_cap/size/funding_rate; oracle 1.05M |

### Variation Margin / Zero-Sum

| Proof | Line | Class | Key Inputs |
|---|---|---|---|
| `proof_variation_margin_no_pnl_teleport` | 4649 | **STRONG** | Symbolic open/close price + size |
| `proof_trade_pnl_zero_sum` | 4743 | **STRONG** | Symbolic oracle+size; exact zero-sum |
| `kani_no_teleport_cross_lp_close` | 4820 | **UNIT TEST** | Concrete regression |
| `kani_rejects_invalid_matcher_output` | 4932 | **UNIT TEST** | Concrete negative test |
| `kani_cross_lp_close_no_pnl_teleport` | 5031 | **UNIT TEST** | Concrete cross-LP regression |

### Haircut / Effective Equity

| Proof | Line | Class | Key Inputs |
|---|---|---|---|
| `proof_haircut_ratio_formula_correctness` | 5095 | **STRONG** | 4 symbolic; 7 properties verified |
| `proof_effective_equity_with_haircut` | 5168 | **STRONG** | 6 symbolic; haircutted vs unhaircutted |
| `proof_principal_protection_across_accounts` | 5242 | **STRONG** | 4 symbolic; cross-account isolation |
| `proof_profit_conversion_payout_formula` | 5314 | **STRONG** | 5 symbolic; exact payout formula |
| `proof_rounding_slack_bound` | 5395 | **STRONG** | 5 symbolic; slack < K verified |

### Liveness / Loss Writeoff

| Proof | Line | Class | Key Inputs |
|---|---|---|---|
| `proof_liveness_after_loss_writeoff` | 5458 | **STRONG** | Symbolic b_capital+withdraw; canonical_inv |

### Gap 1: Error-Path Non-Mutation

| Proof | Line | Class | Key Inputs |
|---|---|---|---|
| `proof_gap1_touch_account_err_no_mutation` | 5663 | **STRONG** | Concrete overflow trigger; 9 fields checked |
| `proof_gap1_settle_mark_err_no_mutation` | 5710 | **STRONG** | Concrete PnL overflow; mutation-freedom |
| `proof_gap1_crank_with_fees_preserves_inv` | 5759 | **STRONG** | Symbolic fee_credits+crank_slot; canonical_inv+conservation |

### Gap 2: Matcher Trust Boundary

| Proof | Line | Class | Key Inputs |
|---|---|---|---|
| `proof_gap2_rejects_overfill_matcher` | 5813 | **UNIT TEST** | Concrete overfill rejection |
| `proof_gap2_rejects_zero_price_matcher` | 5838 | **UNIT TEST** | Concrete zero-price rejection |
| `proof_gap2_rejects_max_price_exceeded_matcher` | 5863 | **UNIT TEST** | Concrete max-price rejection |
| `proof_gap2_execute_trade_err_preserves_inv` | 5891 | **WEAK** | Symbolic capital/size but mutations are no-ops (zero positions) |

### Gap 3: Multi-Step Conservation

| Proof | Line | Class | Key Inputs |
|---|---|---|---|
| `proof_gap3_conservation_trade_entry_neq_oracle` | 5942 | **STRONG** | Symbolic oracle pair+size; mark PnL exercised |
| `proof_gap3_conservation_crank_funding_positions` | 6002 | **STRONG** | Symbolic oracle+funding_rate; funding settlement |
| `proof_gap3_multi_step_lifecycle_conservation` | 6058 | **STRONG** | 3 symbolic; 4-step lifecycle; canonical_inv at every step |

### Gap 4: Extreme Values

| Proof | Line | Class | Key Inputs |
|---|---|---|---|
| `proof_gap4_trade_extreme_price_no_panic` | 6122 | **UNIT TEST** | 3 concrete boundary prices |
| `proof_gap4_trade_extreme_size_no_panic` | 6186 | **UNIT TEST** | 3 concrete boundary sizes |
| `proof_gap4_trade_partial_fill_diff_price_no_panic` | 6247 | **STRONG** | Symbolic oracle+size; non-zero trade PnL |
| `proof_gap4_margin_extreme_values_no_panic` | 6285 | **UNIT TEST** | Concrete extreme values |

### Gap 5: Fee Credits

| Proof | Line | Class | Key Inputs |
|---|---|---|---|
| `proof_gap5_fee_settle_margin_or_err` | 6340 | **STRONG** | 4 symbolic; both Ok+Err branches |
| `proof_gap5_fee_credits_trade_then_settle_bounded` | 6416 | **STRONG** | 3 symbolic; deterministic fee arithmetic |
| `proof_gap5_fee_credits_saturating_near_max` | 6490 | **UNIT TEST** | Concrete near-i128::MAX |
| `proof_gap5_deposit_fee_credits_conservation` | 6535 | **WEAK** | conservation_fast_no_funding, not canonical_inv |

### Set PnL / Capital Aggregate Proofs

| Proof | Line | Class | Key Inputs |
|---|---|---|---|
| `proof_set_pnl_maintains_pnl_pos_tot` | 6599 | **STRONG** | 2 symbolic pnl; canonical_inv; all 4 sign quadrants |
| `proof_set_capital_maintains_c_tot` | 6628 | **STRONG** | 2 symbolic capital; canonical_inv; increase+decrease |
| `proof_force_close_with_set_pnl_preserves_invariant` | 6659 | **STRONG** | 4 symbolic; force-close pattern; canonical_inv |
| `proof_multiple_force_close_preserves_invariant` | 6711 | **STRONG** | 3 symbolic; composability; canonical_inv |

### Haircut / Effective PnL

| Proof | Line | Class | Key Inputs |
|---|---|---|---|
| `proof_haircut_ratio_bounded` | 6761 | **WEAK** | pnl always > 0; pnl_pos_tot=0 branch untested |
| `proof_effective_pnl_bounded_by_actual` | 6798 | **WEAK** | Behavioral only; no canonical_inv |
| `proof_recompute_aggregates_correct` | 6827 | **STRONG** | Symbolic capital+pnl; direct value verification |
| `proof_NEGATIVE_bypass_set_pnl_breaks_invariant` | 6866 | **STRONG** | Meta-proof; should_panic; validates INV non-trivial |

### Mark / Touch / Funding Preservation

| Proof | Line | Class | Key Inputs |
|---|---|---|---|
| `proof_settle_mark_to_oracle_preserves_inv` | 6910 | **STRONG** | Symbolic position+oracle; canonical_inv; vault/c_tot unchanged |
| `proof_touch_account_preserves_inv` | 6963 | **STRONG** | Symbolic position+funding_delta; both pay/receive branches |
| `proof_touch_account_full_preserves_inv` | 7017 | **STRONG** | 4 symbolic: capital/pnl/oracle/slot; full settlement chain |
| `proof_settle_loss_only_preserves_inv` | 7088 | **STRONG** | Symbolic capital+pnl; both absorption+writeoff branches |
| `proof_accrue_funding_preserves_inv` | 7145 | **STRONG** | Symbolic rate/slot/oracle; canonical_inv; vault unchanged |

### Conservation / Aggregate Proofs

| Proof | Line | Class | Key Inputs |
|---|---|---|---|
| `proof_init_in_place_satisfies_inv` | 7200 | **UNIT TEST** | Base case constructor |
| `proof_set_pnl_preserves_conservation` | 7226 | **STRONG** | 2 symbolic pnl; canonical_inv; vault unchanged |
| `proof_set_capital_decrease_preserves_conservation` | 7272 | **WEAK** | conservation_fast_no_funding only on decrease; increase unverified |
| `proof_set_capital_aggregate_correct` | 7307 | **STRONG** | 2 symbolic capital; both increase+decrease |

### Lifecycle Proofs

| Proof | Line | Class | Key Inputs |
|---|---|---|---|
| `proof_lifecycle_trade_then_touch_full_conservation` | 7359 | **STRONG** | Symbolic oracle+funding_rate; canonical_inv at every step |
| `proof_lifecycle_trade_crash_settle_loss_conservation` | 7428 | **STRONG** | Symbolic oracle crash [600K,950K]; canonical_inv at every step |
| `proof_lifecycle_trade_warmup_withdraw_topup_conservation` | 7484 | **UNIT TEST** | Concrete oracle=1.05M; single path |
| `proof_lifecycle_trade_warmup_withdraw_topup_conservation_alt_oracle` | 7558 | **UNIT TEST** | Concrete oracle=1.15M; duplicate path |

### Flaw Rebuttal Proofs

| Proof | Line | Class | Key Inputs |
|---|---|---|---|
| `proof_flaw1_debt_writeoff_requires_flat_position` | 7637 | **UNIT TEST** | Concrete rebuttal scenario |
| `proof_flaw1_gc_never_writes_off_with_open_position` | 7694 | **UNIT TEST** | Concrete rebuttal scenario |
| `proof_flaw2_no_phantom_equity_after_mark_settlement` | 7741 | **STRONG** | Symbolic position/oracle/pnl; canonical_inv |
| `proof_flaw2_withdraw_settles_before_margin_check` | 7814 | **WEAK** | Missing canonical_inv on setup; kani::assume(is_ok) vacuity risk |
| `proof_flaw3_warmup_reset_increases_slope_proportionally` | 7882 | **STRONG** | Symbolic pnl1+pnl2; slope monotonicity |
| `proof_flaw3_warmup_converts_after_single_slot` | 7930 | **STRONG** | Symbolic pnl; canonical_inv |

---

## WEAK Proofs — Detailed Issues

### Frame proofs missing canonical_inv (3)

| Proof | Issue |
|---|---|
| `fast_frame_withdraw_only_mutates_one_account` | Symbolic withdraw only; capital/position/entry concrete |
| `fast_frame_settle_warmup_only_mutates_one_account` | No canonical_inv; no postconditions on target account |
| `fast_frame_update_warmup_slope_only_mutates_one_account` | No canonical_inv; slope correctness not verified |

### Keeper crank proofs with limited assertions (4)

| Proof | Issue |
|---|---|
| `proof_keeper_crank_advances_slot_monotonically` | Concrete engine; only advanced=true path |
| `proof_keeper_crank_best_effort_settle` | No postconditions; no canonical_inv |
| `proof_stale_crank_blocks_withdraw` | Only stale path; no fresh-path coverage |
| `proof_stale_crank_blocks_execute_trade` | Only stale path; no fresh-path coverage |

### Proofs with insufficient branch coverage (7)

| Proof | Issue |
|---|---|
| `i5_warmup_monotonicity` | Only positive pnl; reserved_pnl=0 (default) |
| `withdrawal_requires_sufficient_balance` | Bypasses deposit flow; no canonical_inv |
| `pnl_withdrawal_requires_warmup` | No complementary Ok path |
| `proof_keeper_crank_best_effort_liquidation` | Concrete oracle=entry; mark_pnl always 0 |
| `fast_valid_preserved_by_garbage_collect_dust` | Concrete single dust account; no symbolic state |
| `proof_sequence_deposit_trade_liquidate` | INV gated behind `if trade_result.is_ok()` |
| `proof_sequence_deposit_crank_withdraw` | Concrete trade size/oracle/funding |

### Proofs using weaker invariants (4)

| Proof | Issue |
|---|---|
| `proof_close_account_preserves_inv` | Concrete zero-state; all touch_account sub-branches are no-ops |
| `proof_gap5_deposit_fee_credits_conservation` | Uses conservation_fast_no_funding, not canonical_inv |
| `proof_set_capital_decrease_preserves_conservation` | conservation_fast_no_funding only on decrease path |
| `proof_haircut_ratio_bounded` | pnl always > 0; pnl_pos_tot=0 branch untested; no canonical_inv |

### Other WEAK (5)

| Proof | Issue |
|---|---|
| `proof_inv_preserved_by_add_user` | Fresh engine only; no freelist recycling tested |
| `proof_inv_preserved_by_add_lp` | Fresh engine only; no freelist recycling tested |
| `proof_effective_pnl_bounded_by_actual` | Behavioral only; no canonical_inv |
| `proof_gap2_execute_trade_err_preserves_inv` | Pre-rejection mutations are all no-ops (zero positions) |
| `proof_flaw2_withdraw_settles_before_margin_check` | Missing canonical_inv on setup; kani::assume(is_ok) |

---

## UNIT TEST Proofs (30)

| Proof | Reason |
|---|---|
| `saturating_arithmetic_prevents_overflow` | Tests stdlib, not percolator |
| `zero_pnl_withdrawable_is_zero` | Trivial boundary test |
| `funding_p5_invalid_bounds_return_overflow` | 2 concrete error paths |
| `neg_pnl_is_realized_immediately_by_settle` | Redundant with N1 family proofs |
| `proof_total_open_interest_initial` | Trivial base case |
| `proof_set_risk_reduction_threshold_updates` | Trivial setter |
| `proof_trading_credits_fee_to_user` | Concrete fee arithmetic |
| `proof_lq4_liquidation_fee_paid_to_insurance` | Concrete fee arithmetic |
| `gc_frees_only_true_dust` | Concrete 3-account boundary |
| `withdrawal_rejects_if_below_initial_margin_at_oracle` | Concrete Bug 5 regression |
| `proof_inv_holds_for_new_engine` | Constructor base case |
| `proof_add_user_structural_integrity` | Concrete fresh engine |
| `proof_close_account_structural_integrity` | Concrete fresh engine |
| `proof_gc_dust_preserves_inv` | Concrete single dust |
| `proof_gc_dust_structural_integrity` | Concrete structural |
| `kani_no_teleport_cross_lp_close` | Concrete regression |
| `kani_rejects_invalid_matcher_output` | Concrete negative test |
| `kani_cross_lp_close_no_pnl_teleport` | Concrete cross-LP regression |
| `proof_gap2_rejects_overfill_matcher` | Concrete overfill rejection |
| `proof_gap2_rejects_zero_price_matcher` | Concrete zero-price rejection |
| `proof_gap2_rejects_max_price_exceeded_matcher` | Concrete max-price rejection |
| `proof_gap4_trade_extreme_price_no_panic` | 3 concrete boundary prices |
| `proof_gap4_trade_extreme_size_no_panic` | 3 concrete boundary sizes |
| `proof_gap4_margin_extreme_values_no_panic` | Concrete extreme values |
| `proof_gap5_fee_credits_saturating_near_max` | Concrete near-i128::MAX |
| `proof_init_in_place_satisfies_inv` | Constructor base case |
| `proof_lifecycle_trade_warmup_withdraw_topup_conservation` | Concrete lifecycle |
| `proof_lifecycle_trade_warmup_withdraw_topup_conservation_alt_oracle` | Duplicate lifecycle, different oracle |
| `proof_flaw1_debt_writeoff_requires_flat_position` | Concrete rebuttal |
| `proof_flaw1_gc_never_writes_off_with_open_position` | Concrete rebuttal |

---

## Changes from Previous Audit

### Proofs upgraded from WEAK to STRONG since last audit

These proofs were previously classified as WEAK due to using `valid_state()` instead of `canonical_inv()`, using `inv_aggregates` instead of `canonical_inv()`, or having insufficient symbolic coverage. They have since been upgraded:

| Proof | Previous Issue | Resolution |
|---|---|---|
| `fast_valid_preserved_by_deposit` | Used valid_state() | Now uses canonical_inv |
| `fast_valid_preserved_by_withdraw` | Used valid_state() | Now uses canonical_inv |
| `fast_valid_preserved_by_execute_trade` | Used valid_state() | Now uses canonical_inv |
| `fast_valid_preserved_by_settle_warmup_to_capital` | Used valid_state() | Now uses canonical_inv |
| `fast_valid_preserved_by_top_up_insurance_fund` | Used valid_state() | Now uses canonical_inv |
| `proof_set_pnl_maintains_pnl_pos_tot` | Used inv_aggregates | Now uses canonical_inv |
| `proof_set_capital_maintains_c_tot` | Used inv_aggregates | Now uses canonical_inv |
| `proof_force_close_with_set_pnl_preserves_invariant` | Used inv_aggregates | Now uses canonical_inv |
| `proof_multiple_force_close_preserves_invariant` | Used inv_aggregates | Now uses canonical_inv |
| `proof_execute_trade_margin_enforcement` | Capital over-provisioned | Capital [500,2K] near boundary; delta [-15K,15K] |
| `proof_execute_trade_conservation` | conservation_fast only | Now also uses canonical_inv; symbolic capitals |
| `proof_gap1_crank_with_fees_preserves_inv` | Only fee_credits symbolic | Added symbolic crank_slot; state built via APIs |
| `proof_profit_conversion_payout_formula` | Partial warmup locked | Symbolic slope [0,10] exercises partial+full |
| `proof_liveness_after_loss_writeoff` | No position on B | B has position (500); margin check reachable |
| `proof_lifecycle_trade_crash_settle_loss_conservation` | Bool → 2 concrete values | Symbolic oracle crash [600K,950K] |
| `proof_flaw2_no_phantom_equity_after_mark_settlement` | Only pnl symbolic | Symbolic position/oracle/pnl |
| `proof_flaw3_warmup_reset_increases_slope_proportionally` | Zero-PnL branch locked | Symbolic pnl1 [0,5K] includes zero |
| `proof_set_pnl_preserves_conservation` | conservation_fast only | Now uses canonical_inv |
| `proof_net_extraction_bounded_with_fee_credits` | NoOpMatcher trivially true | 5 symbolic dims; multi-step extraction bound |
| `proof_trade_creates_funding_settled_positions` | Only positive delta | Symbolic delta [-200,200]; both directions |

### Proofs reclassified as WEAK (new findings)

| Proof | Previous | Now | Reason |
|---|---|---|---|
| `proof_close_account_preserves_inv` | UNIT TEST | WEAK | Has canonical_inv but concrete zero-state is all no-ops |
| `proof_sequence_deposit_trade_liquidate` | STRONG | WEAK | INV gated behind `if trade_result.is_ok()` |
| `proof_sequence_deposit_crank_withdraw` | STRONG | WEAK | Concrete trade/oracle/funding; withdraw conservative |
| `proof_gap2_execute_trade_err_preserves_inv` | WEAK | WEAK | Pre-rejection mutations still all no-ops |
| `fast_frame_withdraw_only_mutates_one_account` | STRONG | WEAK | Only withdraw amount symbolic; capital/position concrete |

---

## Recommended Strengthening Priority

**P0 — High-value, directly actionable:**
1. Frame proofs (3): Add `canonical_inv` assertions to `fast_frame_withdraw`, `fast_frame_settle_warmup`, `fast_frame_update_warmup_slope`.
2. `proof_close_account_preserves_inv` → make fee_credits symbolic to exercise fee forgiveness branch.
3. `proof_gap2_execute_trade_err_preserves_inv` → set up accounts with pre-existing positions so touch_account does real work before rejection.

**P1 — Medium effort, good coverage gain:**
4. `proof_sequence_deposit_trade_liquidate` → remove `if trade_result.is_ok()` gate; split into Ok/Err harnesses.
5. `proof_sequence_deposit_crank_withdraw` → make trade size/funding_rate/oracle symbolic.
6. Keeper crank proofs (4): Add canonical_inv, test both stale+fresh paths, add postconditions.
7. `proof_haircut_ratio_bounded` → include pnl=0 to test pnl_pos_tot=0 early return.
8. Lifecycle UNIT TEST pair → merge into single symbolic proof with symbolic oracle_2.

**P2 — Lower priority:**
9. `proof_inv_preserved_by_add_user/add_lp` → pre-populate engine with existing accounts.
10. Matcher rejection UNIT TESTs (3) → make inputs symbolic, add INV preservation on Err.
11. Extreme value UNIT TESTs (3) → make oracle/size symbolic across valid range.
12. `proof_effective_pnl_bounded_by_actual` → add canonical_inv.
13. `proof_flaw2_withdraw_settles_before_margin_check` → fix setup, add canonical_inv.
