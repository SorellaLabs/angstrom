use alloy::sol;
use pade_macro::{PadeDecode, PadeEncode};

sol! {
    #[derive(PadeEncode, PadeDecode, Debug)]
    struct Asset {
        address addr;
        uint128 borrow;
        uint128 save;
        uint128 settle;
    }

    #[derive(Debug, Default, PadeEncode, PadeDecode)]
    struct RewardsUpdate {
        #[pade_width(3)]
        int24 startTick;
        uint128 startLiquidity;
        uint128[] quantities;
    }

    #[derive(PadeEncode, PadeDecode, Debug)]
    struct PoolRewardsUpdate {
        uint16 asset0;
        uint16 asset1;
        RewardsUpdate update;
    }

    #[derive(PadeEncode,PadeDecode, Debug)]
    struct MockContractMessage {
        Asset[] addressList;
        PoolRewardsUpdate update;
    }
}
