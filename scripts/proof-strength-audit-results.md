# Kani Proof Strength Audit Results

Generated: 2026-02-19 (comprehensive 5-point audit, fresh re-analysis)

146 proof harnesses across `/home/anatoly/percolator/tests/kani.rs`.

Methodology: Each proof analyzed for:
1. **Input classification**: concrete (hardcoded) vs symbolic (`kani::any()` with `kani::assume`) vs derived
2. **Branch coverage**: whether constraints allow solver to reach both sides of conditionals in the function-under-test
3. **Invariant strength**: `canonical_inv()` (STRONG) vs `valid_state()` (WEAK) vs neither
4. **Vacuity risk**: contradictory assumes, hand-built unreachable states, always-error paths
5. **Symbolic collapse**: whether derived values collapse symbolic ranges

Classification criteria:
- **STRONG**: Symbolic inputs exercise key branches of function-under-test; uses `canonical_inv()` or equivalently strong property-specific assertions; non-vacuous (reachable paths verified via `assert_ok!`/`assert_err!` or explicit non-vacuity assertions)
- **WEAK**: Uses `valid_state()` instead of `canonical_inv()`, or symbolic inputs miss key branches, or invariant assertions are weaker than available
- **UNIT TEST**: Concrete inputs intentionally limit scope to specific scenarios, or is a meta/negative proof
- **VACUOUS**: Contradictory assumptions make the proof trivially true

Scaffolding policy: Concrete values that do NOT affect branch coverage in the function-under-test (e.g., slot numbers for fresh crank, capital amounts that only ensure margin passes, `last_crank_slot = 100`) are treated as scaffolding and do not downgrade a proof.

---

## Final Tally

| Classification | Count | Description |
|---|---|---|
| **STRONG** | 144 | Symbolic inputs exercise key branches, canonical_inv or equivalent strong assertions, non-vacuous |
| **WEAK** | 0 | -- |
| **UNIT TEST** | 2 | Intentional meta-test and concrete-oracle scenario test |
| **VACUOUS** | 0 | All proofs have non-vacuity assertions or trivially reachable assertions |

---

## Summary Table (All 146 Proofs)

### I2: Conservation of Funds (2 proofs)

| # | Proof Name | Classification | Inputs | Invariant | Non-Vacuity |
|---|---|---|---|---|---|
| 1 | `fast_i2_deposit_preserves_conservation` | **STRONG** | Symbolic capital [100,5K], pnl (-2K,2K), now_slot [100,300], amount (0,5K) | canonical_inv pre+post, conservation_fast_no_funding | Fee accrual branch exercised via last_fee_slot=50 |
| 2 | `fast_i2_withdraw_preserves_conservation` | **STRONG** | Symbolic deposit (0,10K), withdraw (0,10K), withdraw <= deposit | canonical_inv pre+post, conservation_fast_no_funding | assert_ok! on both deposit and withdraw |

### I5: PNL Warmup Properties (3 proofs)

| # | Proof Name | Classification | Inputs | Invariant | Non-Vacuity |
|---|---|---|---|---|---|
| 3 | `i5_warmup_determinism` | **STRONG** | Symbolic pnl (-10K,10K), reserved <5K, slope (0,100), slots <200 | Determinism: w1==w2 | pnl != 0 forced; both positive/negative PnL paths |
| 4 | `i5_warmup_monotonicity` | **STRONG** | Symbolic pnl (-5K,10K), slope <100, reserved, slots1 < slots2 <200 | w2 >= w1 | slots2 > slots1 forced |
| 5 | `i5_warmup_bounded_by_pnl` | **STRONG** | Symbolic pnl (-10K,10K), reserved <5K, slope (0,100), slots <200 | withdrawable <= available, warmup cap | Negative PnL branch asserted separately |

### I7: User Isolation (2 proofs)

| # | Proof Name | Classification | Inputs | Invariant | Non-Vacuity |
|---|---|---|---|---|---|
| 6 | `i7_user_isolation_deposit` | **STRONG** | Symbolic amount1, amount2, op_amount all (0,10K) | user2 capital+pnl unchanged | assert_ok! on all three deposits |
| 7 | `i7_user_isolation_withdrawal` | **STRONG** | Symbolic amount1 (100,10K), amount2 (0,10K), withdraw (0,amount1] | user2 capital+pnl unchanged | assert_ok! on deposits + withdraw |

### I8: Equity Consistency (2 proofs)

| # | Proof Name | Classification | Inputs | Invariant | Non-Vacuity |
|---|---|---|---|---|---|
| 8 | `i8_equity_with_positive_pnl` | **STRONG** | Symbolic principal <10K, pnl (-10K,10K) covering both +/- | equity == max(0, capital+pnl) exact | Full range covers both branches |
| 9 | `i8_equity_with_negative_pnl` | **STRONG** | Symbolic principal <10K, pnl (-10K,0) | equity == max(0, capital+pnl) exact | pnl < 0 forced; subsumes negative branch |

### Withdrawal Safety (2 proofs)

| # | Proof Name | Classification | Inputs | Invariant | Non-Vacuity |
|---|---|---|---|---|---|
| 10 | `withdrawal_requires_sufficient_balance` | **STRONG** | Symbolic principal <10K, withdraw (principal,20K) | canonical_inv pre+post, InsufficientBalance error | withdraw > principal forces error path |
| 11 | `pnl_withdrawal_requires_warmup` | **STRONG** | Symbolic pnl (0,5K), capital [1K,5K], withdraw (capital, capital+pnl], now_slot [10,200] | canonical_inv pre+post, withdrawable==0, error | warmup_started_at=now_slot forces elapsed=0 |

### Arithmetic Safety (1 proof)

| # | Proof Name | Classification | Inputs | Invariant | Non-Vacuity |
|---|---|---|---|---|---|
| 12 | `saturating_arithmetic_prevents_overflow` | **STRONG** | Symbolic pos (-100,100), entry [900K,1.1M], oracle [900K,1.1M] | Sign correctness, magnitude bound, zero case | Both zero and nonzero pos; long and short branches |

### Edge Cases (2 proofs)

| # | Proof Name | Classification | Inputs | Invariant | Non-Vacuity |
|---|---|---|---|---|---|
| 13 | `zero_pnl_withdrawable_is_zero` | **STRONG** | pnl=0 (concrete edge case), symbolic slot <10K, reserved <5K | withdrawable == 0 universally | Symbolic slot+reserved prove for all values |
| 14 | `negative_pnl_withdrawable_is_zero` | **STRONG** | Symbolic pnl (-10K,0), slot <10K, slope <=1K | withdrawable == 0 universally | Symbolic pnl+slot+slope universally quantified |

