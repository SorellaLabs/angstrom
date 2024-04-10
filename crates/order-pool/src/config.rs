use angstrom_types::primitive::PoolId;

/// Guarantees max orders per sender
pub const ORDER_POOL_MAX_ACCOUNT_SLOTS_PER_SENDER: usize = 16;

/// The default maximum allowed number of orders in the given subpool;
pub const LIMIT_SUBPOOL_MAX_ORDERS_DEFAULT: usize = 1_000;

/// The default maximum allowed size of the given subpool.
pub const LIMIT_SUBPOOL_MAX_SIZE_MB_DEFAULT: usize = 20;

/// The default maximum allowed number of orders in the searcher subpool.
pub const SEARCHER_SUBPOOL_MAX_ORDERS_DEFAULT: usize = 100;

/// The default maximum allowed size of the searcher subpool.
pub const SEARCHER_SUBPOOL_MAX_SIZE_MB_DEFAULT: usize = 5;

/// Configuration options for the Transaction pool.
#[derive(Debug, Clone)]
pub struct PoolConfig {
    /// pool ids
    pub ids:               Vec<PoolId>,
    /// Max number of transaction in the pending sub-pool
    pub lo_pending_limit:  LimitSubPoolLimit,
    /// Max number of transaction in the queued sub-pool
    pub lo_queued_limit:   LimitSubPoolLimit,
    /// Max number of transaction in the parked sub-pool
    pub lo_parked_limit:   LimitSubPoolLimit,
    /// Max number of transaction in the composable limit sub-pool
    pub cl_pending_limit:  LimitSubPoolLimit,
    /// Max number of transaction in the searcher & composable searcher sub-pool
    pub s_pending_limit:   SearcherSubPoolLimit,
    /// Max number of executable transaction slots guaranteed per account
    pub max_account_slots: usize
}

impl Default for PoolConfig {
    fn default() -> Self {
        Self {
            ids:               vec![],
            lo_pending_limit:  Default::default(),
            lo_queued_limit:   Default::default(),
            lo_parked_limit:   Default::default(),
            cl_pending_limit:  Default::default(),
            s_pending_limit:   Default::default(),
            max_account_slots: ORDER_POOL_MAX_ACCOUNT_SLOTS_PER_SENDER
        }
    }
}

/// Size limits for a limit order sub-pool.
#[derive(Debug, Clone)]
pub struct LimitSubPoolLimit {
    /// Maximum amount of orders in the pool.
    pub max_orders: usize,
    /// Maximum combined size (in bytes) of transactions in the pool.
    pub max_size:   usize
}

impl LimitSubPoolLimit {
    /// Returns whether the size or amount constraint is violated.
    #[inline]
    pub fn is_exceeded(&self, orders: usize, size: usize) -> bool {
        self.max_orders < orders || self.max_size < size
    }
}

impl Default for LimitSubPoolLimit {
    fn default() -> Self {
        // either 10k transactions or 20MB
        Self {
            max_orders: LIMIT_SUBPOOL_MAX_ORDERS_DEFAULT,
            max_size:   LIMIT_SUBPOOL_MAX_SIZE_MB_DEFAULT * 1024 * 1024
        }
    }
}

#[derive(Debug, Clone)]
pub struct SearcherSubPoolLimit {
    /// Maximum amount of searcher orders in the pool.
    pub max_orders: usize,
    /// Maximum combined size (in bytes) of transactions in the pool.
    pub max_size:   usize
}

impl SearcherSubPoolLimit {
    /// Returns whether the size or amount constraint is violated.
    #[inline]
    pub fn is_exceeded(&self, orders: usize, size: usize) -> bool {
        self.max_orders < orders || self.max_size < size
    }
}

impl Default for SearcherSubPoolLimit {
    fn default() -> Self {
        // either 10k transactions or 20MB
        Self {
            max_orders: SEARCHER_SUBPOOL_MAX_ORDERS_DEFAULT,
            max_size:   SEARCHER_SUBPOOL_MAX_SIZE_MB_DEFAULT * 1024 * 1024
        }
    }
}
