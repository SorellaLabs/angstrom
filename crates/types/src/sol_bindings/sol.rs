mod private {
    use alloy_sol_macro::sol;
    use bincode::{Decode, Encode};
    use serde::{Deserialize, Serialize};

    sol! {
        #[derive(Debug, Default, PartialEq, Eq,Hash,Serialize, Deserialize)]
        enum OrderMode {
            ExactIn,
            ExactOut,
            #[default]
            Partial
        }

        #[derive(Debug, Default, PartialEq, Eq,Hash,Serialize, Deserialize)]
        enum OrderType {
            Flash,
            #[default]
            Standing
        }

        #[derive(Debug, Default, PartialEq, Eq,Hash,Serialize, Deserialize )]
        enum AssetForm {
            #[default]
            Liquid,
            UniV4Claim,
            AngstromClaim,
        }


        #[derive(Debug, Default, PartialEq, Eq,  Hash, Encode, Decode,Serialize, Deserialize)]
        struct StandingOrder {
            string mode;
            #[bincode(with_serde)]
            uint256 max_amount_in_or_out;
            #[bincode(with_serde)]
            uint256 min_price;
            AssetIndex asset_in;
            #[bincode(with_serde)]
            AssetForm asset_in_form;
            AssetIndex asset_out;
            #[bincode(with_serde)]
            AssetForm asset_out_form;
            #[bincode(with_serde)]
            address recipient;
            #[bincode(with_serde)]
            bytes hook_data;
            uint64 nonce;
            #[bincode(with_serde)]
            uint256 deadline;
            #[bincode(with_serde)]
            bytes signature;
        }

        #[derive(Debug, Default, PartialEq, Eq,Hash,Encode, Decode,Serialize, Deserialize)]
        struct FlashOrder {
            string mode;
            #[bincode(with_serde)]
            uint256 max_amount_in_or_out;
            #[bincode(with_serde)]
            uint256 min_price;
            AssetIndex asset_in;
            #[bincode(with_serde)]
            AssetForm asset_in_form;
            AssetIndex asset_out;
            #[bincode(with_serde)]
            AssetForm asset_out_form;
            #[bincode(with_serde)]
            address recipient;
            #[bincode(with_serde)]
            bytes hook_data;
            uint64 valid_for_block;
            #[bincode(with_serde)]
            bytes signature;
        }


        #[derive(Debug, Default, PartialEq, Eq,Hash,Encode, Decode,Serialize, Deserialize)]
        struct TopOfBlockOrder {
            #[bincode(with_serde)]
            uint256 amountIn;
            #[bincode(with_serde)]
            uint256 amountOut;
            AssetIndex assetInIndex;
            #[bincode(with_serde)]
            AssetForm assetInForm;
            AssetIndex assetOutIndex;
            #[bincode(with_serde)]
            AssetForm assetOutForm;
            #[bincode(with_serde)]
            address recipient;
            #[bincode(with_serde)]
            address hook;
            #[bincode(with_serde)]
            bytes hookPayload;
            #[bincode(with_serde)]
            address from;
            #[bincode(with_serde)]
            bytes signature;
        }

        #[derive(Debug, Default, PartialEq, Eq, Hash, Encode, Decode,Serialize, Deserialize)]
        type AssetIndex is uint16;

        #[derive(Debug, Default, PartialEq, Eq,Hash, Encode, Decode,Serialize, Deserialize)]
        struct GenericOrder {
            #[bincode(with_serde)]
            OrderType otype;
            #[bincode(with_serde)]
            OrderMode mode;
            #[bincode(with_serde)]
            uint256 amountSpecified;
            #[bincode(with_serde)]
            uint256 minPrice;
            AssetIndex assetInIndex;
            #[bincode(with_serde)]
            AssetForm assetInForm;
            AssetIndex assetOutIndex;
            #[bincode(with_serde)]
            AssetForm assetOutForm;
            uint64 nonce;
            #[bincode(with_serde)]
            uint256 deadline;
            #[bincode(with_serde)]
            address recipient;
            #[bincode(with_serde)]
            address hook;
            #[bincode(with_serde)]
            bytes hookPayload;
            #[bincode(with_serde)]
            uint256 amountFilled;
            #[bincode(with_serde)]
            address from;
            #[bincode(with_serde)]
            bytes signature;
        }


        #[derive(Debug, Default, PartialEq, Eq,Hash,Encode, Decode,Serialize, Deserialize)]
        struct Price {
            AssetIndex outIndex;
            AssetIndex inIndex;
            #[bincode(with_serde)]
            uint256 price;
        }

        #[derive(Debug, Default, PartialEq, Eq,Hash,Encode, Decode,Serialize, Deserialize)]
        struct Swap {
            AssetIndex asset0Index;
            AssetIndex asset1Index;
            bool zeroForOne;
            #[bincode(with_serde)]
            uint256 amountIn;
        }

        #[derive(Debug, Default, PartialEq, Eq,Hash,Encode, Decode,Serialize, Deserialize)]
        struct Donate {
            AssetIndex asset0Index;
            AssetIndex asset1Index;
            int24 startTick;
            #[bincode(with_serde)]
            uint256 totalTicks;
            #[bincode(with_serde)]
            uint128 startLiquidity;
            #[bincode(with_serde)]
            uint256[] amounts0;
        }

        #[derive(Debug, Default, PartialEq, Eq,Hash, Serialize, Deserialize)]
        struct ContractBundle {
            address[] assets;
            Price[] initial_prices;
            bytes[] pre_transformations;
            TopOfBlockOrder[] top_of_block_orders;
            Swap[] swaps;
            GenericOrder[] orders;
            bytes[] post_transformations;
            Donate[] donates;
        }

        #[derive(Debug)]
        contract AngstromContract {
            error InsufficientBalance();
            error InsufficientPermission();

            mapping(address owner =>
                mapping(uint256 id => uint256 amount)) public balanceOf;
            mapping(address owner =>
                mapping(address spender =>
                    mapping(uint256 id => uint256 amount))) public allowance;
            mapping(address owner =>
                mapping(address spender => bool)) public isOperator;

            function transfer(address receiver, uint256 id, uint256 amount) public returns (bool);
            function transferFrom(
                address sender,
                address receiver,
                uint256 id,
                uint256 amount
            ) public returns (bool);
            function approve(address spender, uint256 id, uint256 amount) public returns (bool);
            function setOperator(address spender, bool approved) public returns (bool);
            function supportsInterface(bytes4 interfaceId) public pure returns (bool supported);

            function execute(bytes calldata data) external;

            function userToUserLiquidTransfer(
                address from,
                address to,
                address asset,
                uint256 amount
            ) public;
            function userToUserV4ClaimTransfer(
                address from,
                address to,
                address asset,
                uint256 amount
            ) public;
            function pullLiquid(address from, address asset, uint256 amount) public;
            function pushLiquid(address to, address asset, uint256 amount) public;
            function pullToV4(address from, address asset, uint256 amount) public;
            function pushFromV4(address to, address asset, uint256 amount) public;
            function burnV4(address from, address asset, uint256 amount) public;
            function mintV4(address to, address asset, uint256 amount) public;
            function saveNodeFee(address asset, uint256 amount) public;
            function uniV4DeltaToFree(address asset, uint256 amount) public;
            function freeToUniV4Free(address asset, uint256 amount) public;
            function allUniV4FreeToUniV4Delta(address asset) public;
        }
    }

    use std::ops::Deref;
    impl Deref for AssetIndex {
        type Target = u16;

        fn deref(&self) -> &Self::Target {
            &self.0
        }
    }
}