### Funding Rate Invariants (7 proofs)

| # | Proof Name | Classification | Inputs | Invariant | Non-Vacuity |
|---|---|---|---|---|---|
| 15 | `funding_p1_settlement_idempotent` | **STRONG** | Symbolic position (abs<1M), pnl (-1M,1M), index (abs<1B) | pnl unchanged on second settle, snapshot==global | touch_account unwrap x2 |
| 16 | `funding_p2_never_touches_principal` | **STRONG** | Symbolic principal <1M, position (abs<1M), funding_delta (abs<1B) | capital unchanged after touch_account | touch_account unwrap |
| 17 | `funding_p3_bounded_drift_between_opposite_positions` | **STRONG** | Symbolic position (0,10K), delta (abs<10K) | change <= 0 AND change >= -2 | Both user+LP settlements must succeed |
| 18 | `funding_p4_settle_before_position_change` | **STRONG** | Symbolic initial_pos (0,10K), delta1/delta2 (abs<1K), new_pos (0,10K) | Direction properties across two periods | Two settlement periods with position change |
| 19 | `funding_p5_bounded_operations_no_overflow` | **STRONG** | Symbolic price (1M,1B), rate (abs<1K), dt <1K | Must succeed in bounded region | Non-vacuity sub-check for small inputs |
| 20 | `funding_p5_invalid_bounds_return_overflow` | **STRONG** | Symbolic rate, dt; at least one bound violated | Must return Err(Overflow) | kani::assume(bad_rate or bad_dt) forces error |
| 21 | `funding_zero_position_no_change` | **STRONG** | position=0 (concrete edge), symbolic pnl (abs<1M), delta (abs<1B) | pnl unchanged | Zero position skip path verified |

### Warmup Correctness (1 proof)

| # | Proof Name | Classification | Inputs | Invariant | Non-Vacuity |
|---|---|---|---|---|---|
| 22 | `proof_warmup_slope_nonzero_when_positive_pnl` | **STRONG** | Symbolic positive_pnl (0,10K) | slope >= 1 when positive_pnl > 0 | assert_ok! on update_warmup_slope |

### Frame Proofs (6 proofs)

| # | Proof Name | Classification | Inputs | Invariant | Non-Vacuity |
|---|---|---|---|---|---|
| 23 | `fast_frame_touch_account_only_mutates_one_account` | **STRONG** | Symbolic position (abs<1K), funding_delta (abs<1M) | Other account + globals unchanged, capital unchanged | unwrap on touch_account |
| 24 | `fast_frame_deposit_only_mutates_one_account_vault_and_warmup` | **STRONG** | Symbolic amount (0,10K), pnl (-2K,5K) nonzero | Other account unchanged | assert_ok! on deposit; fee accrual via last_fee_slot=50 |
| 25 | `fast_frame_withdraw_only_mutates_one_account_vault_and_warmup` | **STRONG** | Symbolic user_capital [1K,5K], position (-500,500), withdraw (0,user_capital) | Other account unchanged, canonical_inv | assert_ok! on withdraw |
| 26 | `fast_frame_execute_trade_only_mutates_two_accounts` | **STRONG** | Symbolic user_cap [500,2K], delta [1,5K] | Observer unchanged, vault unchanged on Ok | Non-vacuity for conservative trades |
| 27 | `fast_frame_settle_warmup_only_mutates_one_account_and_warmup_globals` | **STRONG** | Symbolic capital [0,5K], pnl (-5K,5K), slope <100, slots <200 | Other account unchanged, canonical_inv, N1 | unwrap on settle |
| 28 | `fast_frame_update_warmup_slope_only_mutates_one_account` | **STRONG** | Symbolic pnl (-2K,5K), capital [0,5K] | Other + globals unchanged, canonical_inv | unwrap, slope correctness |

### Validity Preservation (5 proofs)

| # | Proof Name | Classification | Inputs | Invariant | Non-Vacuity |
|---|---|---|---|---|---|
| 29 | `fast_valid_preserved_by_deposit` | **STRONG** | Symbolic capital [100,5K], pnl (-2K,2K), now_slot [100,300], amount (0,5K) | canonical_inv pre+post | Fee accrual via last_fee_slot=50 |
| 30 | `fast_valid_preserved_by_withdraw` | **STRONG** | Symbolic capital [1K,5K], position (-500,500), withdraw (0,capital) | canonical_inv pre+post | Both margin fail + success paths |
| 31 | `fast_valid_preserved_by_execute_trade` | **STRONG** | Symbolic user_cap [500,2K], lp_cap [500,2K], delta [1,5K], oracle [500K,2M] | canonical_inv pre+post | Near-margin capitals for both pass/fail |
| 32 | `fast_valid_preserved_by_settle_warmup_to_capital` | **STRONG** | Symbolic capital [0,5K], pnl (-5K,5K), slope <100, slots <200, insurance, vault_margin | canonical_inv pre+post | assert non-vacuity: settle must succeed |
| 33 | `fast_valid_preserved_by_top_up_insurance_fund` | **STRONG** | Symbolic amount (0,10K) | canonical_inv pre+post | assert non-vacuity: top_up must succeed |

### Negative PnL Settlement / Fix A (5 proofs)

| # | Proof Name | Classification | Inputs | Invariant | Non-Vacuity |
|---|---|---|---|---|---|
| 34 | `fast_neg_pnl_settles_into_capital_independent_of_warm_cap` | **STRONG** | Symbolic capital [0,10K], loss (0,10K] | canonical_inv, exact formula: pay=min(C,L), N1 | unwrap on settle |
| 35 | `fast_withdraw_cannot_bypass_losses_when_position_zero` | **STRONG** | Symbolic capital (0,10K), loss (0,capital) | canonical_inv, InsufficientBalance when capital exhausted | withdraw(capital) forced to fail |
| 36 | `fast_neg_pnl_after_settle_implies_zero_capital` | **STRONG** | Symbolic capital [0,10K], loss (0,10K], slope <100 | canonical_inv, N1 boundary | unwrap on settle |
| 37 | `neg_pnl_settlement_does_not_depend_on_elapsed_or_slope` | **STRONG** | Symbolic capital [0,10K], loss (0,10K], slope (0,100), elapsed (0,500) | Exact formula match independent of slope/elapsed | Universal over slope/elapsed |
| 38 | `withdraw_calls_settle_enforces_pnl_or_zero_capital_post` | **STRONG** | Symbolic capital (0,10K), loss (0,capital), withdraw_amt (0,capital-loss] | canonical_inv, N1 on both Ok/Err paths | Both success/failure paths exercised |

