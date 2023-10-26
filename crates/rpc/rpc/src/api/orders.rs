use guard_types::rpc::{
    SignedComposableLimitOrder, SignedComposableSearcherOrder, SignedLimitOrder,
    SignedSearcherOrder
};
use jsonrpsee::{core::RpcResult, proc_macros::rpc};

#[cfg_attr(not(feature = "client"), rpc(server, namespace = "order"))]
#[cfg_attr(feature = "client", rpc(server, client, namespace = "order"))]
#[async_trait::async_trait]
pub trait OrderApi {
    #[method(name = "submit_limit_order")]
    async fn submit_limit_order(&self, order: SignedLimitOrder) -> RpcResult<bool>;

    #[method(name = "submit_searcher_order")]
    async fn submit_searcher_order(&self, order: SignedSearcherOrder) -> RpcResult<bool>;

    #[method(name = "submit_composable_limit_order")]
    async fn submit_composable_limit_order(
        &self,
        order: SignedComposableLimitOrder
    ) -> RpcResult<bool>;

    #[method(name = "submit_composable_searcher_order")]
    async fn submit_composable_searcher_order(
        &self,
        order: SignedComposableSearcherOrder
    ) -> RpcResult<bool>;
}