use alloy_rlp::{Decodable, Encodable};
pub use private::{
    AngstromContract, AssetForm as SolAssetForm, AssetIndex, ContractBundle, Donate as SolDonate,
    FlashOrder, GenericOrder as SolGenericOrder, OrderMode as SolOrderMode,
    OrderType as SolOrderType, Price as SolPrice, StandingOrder, Swap as SolSwap, TopOfBlockOrder
};

// RLP encoding implementations needed

impl Encodable for SolAssetForm {
    fn encode(&self, out: &mut dyn bytes::BufMut) {
        match self {
            Self::Liquid => 0_u8.encode(out),
            Self::UniV4Claim => 1_u8.encode(out),
            Self::AngstromClaim => 2_u8.encode(out),
            Self::__Invalid => 3_u8.encode(out)
        }
    }

    fn length(&self) -> usize {
        u8::length(&2_u8)
    }
}

impl Decodable for SolAssetForm {
    fn decode(buf: &mut &[u8]) -> alloy_rlp::Result<Self> {
        let v = u8::decode(buf)?;
        match v {
            0 => Ok(Self::Liquid),
            1 => Ok(Self::UniV4Claim),
            2 => Ok(Self::AngstromClaim),
            3 => Ok(Self::__Invalid),
            _ => Err(alloy_rlp::Error::Custom("Unknown value when decoding AssetForm"))
        }
    }
}

#[derive(Default, Debug, Clone)]
pub struct InvalidSolEnumVariant;

impl std::error::Error for InvalidSolEnumVariant {}

impl std::fmt::Display for InvalidSolEnumVariant {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "InvalidSolEnumVariant")
    }
}

#[cfg(test)]
mod test {
    use alloy_primitives::{Address, U256};
    use alloy_sol_types::{SolCall, SolStruct, SolType};

    use super::*;

    #[test]
    fn test_get_eip712_names() {
        println!("TopOfBlockOrder::NAME: {}", TopOfBlockOrder::NAME);
        println!("TopOfBlockOrder::encode_type: {}", TopOfBlockOrder::eip712_encode_type());
        println!("ToB::SOL_NAME: {}", TopOfBlockOrder::SOL_NAME)
    }

    #[test]
    fn test_encode_ang_call() {
        let x = AngstromContract::pullLiquidCall::new((
            Address::parse_checksummed("0xC5EfE216CC029dB7F3C4eC68B82FBC56bbD561b8", None).unwrap(),
            Address::parse_checksummed("0x6DC10D390e3D0517F4A2ebb88939b12d0a9a17F3", None).unwrap(),
            U256::from(238)
        ))
        .abi_encode();

        println!("x: {:x?}", x);
    }
}
