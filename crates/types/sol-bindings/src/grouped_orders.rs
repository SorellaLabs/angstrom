use serde::{Deserialize, Serialize};

use crate::rpc_orders::*;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Hash)]
pub enum AllOrders {
    ExactStanding(ExactStandingOrder),
    PartialStanding(PartialStandingOrder),
    ExactFlash(ExactFlashOrder),
    PartialFlash(PartialFlashOrder),
    TOB(TopOfBlockOrder)
}

impl From<TopOfBlockOrder> for AllOrders {
    fn from(value: TopOfBlockOrder) -> Self {
        Self::TOB(value)
    }
}
