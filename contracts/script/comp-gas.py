tob_cost = 16700 + 1000

# efi = Exact Flash order, Internal balances
efi_var = 19400
efi_amm_total_1 = 154853
efi_amm_fixed = efi_amm_total_1 - tob_cost - efi_var
efi_solo_total_1 = 82770
efi_solo_fixed = efi_solo_total_1 - tob_cost - efi_var

# esln = Exact Standing order, Liquid token balances, Non-zero starting nonce
esln_var = 32400
esln_amm_total_1 = 167916
esln_amm_fixed = esln_amm_total_1 - tob_cost - esln_var
esln_solo_total_1 = 110435
esln_solo_fixed = esln_solo_total_1 - tob_cost - esln_var

v3_gas = 140_000


def fmt(x: float) -> str:
    multip = x / v3_gas
    return f'{multip:7.1%}'


for i in (1, 2, 3, 4, 5, 10, 20, 40, 50):
    efi_amm = efi_var + efi_amm_fixed / i
    efi_solo = efi_var + efi_solo_fixed / i
    esln_amm = esln_var + esln_amm_fixed / i
    esln_solo = esln_var + esln_solo_fixed / i
    print(f'|{i:2}| {fmt(efi_amm)} | {fmt(efi_solo)} | {fmt(esln_amm)} | {fmt(esln_solo)} |')
