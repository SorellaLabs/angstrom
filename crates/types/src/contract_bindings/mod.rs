#[rustfmt::skip]
pub mod angstrom {
    alloy::sol!(
        #[allow(missing_docs)]
        #[sol(rpc)]
        #[derive(Debug,Default, PartialEq, Eq,Hash, serde::Serialize, serde::Deserialize)]
        Angstrom,
        "../../contracts/out/Angstrom.sol/Angstrom.json"
    );
}
#[rustfmt::skip]
pub mod controller_v_1 {
    alloy::sol!(
        #[allow(missing_docs)]
        #[sol(rpc)]
        #[derive(Debug,Default, PartialEq, Eq,Hash, serde::Serialize, serde::Deserialize)]
        ControllerV1,
        "../../contracts/out/ControllerV1.sol/ControllerV1.json"
    );
}
#[rustfmt::skip]
pub mod mintable_mock_erc_20 {
    alloy::sol!(
        #[allow(missing_docs)]
        #[sol(rpc)]
        #[derive(Debug,Default, PartialEq, Eq,Hash, serde::Serialize, serde::Deserialize)]
        MintableMockERC20,
        "../../contracts/out/MintableMockERC20.sol/MintableMockERC20.json"
    );
}
#[rustfmt::skip]
pub mod mock_rewards_manager {
    alloy::sol!(
        #[allow(missing_docs)]
        #[sol(rpc)]
        #[derive(Debug,Default, PartialEq, Eq,Hash, serde::Serialize, serde::Deserialize)]
        MockRewardsManager,
        "../../contracts/out/MockRewardsManager.sol/MockRewardsManager.json"
    );
}
#[rustfmt::skip]
pub mod pool_gate {
    alloy::sol!(
        #[allow(missing_docs)]
        #[sol(rpc)]
        #[derive(Debug,Default, PartialEq, Eq,Hash, serde::Serialize, serde::Deserialize)]
        PoolGate,
        "../../contracts/out/PoolGate.sol/PoolGate.json"
    );
}
#[rustfmt::skip]
pub mod pool_manager {
    alloy::sol!(
        #[allow(missing_docs)]
        #[sol(rpc)]
        #[derive(Debug,Default, PartialEq, Eq,Hash, serde::Serialize, serde::Deserialize)]
        PoolManager,
        "../../contracts/out/PoolManager.sol/PoolManager.json"
    );
}
