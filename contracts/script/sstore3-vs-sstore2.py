def cost_update_store(pools: int):
    total_bytes = 1 + 32 * pools
    deploy_cost = 32_000 + 200 * total_bytes

    # cost of updating store addr, store, overhead
    return deploy_cost + 5_000 + 800


def cost_update_map_naive(pools: int):
    return 5_000 * pools


def cost_store_read(pools: int):
    assert pools >= 1
    return 2600 + (pools - 1) * 100


def cost_map_read(pools: int):
    return 2100 * pools


values = sorted(
    list(
        set(
            update_every_h * 60 / (read_every_blocks * 12 / 60)
            for update_every_h in [1, 4, 8, 12, 24]
            for read_every_blocks in [1, 5, 12, 300]
        )
    )
)


def change_to_map_delta(read_per_update: float, pools: int):
    store_cost = cost_update_store(pools)\
        + read_per_update * cost_store_read(pools)
    map_cost = cost_update_map_naive(pools)\
        + read_per_update * cost_map_read(pools)

    return store_cost - map_cost


print('   |' + '|'.join(
    f'{read_per_update:^11.0f}'
    for read_per_update in values
) + '|')

for pools in [1, 2, 3, 4, 5, 7, 9, 10, 15, 20]:
    changes = [
        change_to_map_delta(read_per_update, pools)
        for read_per_update in values
    ]
    s = '|'.join(
        f'{c:^11.2e}'
        for c in changes
    )
    print(f'{pools:>2}: {s}')
