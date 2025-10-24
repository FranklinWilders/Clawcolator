#!/bin/bash
# Run all medium-complexity Kani proofs
# Target: Each proof <60s

set -e

echo "Running medium-complexity Kani proofs..."
echo "=========================================="
echo ""

PROOFS=(
    "i2_conservation_2users_deposit_withdraw"
    "i2_conservation_deposit_socialize_withdraw"
    "i4_socialization_2users_symbolic_deficit"
    "i4_socialization_both_winners"
    "i5_throttle_symbolic_step_and_amount"
    "i5_throttle_larger_steps"
    "deposit_2users_symbolic"
    "withdrawal_2users_symbolic"
    "i3_multiuser_unauthorized"
    "i1_principal_inviolability_multi_ops"
    "i6_matcher_symbolic_2users"
)

PASSED=0
FAILED=0
TIMEOUT=0

for proof in "${PROOFS[@]}"; do
    echo "Running: $proof"
    START=$(date +%s)

    # Run proof and capture output
    OUTPUT=$(timeout 60 cargo kani -p proofs-kani --harness "$proof" 2>&1)
    EXIT_CODE=$?

    if [ $EXIT_CODE -eq 124 ]; then
        echo "⏱️  $proof TIMEOUT (>60s)"
        ((TIMEOUT++))
    elif echo "$OUTPUT" | grep -q "VERIFICATION:- SUCCESSFUL"; then
        END=$(date +%s)
        DURATION=$((END - START))
        echo "✅ $proof PASSED (${DURATION}s)"
        ((PASSED++))
    else
        echo "❌ $proof FAILED"
        ((FAILED++))
    fi
    echo ""
done

echo "=========================================="
echo "Results: $PASSED passed, $FAILED failed, $TIMEOUT timeout"

if [ $FAILED -eq 0 ] && [ $TIMEOUT -eq 0 ]; then
    echo "✅ All medium proofs VERIFIED!"
    exit 0
else
    echo "⚠️  Some proofs had issues"
    exit 1
fi