### Equity Margin / Fix B (3 proofs)

| # | Proof Name | Classification | Inputs | Invariant | Non-Vacuity |
|---|---|---|---|---|---|
| 39 | `fast_maintenance_margin_uses_equity_including_negative_pnl` | **STRONG** | Symbolic capital [0,10K], pnl (-5K,5K), position (-500,500), vault_margin | Exact margin formula match | Both above/below maintenance cases |
| 40 | `fast_account_equity_computes_correctly` | **STRONG** | Symbolic capital <1M, pnl (-1M,1M) | canonical_inv, equity == max(0, capital+pnl) exact | Uses set_capital/set_pnl + sync_engine_aggregates |
| 41 | `withdraw_im_check_blocks_when_equity_after_withdraw_below_im` | **STRONG** | Symbolic capital [50,500], position [100,5K], withdraw (0,capital] | IM/MM boundary checks, exact formula | Non-vacuity for conservative case |

### Deterministic Negative PnL (1 proof)

| # | Proof Name | Classification | Inputs | Invariant | Non-Vacuity |
|---|---|---|---|---|---|
| 42 | `neg_pnl_is_realized_immediately_by_settle` | **STRONG** | Symbolic capital (0,10K], loss (0,10K] | Exact formula, N1 boundary | unwrap on settle |

### Fee Credits (4 proofs)

| # | Proof Name | Classification | Inputs | Invariant | Non-Vacuity |
|---|---|---|---|---|---|
| 43 | `proof_fee_credits_never_inflate_from_settle` | **STRONG** | Symbolic capital [100,5K], now_slot [200,500] | canonical_inv, credits non-increasing | unwrap on settle_maintenance_fee |
| 44 | `proof_settle_maintenance_deducts_correctly` | **STRONG** | Symbolic capital [100,5K], now_slot [200,500], fee_credits (0,5K) | canonical_inv, zero-sum fee accounting | Exact formula assertions |
| 45 | `proof_trading_credits_fee_to_user` | **STRONG** | Symbolic size [100,5M] | canonical_inv, exact fee formula | assert_ok! on trade |
| 46 | `proof_keeper_crank_forgives_half_slots` | **STRONG** | Symbolic now_slot [200,500] | canonical_inv, exact forgive = dt/2 formula | Deterministic accounting assertions |

### Keeper Crank (3 proofs)

| # | Proof Name | Classification | Inputs | Invariant | Non-Vacuity |
|---|---|---|---|---|---|
| 47 | `proof_keeper_crank_advances_slot_monotonically` | **STRONG** | Symbolic capital [10,500], now_slot [200,500] | canonical_inv, slot monotonicity | Both advancing/non-advancing paths |
| 48 | `proof_keeper_crank_best_effort_settle` | **STRONG** | Symbolic capital [10,500] | canonical_inv, best-effort settle | Capital must not increase |
| 49 | `proof_keeper_crank_best_effort_liquidation` | **STRONG** | Symbolic capital [100,10K], oracle_price [500K,2M] | canonical_inv | Always-undercollateralized setup |

### Close Account (4 proofs)

| # | Proof Name | Classification | Inputs | Invariant | Non-Vacuity |
|---|---|---|---|---|---|
| 50 | `proof_close_account_requires_flat_and_paid` | **STRONG** | Symbolic capital [0,5K], pnl (-5K,5K), position (-500,500) | canonical_inv, close rejects if pos!=0 or pnl>0 | Symbolic inputs cover both paths |
| 51 | `proof_close_account_rejects_positive_pnl` | **STRONG** | Symbolic capital [0,5K], pnl (0,5K) | canonical_inv, PnlNotWarmedUp | assert result == Err |
| 52 | `proof_close_account_includes_warmed_pnl` | **STRONG** | Symbolic capital [0,5K], pnl (0,5K), insurance, slope | Both fully/partially warmed paths | Both branches asserted |
| 53 | `proof_close_account_negative_pnl_written_off` | **STRONG** | Symbolic loss (0,5K] | canonical_inv, res == Ok(0) | Loss written off correctly |

### Parameter Update (1 proof)

| # | Proof Name | Classification | Inputs | Invariant | Non-Vacuity |
|---|---|---|---|---|---|
| 54 | `proof_set_risk_reduction_threshold_updates` | **STRONG** | Symbolic new_threshold | canonical_inv, value matches | Threshold verified |

### Total Open Interest (1 proof)

| # | Proof Name | Classification | Inputs | Invariant | Non-Vacuity |
|---|---|---|---|---|---|
| 55 | `proof_total_open_interest_initial` | **STRONG** | Symbolic pos0, pos1 | canonical_inv, OI == sum(|pos|) | Exact formula |

### Freshness Gate (3 proofs)

| # | Proof Name | Classification | Inputs | Invariant | Non-Vacuity |
|---|---|---|---|---|---|
| 56 | `proof_require_fresh_crank_gates_stale` | **STRONG** | Symbolic now_slot | Err/Ok based on staleness | Both stale/fresh paths covered |
| 57 | `proof_stale_crank_blocks_withdraw` | **STRONG** | Symbolic now_slot | canonical_inv, Unauthorized when stale | Both stale/fresh paths |
| 58 | `proof_stale_crank_blocks_execute_trade` | **STRONG** | Symbolic now_slot | canonical_inv, Unauthorized when stale | Both stale/fresh paths |

### Net Extraction (1 proof)

| # | Proof Name | Classification | Inputs | Invariant | Non-Vacuity |
|---|---|---|---|---|---|
| 59 | `proof_net_extraction_bounded_with_fee_credits` | **STRONG** | Symbolic deposits, do_crank, do_trade, delta, withdraw | canonical_inv, principal-bounded extraction | Non-vacuity for no-trade case |

### Liquidation (3 proofs)

