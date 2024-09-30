# /// script
# requires-python = ">=3.12"
# dependencies = []
# ///

import argparse


def i128(x: int) -> int:
    if x >= (1 << 127):
        return x - (1 << 128)
    return x


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument('signed_balance_delta', type=str, help='balance delta')
    args = parser.parse_args()
    delta: str = args.signed_balance_delta

    if delta.startswith('0x'):
        # (1 << 128) + value = twos_complement
        #
        amount0 = i128(int(delta[2:-32], 16))
        amount1 = i128(int(delta[-32], 16))
    else:
        packed = int(delta)
        amount0, amount1 = map(i128, divmod(packed, 1 << 128))
    print(f'amount0: {amount0}')
    print(f'amount1: {amount1}')


if __name__ == '__main__':
    main()
