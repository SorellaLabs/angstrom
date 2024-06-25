# Storage Layouts

> Documents the various storage layouts to ensure things don't collide with each other *or* with
> Solidity-native layouts.

## Conventions

For the limited amount of persistent and transient storage all derivations (mostly custom) will be
documented here. The custom schemes are collectively documented here for easy reference and to
ensure that no schemes may collide, the mapping specific slot constant serve as a sep

Slot constants are given by taking the hash of the string `"angstrom-v1_0.<name>.slot"`.

## Overview


|Description|Slot Constant (name)|Pre-image bytes|
|-----|-------------|--------------|
|Unordered Nonces Bitmap Map|`0xdaa050e9` (`unordered-nonces`)|31|

## Mappings

### Unordered Nonces Mapping (`(address owner, uint64 nonce) => bool nonce_set`)
**name:** `unordered-nonces`

**slot:** `0xdaa050e9` (`keccak256("angstrom-v1_0.unordered-nonces.slot")`)

**map:** `(address owner, bytes7 bitmap_word_index) => uint256 bitmap`

**slot derivation:** `slot = keccak256(owner || SLOT || bitmap_word_index)`

**specification:**

```python
SLOT: bytes4 = 0xdaa050e9

def get_bitmap_peristent_slot(owner: address, bitmap_word_index: bytes7) -> bytes32:
    return keccak256(owner.to_bytes() + SLOT + bitmap_word_index)

def get_nonce_is_set(owner: address, nonce: uint64) -> bool:
    bitmap_word_index: bytes7 = bytes7(nonce >> 8)
    nonce_bit: uint256 = nonce & 0xff
    bitmap_slot: bytes32 = get_bitmap_peristent_slot(owner, bitmap_word_index)
    return sload_uint(bitmap_slot) & (1 << nonce_bit) != 0

def set_nonce(owner: address, nonce: uint64):
    bitmap_word_index: bytes7 = bytes7(nonce >> 8)
    nonce_bit: uint256 = nonce & 0xff
    bitmap_slot: bytes32 = get_bitmap_peristent_slot(owner, bitmap_word_index)
    bitmap: uint256 = sload_uint(bitmap_slot)
    sstore_uint(bitmap_slot, bitmap | (1 << nonce_bit))
```
