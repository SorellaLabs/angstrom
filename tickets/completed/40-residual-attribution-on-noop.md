# 40 — Attribute the retained budget to the bucket that actually got it

**Blocks on:** —
**Closes:** ISSUES.md 4
**Follows:** 29, 30, 31

## Files
- `crates/types/src/traits/bundles.rs:443` — the `ucp.is_zero()` book arm, ticket 31 step 2
- `crates/types/src/traits/bundles.rs:497-500` — `total_donation`'s `unwrap_or(book_budget)` fallback
- `crates/types/src/traits/bundles.rs:564` — the reward-stage `allocate`
- `crates/types/src/traits/bundles.rs:571-579` — the `RewardsUpdate::CurrentOnly` fallback
- `crates/types/src/uni_structure/donation.rs:81-92` — `DonationResidual`

## Goal
The residual says who got the money.

## Do
- When `book_donation_vec` and `tob_donation_vec` are **both** `None`, the book budget is not
  retained: `total_donation` falls back to `book_budget`, that amount is allocated at the Reward
  stage, and it is emitted as `RewardsUpdate::CurrentOnly { amount: total_donation }` — it goes to
  LPs. Report it as placed, not as `unplaced`.
- Keep the `(None, Some(tob))` arm exactly as it is: there the book budget genuinely stays in
  `contract_liquid` and `collect_extra` sweeps it into `save`, which is what ticket 31 step 2 is
  for and what `book_noop_after_tob_move` asserts.
- Keep conservation holding in both arms. Do not change allocation policy, `save_amount`, or which
  path the money takes — only which bucket reports it.

## Done when
- A `(None, None)` pool reports the book budget as placed with LPs, matching the `CurrentOnly`
  reward the same call emits.
- A `(None, Some(tob))` pool still reports it as `unplaced`.
- A test drives both arms and fails if the two are swapped.

## Notes
PLAN.md's Conservation section requires each bucket be "attributed to documented allocation steps"
and "accounted separately from each other and from the configured protocol fee". Today the
`(None, None)` case reports `unplaced == book_budget` — the bucket that means "the protocol kept it
via `collect_extra`" — for an amount that was donated to the current tick.

**No settlement is wrong.** The equality `0 placed + book_budget unplaced == book_budget` still
holds, no bundle is rejected, and the on-chain result is unchanged. The defect is in the ledger, not
the money.

**It is benign today**, which is why no test caught it: `solution.ucp.is_zero()` implies no filled
limit orders and no book surplus, so `total_user_fees` and `solution.reward_t0` are both zero and
`book_budget` is zero. Mis-attributing zero costs nothing.

It stops being benign for the step-5 reconciliation component, which PLAN.md names as the residual's
intended consumer: "reconstruct the expected allocations from each bundle's construction parent and
the rates in force there, then compare them with the included reward updates and saved amounts." A
consumer that trusts `unplaced` to mean "protocol retained" would mis-derive an accrual on any pool
where this arm is ever reached with a nonzero budget.

Ticket 29's distinction is by *exit*, not by inspecting numbers — an allocator that never ran reports
`unplaced`, one that ran but could not place the last units reports `rounding`. The `(None, None)`
case is a third thing: no allocator ran, but the budget was still placed, by the `CurrentOnly`
fallback rather than by the allocator. That is the shape the residual has no vocabulary for, and the
reason to fix it at the call site rather than in `t0_donation_vec`.

The `unwrap_or(book_budget)` fallback at `:497` and the `CurrentOnly` fallback at `:571` are both
unchanged from `main` — do not "fix" them here. Only the residual is wrong.

**As built.** The `ucp.is_zero()` arm of the book residual match is now two arms keyed on
`tob_swap_info`: `None if tob_swap_info.is_none()` reports `DonationResidual::default()`, and the
remaining `None` (a ToB vector exists) keeps ticket 31's `{ rounding: 0, unplaced: book_budget }`.
Nothing else moved: `total_donation`'s `unwrap_or(book_budget)`, the `CurrentOnly` fallback,
`save_amount`, allocation policy and the whole ToB side are `main`'s and are untouched, so the
money takes the same path as before and only the ledger changed.

The book conservation check no longer derives `placed` from the same condition as the residual.
It reads a `book_placed` binding from the emission side: `sum_donations` of the book vector when
there is one, `book_budget` when neither source produced a vector (the `CurrentOnly` fallback
places the whole budget), else `0`. Deriving `placed` from the vectors and the residual from the
swap info is deliberate: if the two residual arms are ever swapped, the check fails in both cases
instead of staying balanced by construction. Confirmed by mutation: with the arms swapped,
`book_budget_with_no_vectors_is_placed_at_the_current_tick` fails with
`book placed 5000 + fee 0 + residual 5000 != gross 5000` and `book_noop_after_tob_move` fails with
`book placed 0 + fee 0 + residual 0 != gross 5000`. Restored; `git diff --stat` matched the
pre-mutation snapshot.

**The `(None, None)` arm was benign today.** A real solution with `ucp.is_zero()` has no filled
limit orders and no book surplus, so `total_user_fees` and `reward_t0` are zero and `book_budget`
is zero; the old arm mis-attributed nothing. The new test reaches the arm with a synthetic nonzero
budget instead: `book_budget_with_no_vectors_is_placed_at_the_current_tick` builds a
`PoolSolution` with `ucp: Ray::ZERO`, `searcher: None`, `reward_t0: 5_000` and no orders, and
asserts the emitted reward is `CurrentOnly { amount: 5_000, expected_liquidity:
snap.current_liquidity() }`, that `rewarded(0) == 5_000`, and that `save(T0) == 0`. The budget
went to LPs, and the call only returns `Ok` because the residual reports it as placed.
`book_noop_after_tob_move` already drove the `(None, Some(tob))` arm and is unchanged apart from
one sentence in its doc comment saying it pins the retained arm.

Verification: `cargo nextest run -p angstrom-types bundles` - 5 passed;
`cargo nextest run -p angstrom-types donation` - 7 passed; `cargo +nightly fmt -p angstrom-types`
- no changes; `cargo clippy -p angstrom-types --all-targets -- -D warnings -A clippy::result_large_err
-A mismatched_lifetime_syntaxes` - clean. The two allowed lints are pre-existing on `main` in files
outside this ticket; ticket 41's notes name them.