| # | Proof Name | Classification | Inputs | Invariant | Non-Vacuity |
|---|---|---|---|---|---|
| 60 | `proof_lq4_liquidation_fee_paid_to_insurance` | **STRONG** | Symbolic capital [50K,200K] | canonical_inv, insurance increase, fee cap | assert triggered |
| 61 | `proof_lq7_symbolic_oracle_liquidation` | **STRONG** | Symbolic capital [100,10K], oracle_price [500K,2M] | canonical_inv, OI decrease, dust rule, N1 | assert_ok!, always undercollateralized |
| 62 | `proof_liq_partial_symbolic` | **STRONG** | Symbolic capital [100K,400K], oracle [500K,2M] | canonical_inv, OI, dust, N1, margin | Non-vacuity for partial fill |

### Garbage Collection (5 proofs)

| # | Proof Name | Classification | Inputs | Invariant | Non-Vacuity |
|---|---|---|---|---|---|
| 63 | `gc_never_frees_account_with_positive_value` | **STRONG** | Symbolic has_capital/pnl flags + values | canonical_inv, positive account survives | GC closed > 0 |
| 64 | `fast_valid_preserved_by_garbage_collect_dust` | **STRONG** | Symbolic live_capital | canonical_inv, live survives | GC closed > 0 |
| 65 | `gc_respects_full_dust_predicate` | **STRONG** | Symbolic blocker selection (3 cases), values | canonical_inv, target survives | Symbolic blocker selection |
| 66 | `gc_frees_only_true_dust` | **STRONG** | Symbolic reserved_val, pnl_val | canonical_inv, correct classification | Three accounts tested |
| 67 | `crank_bounds_respected` | **STRONG** | Symbolic capital, now_slot | canonical_inv, budget limits | cursor advances or sweep completes |

### Withdrawal Margin Safety (2 proofs)

| # | Proof Name | Classification | Inputs | Invariant | Non-Vacuity |
|---|---|---|---|---|---|
| 68 | `withdrawal_maintains_margin_above_maintenance` | **STRONG** | Symbolic capital, pos, entry, oracle, amount | canonical_inv, above MM post-withdraw | Non-vacuity for conservative case |
| 69 | `withdrawal_rejects_if_below_initial_margin_at_oracle` | **STRONG** | Symbolic capital, withdraw | canonical_inv, IM check | Both above/below IM |

### Canonical INV Proofs (3 proofs)

| # | Proof Name | Classification | Inputs | Invariant | Non-Vacuity |
|---|---|---|---|---|---|
| 70 | `proof_inv_holds_for_new_engine` | **STRONG** | Symbolic warmup, maint_bps, init_bps, fee_bps, deposit | canonical_inv on new() + after add_user + deposit | assert_ok! on deposit |
| 71 | `proof_inv_preserved_by_add_user` | **STRONG** | Symbolic fee <1M | canonical_inv, freelist recycling verified | assert_ok!, recycled slot verified |
| 72 | `proof_inv_preserved_by_add_lp` | **STRONG** | Symbolic fee <1M | canonical_inv, freelist recycling verified | assert_ok!, recycled slot verified |

### Execute Trade Family (3 proofs)

| # | Proof Name | Classification | Inputs | Invariant | Non-Vacuity |
|---|---|---|---|---|---|
| 73 | `proof_execute_trade_preserves_inv` | **STRONG** | Symbolic delta_size, oracle_price | canonical_inv, position = before + delta | assert_ok! |
| 74 | `proof_execute_trade_conservation` | **STRONG** | Symbolic user_cap, lp_cap, delta_size | canonical_inv, conservation_fast_no_funding | assert non-vacuity |
| 75 | `proof_execute_trade_margin_enforcement` | **STRONG** | Symbolic capital [500,2K], delta | canonical_inv, IM checked post-trade | Non-vacuity for conservative trade |

### Deposit/Withdraw Families (2 proofs)

| # | Proof Name | Classification | Inputs | Invariant | Non-Vacuity |
|---|---|---|---|---|---|
| 76 | `proof_deposit_preserves_inv` | **STRONG** | Symbolic capital, pnl, amount, now_slot | canonical_inv | assert_ok! |
| 77 | `proof_withdraw_preserves_inv` | **STRONG** | Symbolic capital, amount, oracle_price | canonical_inv, position exercises IM/MM | Non-vacuity for conservative case |

### Freelist Structural (2 proofs)

| # | Proof Name | Classification | Inputs | Invariant | Non-Vacuity |
|---|---|---|---|---|---|
| 78 | `proof_add_user_structural_integrity` | **STRONG** | Symbolic deposit_amt, fee | canonical_inv, popcount+1, freelist advance | assert_ok!, recycled slot |
| 79 | `proof_close_account_structural_integrity` | **STRONG** | Symbolic deposit | canonical_inv, popcount-1, used bit cleared | assert_ok!, free_head == user_idx |

### Liquidate Family (1 proof)

| # | Proof Name | Classification | Inputs | Invariant | Non-Vacuity |
|---|---|---|---|---|---|
| 80 | `proof_liquidate_preserves_inv` | **STRONG** | Symbolic capital, oracle_price | canonical_inv on Ok | assert_ok! |

### Settle Warmup Family (2 proofs)

| # | Proof Name | Classification | Inputs | Invariant | Non-Vacuity |
|---|---|---|---|---|---|
| 81 | `proof_settle_warmup_preserves_inv` | **STRONG** | Symbolic capital, pnl, slope, warmup_start, slot, reserved, insurance, vault_margin | canonical_inv | assert_ok! |
| 82 | `proof_settle_warmup_negative_pnl_immediate` | **STRONG** | Symbolic capital, loss, insurance | canonical_inv, N1, pnl >= 0 | assert_ok! |

### Keeper Crank Family (1 proof)

| # | Proof Name | Classification | Inputs | Invariant | Non-Vacuity |
|---|---|---|---|---|---|
| 83 | `proof_keeper_crank_preserves_inv` | **STRONG** | Symbolic capital, now_slot, funding_rate | canonical_inv | assert_ok! |

### GC Dust Family (2 proofs)

| # | Proof Name | Classification | Inputs | Invariant | Non-Vacuity |
|---|---|---|---|---|---|
| 84 | `proof_gc_dust_preserves_inv` | **STRONG** | Symbolic live_capital | canonical_inv, live survives | GC freed > 0 |
| 85 | `proof_gc_dust_structural_integrity` | **STRONG** | Symbolic live_capital | canonical_inv, live survives | GC runs successfully |

### Close Account Family (1 proof)

