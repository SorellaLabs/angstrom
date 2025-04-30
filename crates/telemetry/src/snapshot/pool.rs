use angstrom_types::{
    orders::PoolSolution,
    primitive::PoolId,
    sol_bindings::{
        grouped_orders::{AllOrders, OrderWithStorageData},
        rpc_orders::TopOfBlockOrder
    },
    uni_structure::BaselinePoolState
};
use serde::{Deserialize, Serialize};

/// A PoolSnapshot represents the current node's view of a single pair.  It
/// contains the current AMM state of that pair, the ToB order valid for this
/// block (if any), and a list of all user orders that this node is aware of for
/// this block.  It also contains the most recent computed solution for this
/// pool as computed by this node.  Note that all data in this structure is
/// specific to what THIS node has seen and computed, any consensus information
/// that has come in from other nodes is snapshotted separately.
#[derive(Clone, Serialize, Deserialize)]
pub struct PoolSnapshot {
    /// ID of this pool
    id:          PoolId,
    /// Initial AMM state for this pool.  This state is the pre-TOB,
    /// pre-matching state
    amm:         Option<BaselinePoolState>,
    /// Current winning ToB order for this pool
    tob_order:   Option<OrderWithStorageData<TopOfBlockOrder>>,
    /// User orders that the current node has on record for this pool
    user_orders: Vec<OrderWithStorageData<AllOrders>>,
    /// Currently computed solution for this pool
    solution:    Option<PoolSolution>
}

// We might want to include solve_for_t0, but that's part of the matcher config
// and it's always false now (we always solve for T1)
