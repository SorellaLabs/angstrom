# Packed ABI Decodable Encoding (PADE? name TBD) (v0.1)

PADE is an encoding format that's meant to be simple, robust and trade-off calldata usage with
decoding cost aiming to be used by Ethereum mainnet contracts. This is the encoding used in
Angstrom's `execute` method to pass in bundle data.

## Types

### Primitive Types

PADE inherits some of [ABI's primitive types](https://docs.soliditylang.org/en/latest/abi-spec.html#types):

**ABI Inherited Types**
- `uint<N>`
- `int<N>`
- `bytes<N>`
- `address`

**Note on convention**

We follow Solidity's convention of having `uint<N>` types be suffixed with their size in **bits**,
and `bytes<N>` to be suffixed with their length in bytes.

Rust-like syntax will used to define the types.

### The List Type (`List<T: type>`)

The `List` type represents a list or array of type `T` where the encoded length **must not** exceed
`16777215` (length must fit in 3 bytes). 

List are encoded by appended the concatenation of the encoding of the elements to a 3 byte length.

### Fixed length arrays (`[T; N]`)

Fixed length arrays require a constant length. These are encoded as the concatenation of the PADE
encoding of the elements.

### Product Types (aka "Structs")

Types can be aliased or composed into larger product types aka "structs" e.g.

```rust
struct Trade {
    asset_in: address,
    asset_out: address,
    quantity: uint64
}

struct Matched {
    asks: List<Trade>,
    bids: List<Trade>,
}

struct String(List<bytes1>);
```

### Sum Types (aka "Enums")

Types can be enums which have distinct variants. Each variant is itself a struct with `n` fields
e.g.:

```rust
enum OrderInvalidation {
    Flash {
        valid_for_block: uint64
    },
    Standing {
        deadline: uint40,
        nonce: uint64
    }
}

enum Option<T: type> {
    None,
    Some(T)
}
```

Note that recursive type definitions are **disallowed** (reasoning: makes specification simpler).


### Builtins

While these types are not first class citizens of the type system they are useful constructs that
will later have to have some functions like the EIP712 struct hashing defined for them.

```rust
enum Option<T: type> {
    None,
    Some(T)
}

enum Bool {
    false,
    true
}
```

## Encoding

### Helper functions

#### `bits(n: int) -> int`

Returns the amount of bits needed to fit an integer `n`.

```python
# Or just `int.bit_length` in python
def bits(n: int) -> int:
    b = 0
    while n > 0:
        n = n >> 1
        b += 1
    return b
```

#### `full_bytes(bits: int) -> int`

Returns the number of full bytes needed to fit a given number of bits.

```python
# Or just `(bits + 7) // 8`
def full_bytes(n: int) -> int:
    total_bytes = bits // 8
    if bits % 8 > 0:
        total_bytes += 1
    return total_bytes
```


#### `pade_encode(x: PadeValue) -> bytes`

Encodes a pade encodable value based on its type.

Note: For enums the `.inner` attribute refers to the value within a given enum variant (the single
field / struct).

```python

LIST_MAX_LENGTH_BYTES = 3

def pade_encode(x: PadeValue, T: PadeType) -> bytes:
    if T.is_abi_primitive():
        return x.abi_encode_packed()
    if T.is_enum():
        variant_size = full_bytes(bits(len(T.variants)))
        variant_bytes = x.variant.to_bytes(variant_size, 'big')
        # For enums `.inner` represents the "inner" value either the
        # value of the unique field or the struct
        return variant_bytes + pade_encode(x.inner)
    if T.is_fixed_array():
        return concat([
            pade_encode(item)
            for item in x.items
        ])
    if T.is_list():
        encoded_items = concat([
            pade_encode(item)
            for item in x.items
        ])
        length_bytes = len(encoded_items).to_bytes(LIST_MAX_LENGTH_BYTES, 'big')
        return length_bytes + encoded_items
    if T.is_struct():
        # the variants of enum fields are packed together
        variant_bitmap: int = 0
        bitmap_size: int = 0
        fields_encoded: bytes = b''

        # Encode fields
        for field_value, field_type in zip(x.values, T.fields):
            if field_type.is_enum():
                variant_size = bits(len(field_type.variants))
                variant: int = field_value.variant
                fields_encoded += pade_encode(field_value.inner)
            else:
                variant_size = 0
                variant: int = 0
                fields_encoded += pade_encode(field_value)

            variant_bitmap |= variant << bitmap_size
            bitmap_size += variant_size

        variant_bytes = variant_bitmap.to_bytes(full_bytes(bitmap_size), 'little')

        return variant_bytes + fields_encoded
```