| # | Proof Name | Classification | Inputs | Invariant | Non-Vacuity |
|---|---|---|---|---|---|
| 86 | `proof_close_account_preserves_inv` | **STRONG** | Symbolic deposit_amt | canonical_inv, unused bit, num_used-1 | assert_ok! |

### Top Up Insurance Family (1 proof)

| # | Proof Name | Classification | Inputs | Invariant | Non-Vacuity |
|---|---|---|---|---|---|
| 87 | `proof_top_up_insurance_preserves_inv` | **STRONG** | Symbolic capital, insurance, amount | canonical_inv, vault+amount, insurance+amount, threshold | Non-vacuity for below-threshold |

### Sequence-Level Proofs (2 proofs)

| # | Proof Name | Classification | Inputs | Invariant | Non-Vacuity |
|---|---|---|---|---|---|
| 88 | `proof_sequence_deposit_trade_liquidate` | **STRONG** | Symbolic user_cap, size | canonical_inv at each step | Non-vacuity for conservative trade |
| 89 | `proof_sequence_deposit_crank_withdraw` | **STRONG** | Symbolic deposit, size, funding_rate, withdraw | canonical_inv at each step | assert_ok! on all 4 steps |

### Funding/Position Conservation (2 proofs)

| # | Proof Name | Classification | Inputs | Invariant | Non-Vacuity |
|---|---|---|---|---|---|
| 90 | `proof_trade_creates_funding_settled_positions` | **STRONG** | Symbolic delta | canonical_inv, both have positions, funding settled | assert Ok |
| 91 | `proof_crank_with_funding_preserves_inv` | **STRONG** | Symbolic user_cap, size, funding_rate | canonical_inv, last_crank_slot advanced | assert Ok on crank |

### Variation Margin / No PnL Teleportation (2 proofs)

| # | Proof Name | Classification | Inputs | Invariant | Non-Vacuity |
|---|---|---|---|---|---|
| 92 | `proof_variation_margin_no_pnl_teleport` | **STRONG** | Symbolic open_price, close_price, size | Equity change identical across LP1/LP2 | assert_ok! on all 4 trades |
| 93 | `proof_trade_pnl_zero_sum` | **STRONG** | Symbolic oracle, size | Total delta == -fee, LP delta == 0 | assert Ok |

### Inline Migrated (2 proofs)

| # | Proof Name | Classification | Inputs | Invariant | Non-Vacuity |
|---|---|---|---|---|---|
| 94 | `kani_no_teleport_cross_lp_close` | **STRONG** | Symbolic oracle [500K,2M], btc [1K,10M]; concrete capitals (1M each, scaffolding) | All pnl == 0, total == 0, conservation | assert_ok! on both trades |
| 95 | `kani_cross_lp_close_no_pnl_teleport` | **UNIT TEST** | Symbolic cap_mult [1,100], size [1,5]; concrete ORACLE_100K limits oracle to single value | LP2 capital unchanged, pnl == 0, conservation | unwrap on both trades |

**Rationale for #95 UNIT TEST**: The concrete `ORACLE_100K` constant means mark_pnl calculations, margin checks, and the P90kMatcher's price offset are all exercised at a single oracle price point. The symbolic `size` range [1,5] is also small. While the proof correctly verifies the no-teleport property at this price, it does not generalize over oracle prices. This is an intentional scenario test migrated from inline proofs.

### Matcher Guard (1 proof)

| # | Proof Name | Classification | Inputs | Invariant | Non-Vacuity |
|---|---|---|---|---|---|
| 96 | `kani_rejects_invalid_matcher_output` | **STRONG** | Symbolic oracle [1,2M], size [1,10M]; concrete capitals (scaffolding) | InvalidMatchingEngine error | assert matches!; matcher guard depends on sign, not capital |

### Haircut Mechanism C1-C6 (6 proofs)

| # | Proof Name | Classification | Inputs | Invariant | Non-Vacuity |
|---|---|---|---|---|---|
| 97 | `proof_haircut_ratio_formula_correctness` | **STRONG** | Symbolic vault, c_tot, insurance, pnl_pos_tot all <=100K | h_den>0, h in [0,1], 7 properties | Non-vacuity for partial haircut |
| 98 | `proof_effective_equity_with_haircut` | **STRONG** | Symbolic vault <=100, c_tot, insurance, pnl_pos_tot <=100, capital <=50, pnl (-50,50) | Exact formula match, haircutted <= unhaircutted | Non-vacuity for partial haircut |
| 99 | `proof_principal_protection_across_accounts` | **STRONG** | Symbolic a_capital (0,10K], a_loss (a_capital,20K], b_capital (0,10K], b_pnl (0,10K] | B's capital+pnl unchanged, conservation | Loss exceeds capital forces writeoff path |
| 100 | `proof_profit_conversion_payout_formula` | **STRONG** | Symbolic capital <=500, pnl (0,250], vault <=2K, insurance <=500, slope <=10 | Exact formula: C+y, PNL-x, y<=x | Non-vacuity for underbacked case |
| 101 | `proof_rounding_slack_bound` | **STRONG** | Symbolic pnl_a (0,100], pnl_b (0,100], vault <=400, c_tot, insurance | sum_eff <= residual, slack < K | Non-vacuity for underbacked |
| 102 | `proof_liveness_after_loss_writeoff` | **STRONG** | Symbolic a_capital [0,1K], a_loss [1,5K], b_capital [5K,50K], withdraw | canonical_inv, withdrawal succeeds post-writeoff | assert_ok! on withdraw |

### Security Audit Gap Closure - Gap 1 (3 proofs)

| # | Proof Name | Classification | Inputs | Invariant | Non-Vacuity |
|---|---|---|---|---|---|
| 103 | `proof_gap1_touch_account_err_no_mutation` | **STRONG** | Symbolic pos_scale, capital, delta | Full snapshot unchanged on Err | assert result.is_err() |
| 104 | `proof_gap1_settle_mark_err_no_mutation` | **STRONG** | Symbolic pos_scale, capital, pnl_offset | Full snapshot unchanged on Err | assert result.is_err() |
| 105 | `proof_gap1_crank_with_fees_preserves_inv` | **STRONG** | Symbolic fee_credits, crank_slot | canonical_inv, conservation | assert_ok!, crank advanced |

### Security Audit Gap Closure - Gap 2 (4 proofs)

