// SPDX-License-Identifier: MIT
pragma solidity ^0.8.4;

interface IControllerV1 {
    event NewControllerCancelled();
    event NewControllerConfirmed(address indexed new_controller);
    event NewControllerScheduled(address indexed new_controller, uint256 confirmable_at);
    event NodeAdded(address indexed node);
    event NodeRemoved(address indexed node);
    event OwnershipTransferred(address indexed previous_owner, address indexed new_owner);
    event PoolConfigured(
        address indexed asset_a, address indexed asset_b, uint16 tick_spacing, uint24 fee_in_e6
    );
    event PoolRemoved(address indexed asset_a, address indexed asset_b);

    function ANGSTROM() external view returns (address);
    function SCHEDULE_TO_CONFIRM_DELAY() external view returns (uint256);
    function add_node(address node) external;
    function cancel_pending_controller() external;
    function configure_pool(address asset_a, address asset_b, uint16 tick_spacing, uint24 fee_in_e6)
        external;
    function confirm_pending_controller() external;
    function nodes(uint256 arg0) external view returns (address);
    function owner() external view returns (address);
    function pending_controller() external view returns (address);
    function pools(bytes27 arg0) external view returns (uint16, uint24);
    function remove_node(address node) external;
    function remove_pool(address expected_store, address asset_a, address asset_b) external;
    function schedule_new_controller(address new_controller) external;
    function scheduled_at() external view returns (uint256);
    function total_nodes() external view returns (uint256);
    function transfer_ownership(address new_owner) external;
}
