from attrs import define


@define
class Bid:  # Buy
    amount: float
    remainder: float
    partial: bool
    price: float


@define
class Ask:  # Sell
    amount: float
    remainder: float
    partial: bool
    price: float


def clear(bids: list[Bid], asks: list[Ask]) -> tuple[float, float]:
    quantity = 0
    price = 0
    asks.sort(key=lambda a: (a.price, a.amount))
    print(f'asks: {asks}')
    bids.sort(key=lambda b: (b.price, b.amount), reverse=True)
    print(f'bids: {bids}')
    ai = 0
    bi = 0

    while ai < len(asks) and bi < len(bids) and (ask := asks[ai]).price <= (bid := bids[bi]).price:
        matched = min(ask.amount, bid.amount)
        quantity += matched
        excess = bid.amount - ask.amount
        if excess == 0:
            price = (ask.price + bid.price) / 2
            ai += 1
            bi += 1
        elif excess > 0:
            price = bid.price
            ai += 1
        else:
            price = ask.price
            bi += 1
        bid.remainder -= matched
        ask.remainder -= matched

    if len(asks) > 0:
        ai = min(ai, len(asks) - 1)

    if len(bids) > 0:
        bi = min(bi, len(bids) - 1)

    return quantity, price


def split(orders: list[Ask | Bid]) -> tuple[list[Bid], list[Ask]]:
    bids = []
    asks = []
    for o in orders:
        if isinstance(o, Ask):
            asks.append(o)
        else:
            bids.append(o)

    return bids, asks


def main():
    orders = [
        Ask(50.0, 0.9),
        Ask(100.0, 1.0),
        Bid(100.0, 1.0)
    ]
    quantity, price = clear(*split(orders))

    print(f'quantity: {quantity}')
    print(f'price: {price}')

    print(f'orders:\n{orders}')


if __name__ == '__main__':
    main()
