# SPDX-License-Identifier: MIT
# pragma version 0.4.0

from ethereum.ercs import IERC20
from snekmate.auth import ownable

initializes: ownable
exports: (
    ownable.transfer_ownership,
    ownable.owner,
)

interface IAngstromAuth:
    def setController(new_controller: address): nonpayable
    def removePool(expected_store: address, store_index: uint256): nonpayable
    def configurePool(asset_a: address, asset_b: address, tick_spacing: uint16, fee_in_e6: uint24): nonpayable
    def toggleNodes(nodes: DynArray[address, 1]): nonpayable
    def extsload(slot: bytes32) -> bytes32: view
    # `pullFee` ommitted to prevent contract from actually pulling for now

event NewControllerScheduled:
    new_controller: indexed(address)
    confirmable_at: uint256

event NewControllerCancelled:
    pass

event NewControllerConfirmed:
    new_controller: indexed(address)

event PoolConfigured:
    asset_a: indexed(address)
    asset_b: indexed(address)
    tick_spacing: uint16
    fee_in_e6: uint24

event PoolRemoved:
    asset_a: indexed(address)
    asset_b: indexed(address)

event NodeAdded:
    node: indexed(address)

event NodeRemoved:
    node: indexed(address)

struct Pool:
    tick_spacing: uint16
    fee_in_e6: uint24

ANGSTROM: public(immutable(IAngstromAuth))
SCHEDULE_TO_CONFIRM_DELAY: public(constant(uint256)) = 2 * 7 * 24 * 60 * 60
CONTROLLER_NOT_PENDING: constant(address) = empty(address)
MAX_POOLS: constant(uint256) = 767
MAX_NODES: constant(uint256) = 100
STORE_KEY_SHIFT: constant(uint256) = 40


# New Controller Timelock State
pending_controller: public(address)
scheduled_at: public(uint256)
pools: public(HashMap[bytes27, Pool])
# Iterable HashSet[address]
nodes: public(DynArray[address, MAX_NODES])
_node_index: HashMap[address, uint256]

@deploy
def __init__(angstrom: IAngstromAuth, initial_owner: address):
    ANGSTROM = angstrom
    ownable.__init__()
    ownable.owner = initial_owner

@external
def schedule_new_controller(new_controller: address):
    ownable._check_owner()

    assert new_controller != CONTROLLER_NOT_PENDING, "Can't be sentinel value"

    self.pending_controller = new_controller
    self.scheduled_at = block.timestamp

    log NewControllerScheduled(
        new_controller,
        block.timestamp + SCHEDULE_TO_CONFIRM_DELAY
    )

@external
def cancel_pending_controller():
    ownable._check_owner()

    assert self.pending_controller != CONTROLLER_NOT_PENDING, "Controller not pending"

    self.pending_controller = CONTROLLER_NOT_PENDING
    self.scheduled_at = 0

    log NewControllerCancelled()

@external
def confirm_pending_controller():
    new_controller: address = self.pending_controller
    assert new_controller != CONTROLLER_NOT_PENDING, "Controller not pending"

    confirmable_at: uint256 = self.scheduled_at + SCHEDULE_TO_CONFIRM_DELAY
    assert confirmable_at <= block.timestamp, "New controller still pending"

    self.pending_controller = CONTROLLER_NOT_PENDING
    self.scheduled_at = 0

    log NewControllerConfirmed(new_controller)

    extcall ANGSTROM.setController(new_controller)

@external
def configure_pool(
    asset_a: address,
    asset_b: address,
    tick_spacing: uint16,
    fee_in_e6: uint24
):
    ownable._check_owner()

    key: bytes27 = self._compute_partial_key(asset_a, asset_b)
    self.pools[key] = Pool(tick_spacing=tick_spacing, fee_in_e6=fee_in_e6)

    log PoolConfigured(asset_a, asset_b, tick_spacing, fee_in_e6)

    extcall ANGSTROM.configurePool(asset_a, asset_b, tick_spacing, fee_in_e6)


@external
def remove_pool(
    expected_store: address,
    asset_a: address,
    asset_b: address
):
    ownable._check_owner()

    assert convert(asset_a, uint256) < convert(asset_b, uint256), "Assets not sorted or unique"

    log PoolRemoved(asset_a, asset_b)

    extcall ANGSTROM.removePool(expected_store, 0)

@external
def add_node(node: address):
    ownable._check_owner()

    assert self._node_index[node] == 0, "Node already added"
    self.nodes.append(node)
    self._node_index[node] = len(self.nodes)

    log NodeAdded(node)

    extcall ANGSTROM.toggleNodes([node])

@external
def remove_node(node: address):
    ownable._check_owner()

    index_plus_one: uint256 = self._node_index[node]
    assert index_plus_one != 0, "Node not added"
    self._node_index[node] = 0

    total_nodes: uint256 = len(self.nodes)
    if index_plus_one < total_nodes:
        last_node: address = self.nodes.pop()
        self.nodes[index_plus_one - 1] = last_node
        self._node_index[last_node] = index_plus_one
    else:
        self.nodes.pop()

    log NodeRemoved(node)

    extcall ANGSTROM.toggleNodes([node])

@view
@external
def total_nodes() -> uint256:
    return len(self.nodes)

@view
def _config_store() -> address:
    slot2: bytes32 = staticcall ANGSTROM.extsload(convert(2, bytes32))
    bits: uint256 = convert(slot2, uint256)
    return convert((bits << 32) >> 64, address)

@pure
def _compute_partial_key(asset_a: address, asset_b: address) -> bytes27:
    hash: bytes32 = keccak256(abi_encode(asset_a, asset_b))
    return convert(hash, bytes27)
