use serde::{Deserialize, Serialize};

alloy_sol_macro::sol! {

    #[derive(Debug, Default, PartialEq, Eq, Hash, Serialize, Deserialize, PartialOrd, Ord)]
    struct OrderMeta {
        bool isEcdsa;
        address from;
        bytes signature;
    }

    #[derive(Debug, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
    struct PartialStandingOrder {
        uint32 ref_id;
        uint128 min_amount_in;
        uint128 max_amount_in;
        uint128 max_extra_fee_asset0;
        uint256 min_price;
        bool use_internal;
        address asset_in;
        address asset_out;
        address recipient;
        bytes hook_data;
        uint64 nonce;
        uint40 deadline;
        OrderMeta meta;
    }

    #[derive(Debug, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
    struct ExactStandingOrder {
        uint32 ref_id;
        bool exact_in;
        uint128 amount;
        uint128 max_extra_fee_asset0;
        uint256 min_price;
        bool use_internal;
        address asset_in;
        address asset_out;
        address recipient;
        bytes hook_data;
        uint64 nonce;
        uint40 deadline;
        OrderMeta meta;
    }

    #[derive(Debug, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
    struct PartialFlashOrder {
        uint32 ref_id;
        uint128 min_amount_in;
        uint128 max_amount_in;
        uint128 max_extra_fee_asset0;
        uint256 min_price;
        bool use_internal;
        address asset_in;
        address asset_out;
        address recipient;
        bytes hook_data;
        uint64 valid_for_block;
        OrderMeta meta;
    }

    #[derive(Debug, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
    struct ExactFlashOrder {
        uint32 ref_id;
        bool exact_in;
        uint128 amount;
        uint128 max_extra_fee_asset0;
        uint256 min_price;
        bool use_internal;
        address asset_in;
        address asset_out;
        address recipient;
        bytes hook_data;
        uint64 valid_for_block;
        OrderMeta meta;
    }

    #[derive(Debug, Default, PartialEq, Eq, Hash, Serialize, Deserialize, PartialOrd, Ord)]
    struct TopOfBlockOrder {
        uint128 quantity_in;
        uint128 quantity_out;
        uint128 max_gas_asset0;
        bool use_internal;
        address asset_in;
        address asset_out;
        address recipient;
        uint64 valid_for_block;
        OrderMeta meta;
    }

    #[derive(Debug, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
    struct AttestAngstromBlockEmpty {
        uint64 block_number;
    }
}
