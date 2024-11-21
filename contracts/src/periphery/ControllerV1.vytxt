# SPDX-License-Identifier: MIT
# pragma version 0.4.0

from ethereum.ercs import IERC20
from snekmate.auth import ownable
from snekmate.auth import ownable_2step

initializes: ownable
initializes: ownable_2step[ownable := ownable]

exports: (
    ownable_2step.transfer_ownership,
    ownable_2step.accept_ownership,
    ownable.owner,
)

interface IAngstromAuth:
    def setController(new_controller: address): nonpayable
    def removePool(expected_store: address, store_index: uint256): nonpayable
    def configurePool(asset_a: address, asset_b: address, tick_spacing: uint16, fee_in_e6: uint24): nonpayable
    def toggleNodes(nodes: DynArray[address, 1]): nonpayable
    def extsload(key: bytes32) -> bytes32: view
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
    asset0: address
    asset1: address

ANGSTROM: public(immutable(IAngstromAuth))
SCHEDULE_TO_CONFIRM_DELAY: public(constant(uint256)) = 2 * 7 * 24 * 60 * 60
CONTROLLER_NOT_PENDING: constant(address) = empty(address)
MAX_NODES: constant(uint256) = 100

MAX_POOLS: constant(uint256) = 767
STORE_KEY_SHIFT: constant(uint256) = 40
STORE_ANG_SLOT: constant(uint256) = 2
STORE_OFFSET: constant(uint256) = 64


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
    ownable_2step.__init__()
    ownable._transfer_ownership(initial_owner)

@external
def schedule_new_controller(new_controller: address):
    ownable._check_owner()

    assert new_controller != CONTROLLER_NOT_PENDING, "Can\'t be sentinel value"

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
    self.pools[key] = Pool(asset0=asset_a, asset1=asset_b)

    log PoolConfigured(asset_a, asset_b, tick_spacing, fee_in_e6)

    extcall ANGSTROM.configurePool(asset_a, asset_b, tick_spacing, fee_in_e6)


@external
def remove_pool(
    asset_a: address,
    asset_b: address
):
    ownable._check_owner()

    store: address = self._get_store()
    key: bytes27 = self._compute_partial_key(asset_a, asset_b)
    entry_index: uint256 = MAX_POOLS
    for i: uint256 in range(0, MAX_POOLS):
        if self._get_key_at_index(store, i) == key:
            entry_index = i
            break

    self._check_sorted(asset_a, asset_b)
    log PoolRemoved(asset_a, asset_b)

    extcall ANGSTROM.removePool(store, entry_index)

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

@pure
def _compute_partial_key(asset_a: address, asset_b: address) -> bytes27:
    self._check_sorted(asset_a, asset_b)
    hash: bytes32 = keccak256(abi_encode(asset_a, asset_b))
    cleared: bytes32 = convert(convert(hash, uint256) << STORE_KEY_SHIFT, bytes32)
    return convert(cleared, bytes27)

@pure
def _check_sorted(asset_a: address, asset_b: address):
    assert convert(asset_a, uint256) < convert(asset_b, uint256), "Assets not sorted or unique"

@view
def _get_store() -> address:
    slot: bytes32 = convert(STORE_ANG_SLOT, bytes32)
    full_slot_value: uint256 = convert(staticcall ANGSTROM.extsload(slot), uint256)
    return convert(full_slot_value >> STORE_OFFSET, address)

@view
def _get_key_at_index(store: address, index: uint256) -> bytes27:
    raw_key: Bytes[27] = slice(store.code, 32 * index + 1, 27)
    return convert(raw_key, bytes27)
