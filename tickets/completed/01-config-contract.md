# 01 — AngstromProtocolFeeConfig contract

**Blocks on:** —

## Files
- `contracts/src/periphery/AngstromProtocolFeeConfig.sol` (new)
- `contracts/src/periphery/ControllerV1.sol` — auth target, unchanged
- `contracts/src/periphery/AngstromView.sol` — `controller()` lookup, unchanged
- `contracts/src/interfaces/IAngstromAuth.sol` — `extsload`, unchanged

## Goal
Hold `userLpShareE6` and `tobLpShareE6` on chain, settable by the controller's owner or fast owner.

## Do
- `contracts/src/periphery/AngstromProtocolFeeConfig.sol`.
- Two `uint32` private fields packed into slot 0, user in bytes 0..4, tob in bytes 4..8.
- `setLpDonationSplits(uint32,uint32)` — auth via `ControllerV1(ANGSTROM.controller())`, fast owner
  checked first so a fast-owner call never depends on the owner lookup. Bounds `0..=1_000_000`.
- `getLpDonationSplits()`, `angstrom()`, `controller()` — views only.
- No withdrawal path, no fallback, no receive.

## Done when
- `forge build` and `forge fmt --check` clean.
- `forge inspect ... storageLayout` shows slot 0 offsets 0 and 4; immutable takes no slot.
- ABI has exactly one state-changing function and three views.
