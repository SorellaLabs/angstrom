# Crate changes — `angstrom`

| | |
|---|---|
| **Purpose** | Every dependency to add, remove or change so `angstrom` builds on the regular reth release **v2.5.2** (`paradigmxyz/reth`), with the alloy and revm versions that release uses. |
| **Prepared** | 2026-09-25. Manifests read at `origin/main` `3690f91`. **Nothing has been compiled.** |
| **Upstream facts from** | `paradigmxyz/reth` **v2.5.2** and **v2.6.0** `Cargo.toml`; `base/reth` **`base-v2.5.2.6`** compared with v2.5.2; crates.io on 2026-09-25. |

---

## 1. Target stack

`angstrom` uses only regular reth crates, and no Base crates, so it needs no `[patch]`.

| Component | Target | Latest upstream | Note |
|---|---|---|---|
| reth (every `reth-*` git crate) | `paradigmxyz/reth` tag **`v2.5.2`** | v2.6.0 | v2.5.2 is the release Base's reth fork (`base-v2.5.2.6`) is built on. Builds that also contain Base crates redirect these crates to that fork with a `[patch]`, which only works while `angstrom` is on the same release. v2.6.0 moves to revm 43, alloy-evm 0.39 and reth-primitives-traits 0.7. |
| alloy (`alloy`, `alloy-*` 2.x) | **`2.3.0`** | 2.5.0 | reth v2.5.2's floor. Semver-compatible, so a fresh lock may resolve a later 2.x. |
| `alloy-primitives`, `alloy-sol-types`, `alloy-sol-macro`, `alloy-dyn-abi` | **`1.6.1`** | 1.7.3 | reth v2.5.2's floor. |
| `alloy-rlp`, `alloy-rlp-derive` | **`0.3.16`** | 0.3.16 | reth v2.5.2 declares `alloy-rlp` 0.3.16; keep the derive crate in step. |
| `alloy-trie` | **`0.9.4`** | — | reth v2.5.2's floor. |
| `alloy-chains` | `0.2.33` (unchanged) | — | Same as reth v2.5.2. |
| `revm` | **`42.0.1`** | 43.0.3 | reth v2.5.2's version; 43 is a breaking bump. |
| `revm-bytecode`, `revm-database`, `revm-interpreter`, `revm-primitives`, `revm-state` | **`42.0.0`** | 43.x | What revm 42.0.1 itself depends on (`^42.0.0`). |
| `reth-primitives-traits` (crates.io) | **`0.6.0`** | 0.8.1 | reth v2.5.2's version (today's `0.1.0` doesn't match reth 2.x either). |
| `jsonrpsee*` | `0.26.0` (unchanged) | 0.26.0 | Same as reth v2.5.2. |
| Rust | **1.96.0** (required) | — | reth v2.5.2 itself needs 1.95 or newer, and the newest compatible alloy and revm releases need 1.94.1 or older, so 1.96.0 builds the whole stack. |

**Source string.** Use it on every reth crate, and drop the `version = "2.0.0"` / `"1.6.0"` keys:

```toml
{ git = "https://github.com/paradigmxyz/reth", tag = "v2.5.2" }
```

**Keeping the fork redirect working.** Compared with v2.5.2, Base's fork removes three public methods and adds one trait method with a default:
- removed: `EitherWriter::get_last_storage_history_shard`, `EitherWriter::get_last_account_history_shard`, `ValidPoolTransaction::is_underpriced`;
- added: `PoolTransaction::is_replacement_underpriced`, which defaults to the existing check.

`angstrom` calls none of the removed methods (checked on `main`). Keep it that way, and code against v2.5.2 APIs only, so the crates still compile when a build redirects them to the fork.

---

## 2. Toolchain

Rust **1.96.0** is required. `rust-toolchain.toml` pins it for builds in this repo; `rust-version` makes Cargo reject older toolchains for these crates wherever they're built, but only in crates that inherit it with `rust-version.workspace = true`.

| File | Now | Change to |
|---|---|---|
| `rust-toolchain.toml` | `channel = "1.94.0"` | `"1.96.0"` |
| `Cargo.toml` `[workspace.package]` | `rust-version = "1.88.0"` | `"1.96.0"` |
| `[package]` of `crates/types`, `crates/types/constants` and `crates/types/primitives` | no `rust-version` | add `rust-version.workspace = true`; the other 21 crates already inherit it |

---

## 3. `Cargo.toml` `[workspace.dependencies]`

### 3.1 reth

**Change** from `git = "https://github.com/paradigmxyz/reth", version = "2.0.0", tag = "v2.0.0"` to `{ git = "https://github.com/paradigmxyz/reth", tag = "v2.5.2" }` (keep `features = ["serde"]` on `reth-execution-types`; the feature still exists):

`reth`, `reth-chainspec`, `reth-cli-util`, `reth-db`, `reth-discv4`, `reth-ecies`, `reth-errors`, `reth-eth-wire`, `reth-ethereum-primitives`, `reth-execution-types`, `reth-libmdbx`, `reth-metrics`, `reth-network`, `reth-network-api`, `reth-network-peers`, `reth-node-builder`, `reth-node-ethereum`, `reth-node-metrics`, `reth-node-types`, `reth-payload-builder`, `reth-provider`, `reth-revm`, `reth-rpc-builder`, `reth-storage-api`, `reth-tasks`, `reth-tokio-util`, `reth-tracing`, `reth-transaction-pool`, `reth-trie`, `reth-trie-common`.

