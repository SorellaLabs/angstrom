# 36 — Record per-bundle fee telemetry

**Blocks on:** 27

## Overview
The per-pool fee numbers are all local to `process_solution` and none of them survive the call,
so nothing downstream can say what a bundle intended. This ticket returns them instead of
emitting from inside, because `process_solution` also runs for gas probes and a telemetry record
per probe is wrong. One new `TelemetryMessage` variant carries them, emitted once per bundle
from `try_build_proposal`, where the round identity is in scope. Construction parent and round
generation stay separate fields on purpose: the generation is local bookkeeping that means
nothing to another node, while the parent hash is what makes the record reproducible off this
one. This is proposal-time telemetry describing intent — ticket 38 uses it as reconstruction
input and cross-checks it against chain state, never as the source of truth. If the ledger
trusted it, a mis-split bundle would reconcile against its own wrong arithmetic.

## Files
- `crates/telemetry-recorder/src/lib.rs:45` — `TelemetryMessage`
- `crates/types/src/traits/bundles.rs` — where the numbers are produced
- `crates/consensus/src/rounds/proposal.rs:102` — `try_build_proposal`, where the round identity is

## Goal
Feed the ledger.

## Do

1. **Carry the numbers out of `process_solution`.** They are all local to it and none survive
   today. Return them rather than emitting from inside — `process_solution` runs for gas probes as
   well as real construction, and a telemetry record per probe is wrong:

```rust
pub struct PoolFeeRecord {
    pub pool_id:           PoolId,
    pub token0:            Address,
    pub gross_tob_reward:  u128,
    pub tob_lp_allocated:  u128,
    pub tob_protocol_fee:  u128,
    pub tob_residual:      DonationResidual,
    pub total_user_fees:   u128,
    pub user_lp_allocated: u128,
    pub user_protocol_fee: u128,
    pub book_residual:     DonationResidual
}
```

2. **Add one variant**, beside the existing ones at `:45`:

```rust
BundleFees {
    blocknum:           u64,          // the H+1 the bundle targets
    construction_parent: BlockNumHash, // the H it was built on
    round_generation:   u64,
    splits:             DonationSplits,
    pools:              Vec<PoolFeeRecord>
}
```

3. **Emit once per bundle**, from `try_build_proposal` where the round identity is in scope — not
   per pool and not from inside `process_solution`.

4. `DonationResidual` and `PoolFeeRecord` need `Serialize` / `Deserialize`;
   `telemetry-recorder` will need `alloy-primitives` back for `BlockNumHash`.

## Done when
- An included bundle's numbers can be reconstructed from the record alone.
- No record is emitted for a gas probe.

## Notes
**Construction parent and round generation are separate fields on purpose.** The generation is
local bookkeeping — another node replaying this bundle has no idea what this node's counter was
at. The parent hash is what reproduces the check. Collapsing them into one identity makes the
record unreconstructible off this node, which is the thing ticket 38 needs it for.

This is proposal-time telemetry: it records what the builder *intended*. Ticket 37 derives accruals
from canonical included bundles and must not read this as the source of truth — it is the
reconstruction input, cross-checked against chain state, which is why 38 can detect a mismatch at
all. If the ledger trusted this record, a mis-split bundle would reconcile against its own wrong
arithmetic.

Ticket 18 deleted the last per-change telemetry variant in favour of one derived surface. This is
not a reversal of that: the config *state* is still derived from `EthUpdaterSnapshot`
(`eth/telemetry.rs:39`); this records per-bundle *amounts*, which no snapshot carries.
