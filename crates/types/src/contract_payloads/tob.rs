use alloy::sol;
use pade_macro::PadeEncode;

sol! {
    #[derive(PadeEncode, Debug)]
    struct Asset {
        address addr;
        uint128 save;
        uint128 borrow;
        uint128 settle;
    }

    #[derive(Debug, Default, PadeEncode)]
    struct RewardsUpdate {
        #[pade_width(3)]
        int24 startTick;
        uint128 startLiquidity;
        uint128[] quantities;
    }

    #[derive(PadeEncode, Debug)]
    struct PoolRewardsUpdate {
        uint16 asset0;
        uint16 asset1;
        RewardsUpdate update;
    }

    #[derive(PadeEncode, Debug)]
    struct MockContractMessage {
        Asset[] addressList;
        PoolRewardsUpdate update;
    }
}
