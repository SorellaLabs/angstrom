# 41 — Make DonationResidual::total() checked

**Blocks on:** —
**Closes:** ISSUES.md 5
**Follows:** 29, 30

## Files
- `crates/types/src/uni_structure/donation.rs:88-92` — `DonationResidual::total`
- `crates/types/src/uni_structure/donation.rs:327` — `check_conservation`, the caller

## Goal
No addition in the conservation path can wrap into agreement with the check.

## Do
- `total()` is `self.rounding + self.unplaced` — unchecked. Return `Option<u128>` via `checked_add`,
  or have `check_conservation` fold the two buckets into its own `checked_add` chain rather than
  calling `total()`.
- Keep the error message naming the residual total, so a failure still reads the way ticket 30's
  does.

## Done when
- Every addition reachable from `check_conservation` is checked.
- A residual whose two buckets sum past `u128::MAX` fails the check instead of wrapping.

## Notes
Ticket 30 step 3 says "Use checked arithmetic in the sums — a `u128` overflow must fail the check,
not wrap into agreement." `check_conservation` honours that for `placed`, `protocol_fee` and the
residual total it is handed, and `sum_donations` folds with `checked_add` — but the one addition
inside `total()` slipped through.

**Not reachable as constructed.** Every exit in `t0_donation_vec` sets exactly one bucket nonzero
(`{rounding: 0, unplaced: total_donation}` at the two early exits, `{rounding: remaining, unplaced:
0}` at the normal one), and ticket 31's book arm sets `{rounding: 0, unplaced: book_budget}`. So one
of the two is always zero and the sum cannot overflow today.

Worth closing anyway: the entire point of the surrounding code is that an overflow must not wrap
into agreement with the equality that is supposed to catch it. A future exit that populates both
buckets — which is exactly what ticket 40 might introduce, or what a third residual category would —
would make this reachable, and it would fail silently by producing a total that balances.

`sum_donations` already has the shape to copy.

**As built.** The first of the two options: `total()` returns `Option<u128>` via `checked_add`, and
`check_conservation` binds `residual_total` from it up front, failing with
`"{source} residual overflowed"`, before chaining the existing `checked_add`s over `placed` and
`protocol_fee`. The mismatch message still names the residual total, now from the bound value. The
one caller outside `check_conservation`, `allocation_conserves_its_budget` in `pool_swap.rs`,
unwraps. That is the whole diff.

Coverage: `a_residual_past_u128_max_fails_rather_than_wrapping` hands `check_conservation`
`{ rounding: u128::MAX, unplaced: 1 }` against a placed, fee and gross of zero and asserts `Err`.
Before this change the unchecked sum would have wrapped to `0`, `0 + 0 + 0 == 0` would have
balanced, and the check would have returned `Ok` - in a release build, where overflow checks are
off. In the test profile (inherits `dev`, overflow checks on, nothing in `Cargo.toml` overrides
them) it would have panicked at the addition instead. Either way the check never produced the
`Err` it exists to produce. Both were confirmed by mutation: with `total()` reverted to
`self.rounding + self.unplaced` the test fails with `attempt to add with overflow` at the addition;
with `wrapping_add` (the release behavior) it fails at the assertion because `check_conservation`
returned `Ok`. Both reverts were restored and `git diff --stat` matched the pre-mutation snapshot.

Verification: `cargo nextest run -p angstrom-types donation` - 7 passed;
`cargo nextest run -p angstrom-types pool_swap` - 4 passed; `cargo +nightly fmt -p angstrom-types`
- no changes; `cargo clippy -p angstrom-types --all-targets -- -D warnings -A clippy::result_large_err
-A mismatched_lifetime_syntaxes` - clean. The two allowed lints fail the unqualified `-D warnings`
run before it reaches anything here and are pre-existing on `main` in files this ticket does not
touch: `clippy::result_large_err` in
`crates/uniswap-v4/src/uniswap/pool_providers/{mock_block_stream,provider_adapter}.rs` and the
rustc `mismatched_lifetime_syntaxes` lint at `crates/types/src/matching/uniswap/poolsnapshot.rs:226`.
