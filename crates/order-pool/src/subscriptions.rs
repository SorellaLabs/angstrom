use tokio::sync::mpsc;

use crate::{common::Order, PoolOrder};
/// Holds all subscription channels for OrderPool data
pub struct OrderPoolSubscriptions<L, CL, S, CS>
where
    L: PoolOrder,
    CL: PoolOrder,
    S: PoolOrder,
    CS: PoolOrder
{
    new_orders:       Vec<mpsc::Sender<Order<L, CL, S, CS>>>,
    expired_orders:   Vec<mpsc::Sender<Vec<Order<L, CL, S, CS>>>>,
    filled_orders:    Vec<mpsc::Sender<Vec<Order<L, CL, S, CS>>>>,
    finalized_orders: Vec<mpsc::Sender<Vec<Order<L, CL, S, CS>>>>
}

impl<L, CL, S, CS> OrderPoolSubscriptions<L, CL, S, CS>
where
    L: PoolOrder,
    CL: PoolOrder,
    S: PoolOrder,
    CS: PoolOrder
{
    pub const fn new() -> Self {
        Self {
            new_orders:       Vec::new(),
            filled_orders:    Vec::new(),
            expired_orders:   Vec::new(),
            finalized_orders: Vec::new()
        }
    }

    pub fn new_order(&mut self, order: Order<L, CL, S, CS>) {
        self.new_orders
            .retain(|tx| tx.try_send(order.clone()).is_ok())
    }

    pub fn finalized_orders(&mut self, orders: Vec<Order<L, CL, S, CS>>) {
        self.finalized_orders
            .retain(|tx| tx.try_send(orders.clone()).is_ok())
    }

    pub fn filled_orders(&mut self, orders: Vec<Order<L, CL, S, CS>>) {
        self.filled_orders
            .retain(|tx| tx.try_send(orders.clone()).is_ok())
    }

    pub fn expired_orders(&mut self, orders: Vec<Order<L, CL, S, CS>>) {
        self.expired_orders
            .retain(|tx| tx.try_send(orders.clone()).is_ok())
    }

    pub fn subscribe_new_orders(&mut self, tx: mpsc::Sender<Order<L, CL, S, CS>>) {
        self.new_orders.push(tx)
    }

    pub fn subscribe_finalized_orders(&mut self, tx: mpsc::Sender<Vec<Order<L, CL, S, CS>>>) {
        self.finalized_orders.push(tx)
    }

    pub fn subscribe_filled_orders(&mut self, tx: mpsc::Sender<Vec<Order<L, CL, S, CS>>>) {
        self.filled_orders.push(tx)
    }

    pub fn subscribe_expired_orders(&mut self, tx: mpsc::Sender<Vec<Order<L, CL, S, CS>>>) {
        self.expired_orders.push(tx)
    }
}