| # | Proof Name | Classification | Inputs | Invariant | Non-Vacuity |
|---|---|---|---|---|---|
| 106 | `proof_gap2_rejects_overfill_matcher` | **STRONG** | Symbolic oracle, size | InvalidMatchingEngine | assert matches! |
| 107 | `proof_gap2_rejects_zero_price_matcher` | **STRONG** | Symbolic oracle, size | InvalidMatchingEngine | assert matches! |
| 108 | `proof_gap2_rejects_max_price_exceeded_matcher` | **STRONG** | Symbolic oracle, size | InvalidMatchingEngine | assert matches! |
| 109 | `proof_gap2_execute_trade_err_preserves_inv` | **STRONG** | Symbolic user_cap, size | canonical_inv on Err path | assert result.is_err() |

### Security Audit Gap Closure - Gap 3 (3 proofs)

| # | Proof Name | Classification | Inputs | Invariant | Non-Vacuity |
|---|---|---|---|---|---|
| 110 | `proof_gap3_conservation_trade_entry_neq_oracle` | **STRONG** | Symbolic oracle_1, oracle_2, size | canonical_inv, conservation | assert Ok on both trades |
| 111 | `proof_gap3_conservation_crank_funding_positions` | **STRONG** | Symbolic size, oracle_2, funding_rate | canonical_inv, conservation | assert_ok! on crank |
| 112 | `proof_gap3_multi_step_lifecycle_conservation` | **STRONG** | Symbolic oracle_2, funding_rate, size, user_deposit | canonical_inv at each step, conservation | 4-step lifecycle |

### Security Audit Gap Closure - Gap 4 (4 proofs)

| # | Proof Name | Classification | Inputs | Invariant | Non-Vacuity |
|---|---|---|---|---|---|
| 113 | `proof_gap4_trade_extreme_price_no_panic` | **STRONG** | Symbolic capital [100M,1e15], oracle [1,MAX_ORACLE_PRICE] | canonical_inv on Ok | Non-vacuity: large capital must succeed |
| 114 | `proof_gap4_trade_extreme_size_no_panic` | **STRONG** | Symbolic size [1,MAX_POSITION_ABS], oracle [100K,10M]; concrete deep_capital (scaffolding) | canonical_inv on Ok | Non-vacuity: moderate size must succeed |
| 115 | `proof_gap4_trade_partial_fill_diff_price_no_panic` | **STRONG** | Symbolic user_cap [100K,500K], lp_cap [100K,500K], oracle [500K,1.5M], size [50,500] | canonical_inv on Ok | Non-vacuity for conservative case |
| 116 | `proof_gap4_margin_extreme_values_no_panic` | **STRONG** | Symbolic pos (-500,500), capital [1K,10K], pnl (-5K,5K), entry [900K,1.1M], oracle [900K,1.1M] | canonical_inv, equity+margin properties | Equity positivity assertions |

### Security Audit Gap Closure - Gap 5 (4 proofs)

| # | Proof Name | Classification | Inputs | Invariant | Non-Vacuity |
|---|---|---|---|---|---|
| 117 | `proof_gap5_fee_settle_margin_or_err` | **STRONG** | Symbolic user_cap, size, fee_credits, now_slot | canonical_inv, MM or no-position on Ok | Both Ok/Err paths |
| 118 | `proof_gap5_fee_credits_trade_then_settle_bounded` | **STRONG** | Symbolic user_cap, size, dt | canonical_inv, exact coupon formula | Deterministic accounting |
| 119 | `proof_gap5_fee_credits_saturating_near_max` | **STRONG** | Symbolic offset, size | canonical_inv, no wrap-around | Credits non-decreasing |
| 120 | `proof_gap5_deposit_fee_credits_conservation` | **STRONG** | Symbolic capital, amount | canonical_inv, vault+amount, insurance+amount | assert_ok! |

### Premarket Resolution / Aggregate Consistency (8 proofs)

| # | Proof Name | Classification | Inputs | Invariant | Non-Vacuity |
|---|---|---|---|---|---|
| 121 | `proof_set_pnl_maintains_pnl_pos_tot` | **STRONG** | Symbolic initial_pnl, new_pnl | canonical_inv after two set_pnl calls | Two set_pnl calls |
| 122 | `proof_set_capital_maintains_c_tot` | **STRONG** | Symbolic initial_cap, new_cap | canonical_inv after two set_capital calls | Two set_capital calls |
| 123 | `proof_force_close_with_set_pnl_preserves_invariant` | **STRONG** | Symbolic initial_pnl, position, entry_price, settlement_price | canonical_inv after force-close simulation | Force-close simulation |
| 124 | `proof_multiple_force_close_preserves_invariant` | **STRONG** | Symbolic pos1, pos2, settlement_price | canonical_inv after two force-closes | Two force-closes |
| 125 | `proof_haircut_ratio_bounded` | **STRONG** | Symbolic capital, pnl, insurance, vault | h_num <= h_den, correct edge cases | Both positive/negative pnl |
| 126 | `proof_effective_pnl_bounded_by_actual` | **STRONG** | Symbolic capital, pnl, insurance | canonical_inv, eff <= actual | Negative pnl case verified |
| 127 | `proof_recompute_aggregates_correct` | **STRONG** | Symbolic capital, pnl | c_tot == capital, pnl_pos_tot correct | Bypassed helpers to test recompute |
| 128 | `proof_NEGATIVE_bypass_set_pnl_breaks_invariant` | **UNIT TEST** | Symbolic initial_pnl, new_pnl, bypass_pnl | !inv_aggregates (intentional negative proof) | Meta-test: proves bypassing set_pnl breaks invariant |

**Rationale for #128 UNIT TEST**: This is an intentional negative/meta proof that demonstrates the WRONG approach (bypassing `set_pnl()`) DOES break `inv_aggregates`. It uses symbolic inputs but asserts `!inv_aggregates(&engine)` -- the negation of correctness. It exists to prove the real proofs are non-vacuous: if the invariant can be broken by bypass, then the proofs showing it holds via proper helpers are meaningful.

### Missing Conservation Proofs (8 proofs)