**Add** (moved here from `crates/angstrom-net`, see §4):

```toml
reth-net-banlist = { git = "https://github.com/paradigmxyz/reth", tag = "v2.5.2" }
reth-network-p2p = { git = "https://github.com/paradigmxyz/reth", tag = "v2.5.2" }
```

**Remove:**

| Crate | Why |
|---|---|
| `reth-codecs` | declared but used by no member crate; in reth 2.x it lives on crates.io (0.6.0), not in the reth repo |
| `reth-rpc-types-compat` | declared but used by no member crate; it doesn't exist in reth v2.0.0 or later |

**Bump:**

| Crate | Now | Change to |
|---|---|---|
| `reth-primitives-traits` | `"0.1.0"` | `"0.6.0"` |

### 3.2 alloy

| Crate | Now | Change to |
|---|---|---|
| `alloy` | `1.8.2` | `2.3.0` (features unchanged; all 15 exist in 2.3.0: `rlp`, `asm-keccak`, `full`, `node-bindings`, `rpc-types-debug`, `rpc-types-trace`, `json-rpc`, `rpc-client`, `signer-keystore`, `signer-ledger`, `signer-mnemonic`, `signer-trezor`, `signer-yubihsm`, `sol-types`, `contract`) |
| `alloy-consensus`, `alloy-contract`, `alloy-eips`, `alloy-genesis`, `alloy-json-rpc`, `alloy-network`, `alloy-node-bindings`, `alloy-provider`, `alloy-pubsub`, `alloy-rpc-client`, `alloy-rpc-types`, `alloy-rpc-types-admin`, `alloy-rpc-types-anvil`, `alloy-rpc-types-beacon`, `alloy-rpc-types-engine`, `alloy-rpc-types-eth`, `alloy-rpc-types-txpool`, `alloy-serde`, `alloy-signer`, `alloy-signer-local`, `alloy-transport`, `alloy-transport-http`, `alloy-transport-ipc`, `alloy-transport-ws` | `1.8.2` | `2.3.0` (features unchanged: `reqwest`, `eth`, `reqwest-rustls-tls` exist in 2.3.0) |
| `alloy-primitives` | `1.5.6` | `1.6.1` (`map-foldhash`, `asm-keccak` exist) |
| `alloy-sol-types`, `alloy-sol-macro`, `alloy-dyn-abi` | `1.5.6` | `1.6.1` |
| `alloy-rlp`, `alloy-rlp-derive` | `0.3.13` | `0.3.16` |
| `alloy-trie` | `0.9.0` | `0.9.4` |
| `alloy-chains` | `0.2.33` | unchanged |

### 3.3 revm

| Crate | Now | Change to |
|---|---|---|
| `revm` | `36.0.0` | `42.0.1` (features unchanged: `std`, `secp256k1`, `optional_balance_check`, `optional_block_gas_limit`) |
| `revm-bytecode` | `9.0.0` | `42.0.0` |
| `revm-database` | `12.0.0` | `42.0.0` |
| `revm-interpreter` | `34.0.0` | `42.0.0` |
| `revm-primitives` | `22.1.0` | `42.0.0` |
| `revm-state` | `10.0.0` | `42.0.0` |

### 3.4 Unchanged

`jsonrpsee`, `jsonrpsee-core`, `jsonrpsee-http-client`, `jsonrpsee-server`, `jsonrpsee-types` stay at `0.26.0`.

---

## 4. `crates/angstrom-net/Cargo.toml`

It declares two reth crates directly, at **reth v1.6.0**, so today's build already mixes reth v2.0.0 with a v1.6.0 subtree.

| Line now | Change to |
|---|---|
| `reth-net-banlist = { git = "https://github.com/paradigmxyz/reth", version = "1.6.0", tag = "v1.6.0" }` | `reth-net-banlist.workspace = true` |
| `reth-network-p2p = { git = "https://github.com/paradigmxyz/reth", version = "1.6.0", tag = "v1.6.0" }` | `reth-network-p2p.workspace = true` |

No other member crate declares reth, alloy or revm outside the workspace.

---

## 5. Checks

```bash
cargo tree -d -e normal | grep -E '^(reth-|alloy-|revm)'   # no second version of any of these, except the alloy 1.8.x set from pade (below)
cargo tree -i reth-provider         # exactly one source: github.com/paradigmxyz/reth?tag=v2.5.2
grep -rnE 'tag = "v(2\.0\.0|1\.6\.0)"' --include=Cargo.toml .   # empty
grep -rnE '\b(is_underpriced|get_last_storage_history_shard|get_last_account_history_shard)\b' --include='*.rs' .   # empty
```

- **Expected alloy 1.x duplicates, from `pade`.** `pade` enables the alloy 1.x umbrella crate with its default features, which include `essentials`, so an alloy 1.8.x set (`alloy-contract`, `alloy-provider`, `alloy-rpc-types`, `alloy-signer-local` and their dependencies) stays in the graph next to alloy 2.
  - It shares no types with the alloy 2 code: `pade` only uses alloy's core types, and `alloy-primitives`/`alloy-sol-types` resolve to one 1.x version for every crate.
  - `cargo tree -i alloy-provider@1.8.3` should show `pade` as the only path. Removing the duplicates takes a change in `pade` itself (`default-features = false` on its `alloy` dependency).