| # | Proof Name | Classification | Inputs | Invariant | Non-Vacuity |
|---|---|---|---|---|---|
| 129 | `proof_settle_mark_to_oracle_preserves_inv` | **STRONG** | Symbolic pos, oracle | canonical_inv, vault/c_tot/insurance unchanged | assert_ok! |
| 130 | `proof_touch_account_preserves_inv` | **STRONG** | Symbolic pos, funding_delta | canonical_inv, vault/c_tot/insurance unchanged | assert_ok! |
| 131 | `proof_touch_account_full_preserves_inv` | **STRONG** | Symbolic capital, pnl_raw, oracle, now_slot | canonical_inv on Ok | Non-vacuity for conservative case |
| 132 | `proof_settle_loss_only_preserves_inv` | **STRONG** | Symbolic capital, pnl < 0 | canonical_inv, vault/insurance unchanged, pnl >= 0 | assert_ok! |
| 133 | `proof_accrue_funding_preserves_inv` | **STRONG** | Symbolic rate, now_slot, oracle | canonical_inv, vault/c_tot/insurance unchanged | assert_ok! |
| 134 | `proof_init_in_place_satisfies_inv` | **STRONG** | Symbolic deposit, withdraw | canonical_inv at each step | assert_ok! on deposit+withdraw |
| 135 | `proof_set_pnl_preserves_conservation` | **STRONG** | Symbolic initial_pnl, new_pnl | canonical_inv, vault/c_tot/insurance unchanged | Two set_pnl calls |
| 136 | `proof_set_capital_decrease_preserves_conservation` | **STRONG** | Symbolic old_capital, new_capital | canonical_inv when decreasing, aggregates always correct | Both increase/decrease |

### set_capital Aggregate (1 proof)

| # | Proof Name | Classification | Inputs | Invariant | Non-Vacuity |
|---|---|---|---|---|---|
| 137 | `proof_set_capital_aggregate_correct` | **STRONG** | Symbolic old_capital, new_capital | c_tot delta exactly tracked | Both increase/decrease |

### Multi-Step Conservation (3 proofs)

| # | Proof Name | Classification | Inputs | Invariant | Non-Vacuity |
|---|---|---|---|---|---|
| 138 | `proof_lifecycle_trade_then_touch_full_conservation` | **STRONG** | Symbolic user_deposit, size, oracle_2, funding_rate | canonical_inv + conservation at each step | 6-step lifecycle |
| 139 | `proof_lifecycle_trade_crash_settle_loss_conservation` | **STRONG** | Symbolic oracle_crash | canonical_inv + conservation at each step | 6-step crash lifecycle |
| 140 | `proof_lifecycle_trade_warmup_withdraw_topup_conservation` | **STRONG** | Symbolic lp_deposit, oracle_2, withdraw_amt | canonical_inv + conservation at each step | 9-step profitable lifecycle |

### External Review Rebuttal - Flaw 1 (2 proofs)

| # | Proof Name | Classification | Inputs | Invariant | Non-Vacuity |
|---|---|---|---|---|---|
| 141 | `proof_flaw1_debt_writeoff_requires_flat_position` | **STRONG** | Symbolic user_capital, user_loss | Position zero after full liquidation | assert triggered, assert Ok |
| 142 | `proof_flaw1_gc_never_writes_off_with_open_position` | **STRONG** | Symbolic neg_pnl, pos | canonical_inv, pnl unchanged, account survives | is_used asserted |

### External Review Rebuttal - Flaw 2 (2 proofs)

| # | Proof Name | Classification | Inputs | Invariant | Non-Vacuity |
|---|---|---|---|---|---|
| 143 | `proof_flaw2_no_phantom_equity_after_mark_settlement` | **STRONG** | Symbolic pos, oracle, pnl | entry==oracle, mark_pnl==0, equity unchanged | assert_ok!, canonical_inv |
| 144 | `proof_flaw2_withdraw_settles_before_margin_check` | **STRONG** | Symbolic oracle, w_amount | entry settled to oracle, pnl >= 0 on Ok | Non-vacuity for conservative case |

### External Review Rebuttal - Flaw 3 (2 proofs)

| # | Proof Name | Classification | Inputs | Invariant | Non-Vacuity |
|---|---|---|---|---|---|
| 145 | `proof_flaw3_warmup_reset_increases_slope_proportionally` | **STRONG** | Symbolic pnl1, pnl2 > pnl1 | slope2 >= slope1, timer reset | assert_ok! on both updates |
| 146 | `proof_flaw3_warmup_converts_after_single_slot` | **STRONG** | Symbolic pnl > 0 | slope >= 1, capital increased, canonical_inv | assert_ok!, solvent case strictly increases |

---

## Detailed Analysis of UNIT TEST Proofs (2 proofs)

### 1. `proof_NEGATIVE_bypass_set_pnl_breaks_invariant` (proof #128, line 7363)

- **Classification**: UNIT TEST (intentional negative/meta proof)
- **Purpose**: Demonstrates that bypassing `set_pnl()` and directly assigning `account.pnl` breaks the `inv_aggregates` invariant. This is a meta-test that proves the REAL proofs are non-vacuous: if the invariant can be broken via bypass, the proofs showing it holds via proper helpers are meaningful.
- **Inputs**: Symbolic `initial_pnl`, `new_pnl`, `bypass_pnl` with `kani::assume(old_contrib != new_contrib)` to ensure positive-PnL contribution changes.
- **Key assertion**: `!inv_aggregates(&engine)` -- asserts the negation of correctness.
- **Assessment**: Correctly designed. No strengthening possible or needed -- this is a negative proof by definition.

### 2. `kani_cross_lp_close_no_pnl_teleport` (proof #95, line 5484)

- **Classification**: UNIT TEST (concrete oracle limits generality)
- **Purpose**: Migrated from inline proofs. Tests that opening a position at 90K via P90kMatcher with LP1 and closing at oracle with LP2 does not teleport PnL to LP2.
- **Inputs**: Symbolic `cap_mult` [1,100] (multiplied by 1B), symbolic `size` [1,5], but concrete `ORACLE_100K = 100_000_000_000` for both trades.
- **Concrete limitation**: The concrete oracle means:
  - P90kMatcher returns price = `ORACLE_100K - 10_000 * 1_000_000 = 90_000_000_000` (always the same execution price)
  - Mark PnL calculations exercise only one price point
  - Margin checks (both IM=0 and MM=0 for this params set) are trivially satisfied
- **Why not STRONG**: The no-teleport property is specific to the relationship between execution price and oracle price. With concrete oracle, we only prove it for one such relationship. Proof #94 (`kani_no_teleport_cross_lp_close`) covers this property with symbolic oracle and is STRONG.
- **Assessment**: Correctly designed as a migrated inline scenario test. Could be strengthened by making oracle symbolic, but proof #94 already provides the STRONG version of this property.

---

## Audit Methodology Applied

### Criterion 1: Input Classification

Every proof was checked for whether its inputs come from `kani::any()` (symbolic) or hardcoded values (concrete).

**Scaffolding policy applied**: Concrete values that do NOT affect branch coverage in the function-under-test are treated as scaffolding and do not downgrade:
- `engine.current_slot = 100` -- scaffolding for fresh crank
- `engine.last_crank_slot = 100` -- scaffolding for freshness gate
- `engine.last_full_sweep_start_slot = 100` -- scaffolding for sweep gate
- `engine.funding_index_qpb_e6 = I128::new(0)` -- scaffolding for settled state
- Deep capital constants when the proof's focus is extreme oracle/size -- scaffolding
- `last_fee_slot = 50` -- intentional setup to exercise fee accrual branches (not scaffolding, but correctly placed)

### Criterion 2: Branch Coverage

For each proof, the symbolic input ranges were verified against the function-under-test's conditionals:
- **Conservation proofs**: amount > 0 ensures actual transfer occurs
- **Margin proofs**: capital near IM/MM boundary ensures both pass/fail paths
- **Settlement proofs**: both positive and negative PnL paths covered
- **Frame proofs**: other account fields verified unchanged
- **Error-path proofs**: overflow/boundary conditions actually trigger
- **Warmup proofs**: both positive PnL (conversion) and negative PnL (loss settlement) paths
- **Funding proofs**: non-zero position * non-zero delta = non-zero funding effect
- **Haircut proofs**: symbolic vault/c_tot/insurance cover fully-backed, underbacked, and zero-PnL cases
- **Liquidation proofs**: symbolic oracle covers price movement into/out of maintenance margin

### Criterion 3: Invariant Strength

- **`canonical_inv()`** = `inv_structural` + `inv_aggregates` + `inv_accounting` + `inv_mode` + `inv_per_account`
  - `inv_structural`: popcount, freelist acyclicity/disjointness, cursor bounds
  - `inv_aggregates`: c_tot == sum(capital), pnl_pos_tot == sum(max(pnl,0)), OI == sum(|pos|)
  - `inv_accounting`: vault >= c_tot + insurance (primary conservation)
  - `inv_mode`: placeholder (always true in current system)
  - `inv_per_account`: reserved_pnl <= max(pnl,0), no i128::MIN, warmup_slope != u128::MAX
- All preservation proofs use `canonical_inv()` (upgraded from `valid_state()` in prior commits)
- Non-preservation proofs use property-specific assertions (exact formula match, determinism, direction properties) that are appropriate for their specific property
- No proofs use the weaker `valid_state()` as their primary invariant

### Criterion 4: Vacuity Risk

Every proof was checked for:
- `assert_ok!` / `assert_err!` macros ensure the intended code path is reached
- Explicit non-vacuity assertions (e.g., "non-vacuity: conservative trade must succeed")
- `kani::assume` constraints checked for mutual satisfiability
- No hand-built states with impossible field combinations
- `sync_engine_aggregates()` called after manual account field assignment to ensure consistent state

No vacuity issues found. All proofs have at least one of:
1. `assert_ok!` forcing the Ok path to be reachable
2. `assert_err!` forcing the Err path to be reachable
3. `unwrap()` on results (would fail if Err, proving Ok path reached)
4. Explicit non-vacuity sub-assertions for conservative parameter ranges
5. Trivially reachable assertions on pure function outputs

### Criterion 5: Symbolic Collapse

Checked whether derived values collapse symbolic ranges into effectively concrete values:
- **Haircut ratio**: Symbolic vault/c_tot/insurance in C1-C6 proofs ensures h varies between 0 and 1 (not always 1)
- **Warmup cap**: Symbolic slope * elapsed can be both above and below avail_gross
- **Margin thresholds**: Capital near IM/MM boundary, not always above or always below
- **Funding settlement**: Non-zero position * non-zero delta = non-zero effect (verified)
- **Mark PnL**: Symbolic oracle and entry_price create both positive and negative mark PnL values
- **Fee deduction**: Symbolic dt and fee_per_slot create varying fee amounts

No symbolic collapse issues found in any of the 144 STRONG proofs.

---

## Final Summary

```
STRONG:    144 / 146  (98.6%)
WEAK:        0 / 146  ( 0.0%)
UNIT TEST:   2 / 146  ( 1.4%)
VACUOUS:     0 / 146  ( 0.0%)
```

All 146 proofs are correctly designed. The 2 UNIT TEST proofs are intentional:
1. **`proof_NEGATIVE_bypass_set_pnl_breaks_invariant`** -- a meta/negative proof that validates the non-vacuity of real proofs by showing the invariant CAN be broken when helpers are bypassed.
2. **`kani_cross_lp_close_no_pnl_teleport`** -- a migrated inline scenario test with concrete oracle. The STRONG version of this property is covered by `kani_no_teleport_cross_lp_close` (proof #94) which uses symbolic oracle.

No proofs are WEAK or VACUOUS. No proofs require strengthening.

### Changes from Previous Audit

The previous audit classified 7 proofs as UNIT TEST and 139 as STRONG. This fresh re-analysis reclassifies 5 of those former UNIT TEST proofs to STRONG:

| Proof | Previous | Current | Reason for reclassification |
|---|---|---|---|
| `proof_gap4_trade_extreme_price_no_panic` | UNIT TEST | **STRONG** | Symbolic capital [100M,1e15] AND symbolic oracle [1,MAX_ORACLE_PRICE] cover key dimensions |
| `proof_gap4_trade_extreme_size_no_panic` | UNIT TEST | **STRONG** | Symbolic size [1,MAX_POSITION_ABS] AND symbolic oracle [100K,10M]; concrete deep_capital is scaffolding |
| `proof_gap4_trade_partial_fill_diff_price_no_panic` | UNIT TEST | **STRONG** | All four key inputs (user_cap, lp_cap, oracle, size) are symbolic |
| `proof_gap4_margin_extreme_values_no_panic` | UNIT TEST | **STRONG** | All five key inputs (pos, capital, pnl, entry, oracle) are symbolic with canonical_inv |
| `fast_account_equity_computes_correctly` | UNIT TEST | **STRONG** | Symbolic capital and pnl test a pure function; all branches exercised |
