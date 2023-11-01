pub mod api;
pub mod constants;
pub mod cors;
mod errors;
pub mod id_provider;
pub mod impls;
pub mod metrics;
pub mod types;

use std::{
    collections::{HashMap, HashSet},
    fmt,
    net::{Ipv4Addr, SocketAddr, SocketAddrV4},
    str::FromStr,
    time::Duration
};

use errors::RpcError;
use guard_ipc::server::{Builder as IpcServerBuilder, Endpoint, IpcServer};
use jsonrpsee::{
    server::{IdProvider, Server, ServerBuilder, ServerHandle},
    Methods, RpcModule
};
use metrics::RpcServerMetrics;
use serde::{Deserialize, Serialize, Serializer};
use strum::{AsRefStr, EnumString, EnumVariantNames, ParseError, VariantNames};
use tower::{
    layer::util::{Identity, Stack},
    limit::RateLimitLayer
};
use tower_http::cors::CorsLayer;
use tracing::{instrument, trace};

use crate::{constants::*, errors::*, id_provider::EthSubscriptionIdProvider};

const REQUESTS_PER_SECOND: usize = 100;

pub async fn launch<Consensus, OrderPool, Tasks>(
    consensus: Consensus,
    order_pool: OrderPool,
    tasks: Tasks,
    module_config: impl Into<TransportRpcModuleConfig>,
    server_config: impl Into<RpcServerConfig>
) -> Result<RpcServerHandle, RpcError> {
    let module_config = module_config.into();
    let server_config = server_config.into();

    RpcModuleBuilder::new(consensus, order_pool, tasks)
        .build(module_config)
        .start_server(server_config)
        .await
}

/// A builder type to configure the RPC module: See [RpcModule]
///
/// This is the main entrypoint and the easiest way to configure an RPC server.
#[derive(Debug, Clone)]
pub struct RpcModuleBuilder<Consensus, OrderPool, Tasks> {
    /// The Provider type to when creating all rpc handlers
    consensus:  Consensus,
    /// The Pool type to when creating all rpc handlers
    order_pool: OrderPool,
    /// The Network type to when creating all rpc handlers
    /// How additional tasks are spawned, for example in the eth pubsub
    /// namespace
    executor:   Tasks
}

// === impl RpcBuilder ===

impl<Consensus, OrderPool, Tasks> RpcModuleBuilder<Consensus, OrderPool, Tasks> {
    /// Create a new instance of the builder
    pub fn new(consensus: Consensus, order_pool: OrderPool, executor: Tasks) -> Self {
        Self { order_pool, consensus, executor }
    }

    /// Configures all [RpcModule]s specific to the given
    /// [TransportRpcModuleConfig] which can be used to start the transport
    /// server(s).
    ///
    /// See also [RpcServer::start]
    pub fn build(self, module_config: TransportRpcModuleConfig) -> TransportRpcModules<()> {
        let mut modules = TransportRpcModules::default();

        let Self { consensus, order_pool, executor } = self;

        if !module_config.is_empty() {
            let TransportRpcModuleConfig { http, ws, ipc } = module_config.clone();

            let mut registry = GuardModuleRegistry::new(consensus, order_pool, executor);

            modules.config = module_config;
            modules.http = registry.maybe_module(http.as_ref());
            modules.ws = registry.maybe_module(ws.as_ref());
            modules.ipc = registry.maybe_module(ipc.as_ref());
        }

        modules
    }
}

/// Holds modules to be installed per transport type
///
/// # Example
///
/// Configure a http transport only
///
/// ```
/// use reth_rpc_builder::{GuardRpcModule, TransportRpcModuleConfig};
///  let config = TransportRpcModuleConfig::default()
///       .with_http([GuardRpcModule::Eth, GuardRpcModule::Admin]);
/// ```
#[derive(Debug, Clone, Default, Eq, PartialEq)]
pub struct TransportRpcModuleConfig {
    /// http module configuration
    http: Option<RpcModuleSelection>,
    /// ws module configuration
    ws:   Option<RpcModuleSelection>,
    /// ipc module configuration
    ipc:  Option<RpcModuleSelection>
}

// === impl TransportRpcModuleConfig ===

impl TransportRpcModuleConfig {
    /// Creates a new config with only http set
    pub fn set_http(http: impl Into<RpcModuleSelection>) -> Self {
        Self::default().with_http(http)
    }

    /// Creates a new config with only ws set
    pub fn set_ws(ws: impl Into<RpcModuleSelection>) -> Self {
        Self::default().with_ws(ws)
    }

    /// Creates a new config with only ipc set
    pub fn set_ipc(ipc: impl Into<RpcModuleSelection>) -> Self {
        Self::default().with_ipc(ipc)
    }

    /// Sets the [RpcModuleSelection] for the http transport.
    pub fn with_http(mut self, http: impl Into<RpcModuleSelection>) -> Self {
        self.http = Some(http.into());
        self
    }

    /// Sets the [RpcModuleSelection] for the ws transport.
    pub fn with_ws(mut self, ws: impl Into<RpcModuleSelection>) -> Self {
        self.ws = Some(ws.into());
        self
    }

    /// Sets the [RpcModuleSelection] for the http transport.
    pub fn with_ipc(mut self, ipc: impl Into<RpcModuleSelection>) -> Self {
        self.ipc = Some(ipc.into());
        self
    }

    /// Returns true if no transports are configured
    pub fn is_empty(&self) -> bool {
        self.http.is_none() && self.ws.is_none() && self.ipc.is_none()
    }

    /// Returns the [RpcModuleSelection] for the http transport
    pub fn http(&self) -> Option<&RpcModuleSelection> {
        self.http.as_ref()
    }

    /// Returns the [RpcModuleSelection] for the ws transport
    pub fn ws(&self) -> Option<&RpcModuleSelection> {
        self.ws.as_ref()
    }

    /// Returns the [RpcModuleSelection] for the ipc transport
    pub fn ipc(&self) -> Option<&RpcModuleSelection> {
        self.ipc.as_ref()
    }

    /// Ensures that both http and ws are configured and that they are
    /// configured to use the same port.
    fn ensure_ws_http_identical(&self) -> Result<(), WsHttpSamePortError> {
        if RpcModuleSelection::are_identical(self.http.as_ref(), self.ws.as_ref()) {
            Ok(())
        } else {
            let http_modules = self
                .http
                .clone()
                .map(RpcModuleSelection::into_selection)
                .unwrap_or_default();
            let ws_modules = self
                .ws
                .clone()
                .map(RpcModuleSelection::into_selection)
                .unwrap_or_default();
            Err(WsHttpSamePortError::ConflictingModules { http_modules, ws_modules })
        }
    }
}

/// Holds installed modules per transport type.
#[derive(Debug, Clone, Default)]
pub struct TransportRpcModules<Context = ()> {
    /// The original config
    config: TransportRpcModuleConfig,
    /// rpcs module for http
    http:   Option<RpcModule<Context>>,
    /// rpcs module for ws
    ws:     Option<RpcModule<Context>>,
    /// rpcs module for ipc
    ipc:    Option<RpcModule<Context>>
}

// === impl TransportRpcModules ===

impl TransportRpcModules {
    /// Returns the [TransportRpcModuleConfig] used to configure this instance.
    pub fn module_config(&self) -> &TransportRpcModuleConfig {
        &self.config
    }

    /// Merge the given Methods in the configured http methods.
    ///
    /// Fails if any of the methods in other is present already.
    ///
    /// Returns Ok(false) if no http transport is configured.
    pub fn merge_http(
        &mut self,
        other: impl Into<Methods>
    ) -> Result<bool, jsonrpsee::core::error::Error> {
        if let Some(ref mut http) = self.http {
            return http.merge(other.into()).map(|_| true)
        }
        Ok(false)
    }

    /// Merge the given Methods in the configured ws methods.
    ///
    /// Fails if any of the methods in other is present already.
    ///
    /// Returns Ok(false) if no http transport is configured.
    pub fn merge_ws(
        &mut self,
        other: impl Into<Methods>
    ) -> Result<bool, jsonrpsee::core::error::Error> {
        if let Some(ref mut ws) = self.ws {
            return ws.merge(other.into()).map(|_| true)
        }
        Ok(false)
    }

    /// Merge the given Methods in the configured ipc methods.
    ///
    /// Fails if any of the methods in other is present already.
    ///
    /// Returns Ok(false) if no ipc transport is configured.
    pub fn merge_ipc(
        &mut self,
        other: impl Into<Methods>
    ) -> Result<bool, jsonrpsee::core::error::Error> {
        if let Some(ref mut ipc) = self.ipc {
            return ipc.merge(other.into()).map(|_| true)
        }
        Ok(false)
    }

    /// Merge the given Methods in all configured methods.
    ///
    /// Fails if any of the methods in other is present already.
    pub fn merge_configured(
        &mut self,
        other: impl Into<Methods>
    ) -> Result<(), jsonrpsee::core::error::Error> {
        let other = other.into();
        self.merge_http(other.clone())?;
        self.merge_ws(other.clone())?;
        self.merge_ipc(other)?;
        Ok(())
    }

    /// Convenience function for starting a server
    pub async fn start_server(self, builder: RpcServerConfig) -> Result<RpcServerHandle, RpcError> {
        builder.start(self).await
    }
}

/// A builder type for configuring and launching the servers that will handle
/// RPC requests.
///
/// Supported server transports are:
///    - http
///    - ws
///    - ipc
///
/// Http and WS share the same settings: [`ServerBuilder`].
///
/// Once the [RpcModule] is built via [RpcModuleBuilder] the servers can be
/// started, See also [ServerBuilder::build] and
/// [Server::start](jsonrpsee::server::Server::start).
#[derive(Default)]
pub struct RpcServerConfig {
    /// Configs for JSON-RPC Http.
    http_server_config: Option<ServerBuilder>,
    /// Allowed CORS Domains for http
    http_cors_domains:  Option<String>,
    /// Address where to bind the http server to
    http_addr:          Option<SocketAddr>,
    /// Configs for WS server
    ws_server_config:   Option<ServerBuilder>,
    /// Allowed CORS Domains for ws.
    ws_cors_domains:    Option<String>,
    /// Address where to bind the ws server to
    ws_addr:            Option<SocketAddr>,
    /// Configs for JSON-RPC IPC server
    ipc_server_config:  Option<IpcServerBuilder>,
    /// The Endpoint where to launch the ipc server
    ipc_endpoint:       Option<Endpoint>
}

impl fmt::Debug for RpcServerConfig {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("RpcServerConfig")
            .field("http_server_config", &self.http_server_config)
            .field("http_cors_domains", &self.http_cors_domains)
            .field("http_addr", &self.http_addr)
            .field("ws_server_config", &self.ws_server_config)
            .field("ws_addr", &self.ws_addr)
            .field("ipc_server_config", &self.ipc_server_config)
            .field("ipc_endpoint", &self.ipc_endpoint.as_ref().map(|endpoint| endpoint.path()))
            .finish()
    }
}

/// === impl RpcServerConfig ===

impl RpcServerConfig {
    /// Creates a new config with only http set
    pub fn http(config: ServerBuilder) -> Self {
        Self::default().with_http(config)
    }

    /// Creates a new config with only ws set
    pub fn ws(config: ServerBuilder) -> Self {
        Self::default().with_ws(config)
    }

    /// Creates a new config with only ipc set
    pub fn ipc(config: IpcServerBuilder) -> Self {
        Self::default().with_ipc(config)
    }

    /// Configures the http server
    ///
    /// Note: this always configures an [EthSubscriptionIdProvider] [IdProvider]
    /// for convenience. To set a custom [IdProvider], please use
    /// [Self::with_id_provider].
    pub fn with_http(mut self, config: ServerBuilder) -> Self {
        self.http_server_config =
            Some(config.set_id_provider(EthSubscriptionIdProvider::default()));
        self
    }

    /// Configure the cors domains for http _and_ ws
    pub fn with_cors(self, cors_domain: Option<String>) -> Self {
        self.with_http_cors(cors_domain.clone())
            .with_ws_cors(cors_domain)
    }

    /// Configure the cors domains for HTTP
    pub fn with_http_cors(mut self, cors_domain: Option<String>) -> Self {
        self.http_cors_domains = cors_domain;
        self
    }

    /// Configure the cors domains for WS
    pub fn with_ws_cors(mut self, cors_domain: Option<String>) -> Self {
        self.ws_cors_domains = cors_domain;
        self
    }

    /// Configures the ws server
    ///
    /// Note: this always configures an [EthSubscriptionIdProvider] [IdProvider]
    /// for convenience. To set a custom [IdProvider], please use
    /// [Self::with_id_provider].
    pub fn with_ws(mut self, config: ServerBuilder) -> Self {
        self.ws_server_config = Some(config.set_id_provider(EthSubscriptionIdProvider::default()));
        self
    }

    /// Configures the [SocketAddr] of the http server
    ///
    /// Default is [Ipv4Addr::LOCALHOST] and [DEFAULT_HTTP_RPC_PORT]
    pub fn with_http_address(mut self, addr: SocketAddr) -> Self {
        self.http_addr = Some(addr);
        self
    }

    /// Configures the [SocketAddr] of the ws server
    ///
    /// Default is [Ipv4Addr::LOCALHOST] and [DEFAULT_WS_RPC_PORT]
    pub fn with_ws_address(mut self, addr: SocketAddr) -> Self {
        self.ws_addr = Some(addr);
        self
    }

    /// Configures the ipc server
    ///
    /// Note: this always configures an [EthSubscriptionIdProvider] [IdProvider]
    /// for convenience. To set a custom [IdProvider], please use
    /// [Self::with_id_provider].
    pub fn with_ipc(mut self, config: IpcServerBuilder) -> Self {
        self.ipc_server_config = Some(config.set_id_provider(EthSubscriptionIdProvider::default()));
        self
    }

    /// Sets a custom [IdProvider] for all configured transports.
    ///
    /// By default all transports use [EthSubscriptionIdProvider]
    pub fn with_id_provider<I>(mut self, id_provider: I) -> Self
    where
        I: IdProvider + Clone + 'static
    {
        if let Some(http) = self.http_server_config {
            self.http_server_config = Some(http.set_id_provider(id_provider.clone()));
        }
        if let Some(ws) = self.ws_server_config {
            self.ws_server_config = Some(ws.set_id_provider(id_provider.clone()));
        }
        if let Some(ipc) = self.ipc_server_config {
            self.ipc_server_config = Some(ipc.set_id_provider(id_provider));
        }

        self
    }

    /// Configures the endpoint of the ipc server
    ///
    /// Default is [DEFAULT_IPC_ENDPOINT]
    pub fn with_ipc_endpoint(mut self, path: impl Into<String>) -> Self {
        self.ipc_endpoint = Some(Endpoint::new(path.into()));
        self
    }

    /// Returns true if any server is configured.
    ///
    /// If no server is configured, no server will be be launched on
    /// [RpcServerConfig::start].
    pub fn has_server(&self) -> bool {
        self.http_server_config.is_some()
            || self.ws_server_config.is_some()
            || self.ipc_server_config.is_some()
    }

    /// Returns the [SocketAddr] of the http server
    pub fn http_address(&self) -> Option<SocketAddr> {
        self.http_addr
    }

    /// Returns the [SocketAddr] of the ws server
    pub fn ws_address(&self) -> Option<SocketAddr> {
        self.ws_addr
    }

    /// Returns the [Endpoint] of the ipc server
    pub fn ipc_endpoint(&self) -> Option<&Endpoint> {
        self.ipc_endpoint.as_ref()
    }

    /// Convenience function to do [RpcServerConfig::build] and
    /// [RpcServer::start] in one step
    pub async fn start(self, modules: TransportRpcModules) -> Result<RpcServerHandle, RpcError> {
        self.build().await?.start(modules).await
    }

    /// Builds the ws and http server(s).
    ///
    /// If both are on the same port, they are combined into one server.
    async fn build_ws_http(&mut self) -> Result<WsHttpServer, RpcError> {
        let http_socket_addr = self.http_addr.unwrap_or(SocketAddr::V4(SocketAddrV4::new(
            Ipv4Addr::LOCALHOST,
            DEFAULT_HTTP_RPC_PORT
        )));

        let ws_socket_addr = self
            .ws_addr
            .unwrap_or(SocketAddr::V4(SocketAddrV4::new(Ipv4Addr::LOCALHOST, DEFAULT_WS_RPC_PORT)));
        let metrics = RpcServerMetrics::default();
        // If both are configured on the same port, we combine them into one server.
        if self.http_addr == self.ws_addr
            && self.http_server_config.is_some()
            && self.ws_server_config.is_some()
        {
            let cors = match (self.ws_cors_domains.as_ref(), self.http_cors_domains.as_ref()) {
                (Some(ws_cors), Some(http_cors)) => {
                    if ws_cors.trim() != http_cors.trim() {
                        return Err(WsHttpSamePortError::ConflictingCorsDomains {
                            http_cors_domains: Some(http_cors.clone()),
                            ws_cors_domains:   Some(ws_cors.clone())
                        }
                        .into())
                    }
                    Some(ws_cors)
                }
                (None, cors @ Some(_)) => cors,
                (cors @ Some(_), None) => cors,
                _ => None
            }
            .cloned();

            // we merge this into one server using the http setup
            self.ws_server_config.take();

            let builder = self.http_server_config.take().expect("is set; qed");
            let (server, addr) = WsHttpServerKind::build(
                builder,
                http_socket_addr,
                cors,
                ServerKind::WsHttp(http_socket_addr),
                metrics.clone()
            )
            .await?;
            return Ok(WsHttpServer {
                http_local_addr: Some(addr),
                ws_local_addr:   Some(addr),
                server:          WsHttpServers::SamePort(server)
            })
        }

        let mut http_local_addr = None;
        let mut http_server = None;

        let mut ws_local_addr = None;
        let mut ws_server = None;
        if let Some(builder) = self.ws_server_config.take() {
            let builder = builder.ws_only();
            let (server, addr) = WsHttpServerKind::build(
                builder,
                ws_socket_addr,
                self.ws_cors_domains.take(),
                ServerKind::WS(ws_socket_addr),
                metrics.clone()
            )
            .await?;
            ws_local_addr = Some(addr);
            ws_server = Some(server);
        }

        if let Some(builder) = self.http_server_config.take() {
            let builder = builder.http_only();
            let (server, addr) = WsHttpServerKind::build(
                builder,
                http_socket_addr,
                self.http_cors_domains.take(),
                ServerKind::Http(http_socket_addr),
                metrics.clone()
            )
            .await?;
            http_local_addr = Some(addr);
            http_server = Some(server);
        }

        Ok(WsHttpServer {
            http_local_addr,
            ws_local_addr,
            server: WsHttpServers::DifferentPort { http: http_server, ws: ws_server }
        })
    }

    /// Finalize the configuration of the server(s).
    ///
    /// This consumes the builder and returns a server.
    ///
    /// Note: The server ist not started and does nothing unless polled, See
    /// also [RpcServer::start]
    pub async fn build(mut self) -> Result<RpcServer, RpcError> {
        let mut server = RpcServer::empty();
        server.ws_http = self.build_ws_http().await?;

        if let Some(builder) = self.ipc_server_config {
            let ipc_path = self
                .ipc_endpoint
                .unwrap_or_else(|| Endpoint::new(DEFAULT_IPC_ENDPOINT.to_string()));
            let ipc = builder.build(ipc_path.path())?;
            server.ipc = Some(ipc);
        }

        Ok(server)
    }
}

/// Container type for ws and http servers in all possible combinations.
#[derive(Default)]
struct WsHttpServer {
    /// The address of the http server
    http_local_addr: Option<SocketAddr>,
    /// The address of the ws server
    ws_local_addr:   Option<SocketAddr>,
    /// Configured ws,http servers
    server:          WsHttpServers
}

/// Enum for holding the http and ws servers in all possible combinations.
enum WsHttpServers {
    /// Both servers are on the same port
    SamePort(WsHttpServerKind),
    /// Servers are on different ports
    DifferentPort { http: Option<WsHttpServerKind>, ws: Option<WsHttpServerKind> }
}

// === impl WsHttpServers ===

impl WsHttpServers {
    /// Starts the servers and returns the handles (http, ws)
    async fn start(
        self,
        http_module: Option<RpcModule<()>>,
        ws_module: Option<RpcModule<()>>,
        config: &TransportRpcModuleConfig
    ) -> Result<(Option<ServerHandle>, Option<ServerHandle>), RpcError> {
        let mut http_handle = None;
        let mut ws_handle = None;
        match self {
            WsHttpServers::SamePort(both) => {
                // Make sure http and ws modules are identical, since we currently can't run
                // different modules on same server
                config.ensure_ws_http_identical()?;

                if let Some(module) = http_module.or(ws_module) {
                    let handle = both.start(module).await;
                    http_handle = Some(handle.clone());
                    ws_handle = Some(handle);
                }
            }
            WsHttpServers::DifferentPort { http, ws } => {
                if let Some((server, module)) =
                    http.and_then(|server| http_module.map(|module| (server, module)))
                {
                    http_handle = Some(server.start(module).await);
                }
                if let Some((server, module)) =
                    ws.and_then(|server| ws_module.map(|module| (server, module)))
                {
                    ws_handle = Some(server.start(module).await);
                }
            }
        }

        Ok((http_handle, ws_handle))
    }
}

impl Default for WsHttpServers {
    fn default() -> Self {
        Self::DifferentPort { http: None, ws: None }
    }
}

/// Http Servers Enum
enum WsHttpServerKind {
    /// Http server
    Plain(Server<Stack<RateLimitLayer, Identity>, RpcServerMetrics>),
    /// Http server with cors
    WithCors(Server<Stack<RateLimitLayer, Stack<CorsLayer, Identity>>, RpcServerMetrics>)
}

// === impl WsHttpServerKind ===

impl WsHttpServerKind {
    /// Starts the server and returns the handle
    async fn start(self, module: RpcModule<()>) -> ServerHandle {
        match self {
            WsHttpServerKind::Plain(server) => server.start(module),
            WsHttpServerKind::WithCors(server) => server.start(module)
        }
    }

    /// Builds
    async fn build(
        builder: ServerBuilder,
        socket_addr: SocketAddr,
        cors_domains: Option<String>,
        server_kind: ServerKind,
        metrics: RpcServerMetrics
    ) -> Result<(Self, SocketAddr), RpcError> {
        let rate_limiting = RateLimitLayer::new(REQUESTS_PER_SECOND, Duration::from_secs(1));

        if let Some(cors) = cors_domains.as_deref().map(cors::create_cors_layer) {
            let cors = cors.map_err(|err| RpcError::Custom(err.to_string()))?;
            let middleware = tower::ServiceBuilder::new()
                .layer(cors)
                .layer(rate_limiting);
            let server = builder
                .set_middleware(middleware)
                .set_logger(metrics)
                .build(socket_addr)
                .await
                .map_err(|err| RpcError::from_jsonrpsee_error(err, server_kind))?;
            let local_addr = server.local_addr()?;
            let server = WsHttpServerKind::WithCors(server);
            Ok((server, local_addr))
        } else {
            let middleware = tower::ServiceBuilder::new().layer(rate_limiting);

            let server = builder
                .set_middleware(middleware)
                .set_logger(metrics)
                .build(socket_addr)
                .await
                .map_err(|err| RpcError::from_jsonrpsee_error(err, server_kind))?;
            let local_addr = server.local_addr()?;
            let server = WsHttpServerKind::Plain(server);
            Ok((server, local_addr))
        }
    }
}

/// Container type for each transport ie. http, ws, and ipc server
pub struct RpcServer {
    /// Configured ws,http servers
    ws_http: WsHttpServer,
    /// ipc server
    ipc:     Option<IpcServer>
}

// === impl RpcServer ===

impl RpcServer {
    fn empty() -> RpcServer {
        RpcServer { ws_http: Default::default(), ipc: None }
    }

    /// Returns the [`SocketAddr`] of the http server if started.
    pub fn http_local_addr(&self) -> Option<SocketAddr> {
        self.ws_http.http_local_addr
    }

    /// Returns the [`SocketAddr`] of the ws server if started.
    pub fn ws_local_addr(&self) -> Option<SocketAddr> {
        self.ws_http.ws_local_addr
    }

    /// Returns the [`Endpoint`] of the ipc server if started.
    pub fn ipc_endpoint(&self) -> Option<&Endpoint> {
        self.ipc.as_ref().map(|ipc| ipc.endpoint())
    }

    /// Starts the configured server by spawning the servers on the tokio
    /// runtime.
    ///
    /// This returns an [RpcServerHandle] that's connected to the server task(s)
    /// until the server is stopped or the [RpcServerHandle] is dropped.
    #[instrument(name = "start", skip_all, fields(http = ?self.http_local_addr(), ws = ?self.ws_local_addr(), ipc = ?self.ipc_endpoint().map(|ipc|ipc.path())), target = "rpc", level = "TRACE")]
    pub async fn start(self, modules: TransportRpcModules) -> Result<RpcServerHandle, RpcError> {
        trace!(target: "rpc", "staring RPC server");
        let Self { ws_http, ipc: ipc_server } = self;
        let TransportRpcModules { config, http, ws, ipc } = modules;
        let mut handle = RpcServerHandle {
            http_local_addr: ws_http.http_local_addr,
            ws_local_addr:   ws_http.ws_local_addr,
            http:            None,
            ws:              None,
            ipc_endpoint:    None,
            ipc:             None
        };

        let (http, ws) = ws_http.server.start(http, ws, &config).await?;
        handle.http = http;
        handle.ws = ws;

        if let Some((server, module)) =
            ipc_server.and_then(|server| ipc.map(|module| (server, module)))
        {
            handle.ipc_endpoint = Some(server.endpoint().path().to_string());
            handle.ipc = Some(server.start(module).await?);
        }

        Ok(handle)
    }
}

impl fmt::Debug for RpcServer {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("RpcServer")
            .field("http", &self.ws_http.http_local_addr.is_some())
            .field("ws", &self.ws_http.http_local_addr.is_some())
            .field("ipc", &self.ipc.is_some())
            .finish()
    }
}

/// A handle to the spawned servers.
///
/// When this type is dropped or [RpcServerHandle::stop] has been called the
/// server will be stopped.
#[derive(Clone)]
#[must_use = "Server stops if dropped"]
pub struct RpcServerHandle {
    /// The address of the http/ws server
    http_local_addr: Option<SocketAddr>,
    ws_local_addr:   Option<SocketAddr>,
    http:            Option<ServerHandle>,
    ws:              Option<ServerHandle>,
    ipc_endpoint:    Option<String>,
    ipc:             Option<ServerHandle>
}

// === impl RpcServerHandle ===

impl RpcServerHandle {
    /// Returns the [`SocketAddr`] of the http server if started.
    pub fn http_local_addr(&self) -> Option<SocketAddr> {
        self.http_local_addr
    }

    /// Returns the [`SocketAddr`] of the ws server if started.
    pub fn ws_local_addr(&self) -> Option<SocketAddr> {
        self.ws_local_addr
    }

    /// Tell the server to stop without waiting for the server to stop.
    pub fn stop(self) -> Result<(), RpcError> {
        if let Some(handle) = self.http {
            handle.stop()?
        }

        if let Some(handle) = self.ws {
            handle.stop()?
        }

        if let Some(handle) = self.ipc {
            handle.stop()?
        }

        Ok(())
    }

    /// Returns the endpoint of the launched IPC server, if any
    pub fn ipc_endpoint(&self) -> Option<String> {
        self.ipc_endpoint.clone()
    }

    /// Returns the url to the http server
    pub fn http_url(&self) -> Option<String> {
        self.http_local_addr.map(|addr| format!("http://{addr}"))
    }

    /// Returns the url to the ws server
    pub fn ws_url(&self) -> Option<String> {
        self.ws_local_addr.map(|addr| format!("ws://{addr}"))
    }

    /// Returns a http client connected to the server.
    pub fn http_client(&self) -> Option<jsonrpsee::http_client::HttpClient> {
        let url = self.http_url()?;
        let client = jsonrpsee::http_client::HttpClientBuilder::default()
            .build(url)
            .expect("Failed to create http client");
        Some(client)
    }

    /// Returns a ws client connected to the server.
    pub async fn ws_client(&self) -> Option<jsonrpsee::ws_client::WsClient> {
        let url = self.ws_url()?;
        let client = jsonrpsee::ws_client::WsClientBuilder::default()
            .build(url)
            .await
            .expect("Failed to create ws client");
        Some(client)
    }
}

impl fmt::Debug for RpcServerHandle {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("RpcServerHandle")
            .field("http", &self.http.is_some())
            .field("ws", &self.ws.is_some())
            .field("ipc", &self.ipc.is_some())
            .finish()
    }
}

/// Describes the modules that should be installed.
///
/// # Example
///
/// Create a [RpcModuleSelection] from a selection.
///
/// ```
/// use reth_rpc_builder::{GuardRpcModule, RpcModuleSelection};
/// let config: RpcModuleSelection = vec![GuardRpcModule::Eth].into();
/// ```
#[derive(Debug, Default, Clone, Eq, PartialEq)]
pub enum RpcModuleSelection {
    /// Use _all_ available modules.
    All,
    /// The default modules `eth`, `net`, `web3`
    #[default]
    Standard,
    /// Only use the configured modules.
    Selection(Vec<GuardRpcModule>)
}

impl RpcModuleSelection {
    pub const STANDARD_MODULES: [GuardRpcModule; 3] =
        [GuardRpcModule::Consensus, GuardRpcModule::Order, GuardRpcModule::Quoting];

    /// Returns a selection of [GuardRpcModule] with all
    /// [GuardRpcModule::VARIANTS].
    pub fn all_modules() -> Vec<GuardRpcModule> {
        RpcModuleSelection::try_from_selection(GuardRpcModule::VARIANTS.iter().copied())
            .expect("valid selection")
            .into_selection()
    }

    /// Returns the [RpcModuleSelection::STANDARD_MODULES] as a selection.
    pub fn standard_modules() -> Vec<GuardRpcModule> {
        RpcModuleSelection::try_from_selection(RpcModuleSelection::STANDARD_MODULES.iter().copied())
            .expect("valid selection")
            .into_selection()
    }

    /// All modules that are available by default on IPC.
    ///
    /// By default all modules are available on IPC.
    pub fn default_ipc_modules() -> Vec<GuardRpcModule> {
        Self::all_modules()
    }

    /// Creates a new _unique_ [RpcModuleSelection::Selection] from the given
    /// items.
    ///
    /// # Note
    ///
    /// This will dedupe the selection and remove duplicates while preserving
    /// the order.
    ///
    /// # Example
    ///
    /// Create a selection from the [GuardRpcModule] string identifiers
    ///
    /// ```
    ///  use reth_rpc_builder::{GuardRpcModule, RpcModuleSelection};
    /// let selection = vec!["eth", "admin"];
    /// let config = RpcModuleSelection::try_from_selection(selection).unwrap();
    /// assert_eq!(config, RpcModuleSelection::Selection(vec![GuardRpcModule::Eth, GuardRpcModule::Admin]));
    /// ```
    ///
    /// Create a unique selection from the [GuardRpcModule] string identifiers
    ///
    /// ```
    ///  use reth_rpc_builder::{GuardRpcModule, RpcModuleSelection};
    /// let selection = vec!["eth", "admin", "eth", "admin"];
    /// let config = RpcModuleSelection::try_from_selection(selection).unwrap();
    /// assert_eq!(config, RpcModuleSelection::Selection(vec![GuardRpcModule::Eth, GuardRpcModule::Admin]));
    /// ```
    pub fn try_from_selection<I, T>(selection: I) -> Result<Self, T::Error>
    where
        I: IntoIterator<Item = T>,
        T: TryInto<GuardRpcModule>
    {
        let mut unique = HashSet::new();

        let mut s = Vec::new();
        for item in selection.into_iter() {
            let item = item.try_into()?;
            if unique.insert(item) {
                s.push(item);
            }
        }
        Ok(RpcModuleSelection::Selection(s))
    }

    /// Returns true if no selection is configured
    pub fn is_empty(&self) -> bool {
        match self {
            RpcModuleSelection::Selection(sel) => sel.is_empty(),
            _ => false
        }
    }

    /// Returns an iterator over all configured [GuardRpcModule]
    pub fn iter_selection(&self) -> Box<dyn Iterator<Item = GuardRpcModule> + '_> {
        match self {
            RpcModuleSelection::All => Box::new(Self::all_modules().into_iter()),
            RpcModuleSelection::Standard => Box::new(Self::STANDARD_MODULES.iter().copied()),
            RpcModuleSelection::Selection(s) => Box::new(s.iter().copied())
        }
    }

    /// Returns the list of configured [GuardRpcModule]
    pub fn into_selection(self) -> Vec<GuardRpcModule> {
        match self {
            RpcModuleSelection::All => Self::all_modules(),
            RpcModuleSelection::Selection(s) => s,
            RpcModuleSelection::Standard => Self::STANDARD_MODULES.to_vec()
        }
    }

    /// Returns true if both selections are identical.
    fn are_identical(http: Option<&RpcModuleSelection>, ws: Option<&RpcModuleSelection>) -> bool {
        match (http, ws) {
            (Some(http), Some(ws)) => {
                let http = http.clone().iter_selection().collect::<HashSet<_>>();
                let ws = ws.clone().iter_selection().collect::<HashSet<_>>();

                http == ws
            }
            (Some(http), None) => http.is_empty(),
            (None, Some(ws)) => ws.is_empty(),
            _ => true
        }
    }
}

impl<I, T> From<I> for RpcModuleSelection
where
    I: IntoIterator<Item = T>,
    T: Into<GuardRpcModule>
{
    fn from(value: I) -> Self {
        RpcModuleSelection::Selection(value.into_iter().map(Into::into).collect())
    }
}

impl FromStr for RpcModuleSelection {
    type Err = ParseError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        let mut modules = s.split(',').map(str::trim).peekable();
        let first = modules.peek().copied().ok_or(ParseError::VariantNotFound)?;
        match first {
            "all" | "All" => Ok(RpcModuleSelection::All),
            _ => RpcModuleSelection::try_from_selection(modules)
        }
    }
}

impl fmt::Display for RpcModuleSelection {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "[{}]",
            self.iter_selection()
                .map(|s| s.to_string())
                .collect::<Vec<_>>()
                .join(", ")
        )
    }
}

/// Represents RPC modules that are supported by the guard
#[derive(
    Debug, Clone, Copy, Eq, PartialEq, Hash, AsRefStr, EnumVariantNames, EnumString, Deserialize,
)]
#[serde(rename_all = "snake_case")]
#[strum(serialize_all = "kebab-case")]
pub enum GuardRpcModule {
    Consensus,
    Order,
    Quoting
}

impl GuardRpcModule {
    /// Returns all variants of the enum
    pub const fn all_variants() -> &'static [&'static str] {
        Self::VARIANTS
    }
}

impl fmt::Display for GuardRpcModule {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.pad(self.as_ref())
    }
}

impl Serialize for GuardRpcModule {
    fn serialize<S>(&self, s: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer
    {
        s.serialize_str(self.as_ref())
    }
}

/// A Helper type the holds instances of the configured modules.
#[derive(Debug)]
pub struct GuardModuleRegistry<Consensus, OrderPool, Tasks> {
    consensus:  Consensus,
    order_pool: OrderPool,
    tasks:      Tasks,
    // eth:        Option<EthHandlers<Provider, Pool, Network, Events>>,
    /// Contains the [Methods] of a module
    modules:    HashMap<GuardRpcModule, Methods>
}

// === impl GuardModuleRegistry ===

impl<Consensus, OrderPool, Tasks> GuardModuleRegistry<Consensus, OrderPool, Tasks> {
    /// Creates a new, empty instance.
    pub fn new(consensus: Consensus, order_pool: OrderPool, tasks: Tasks) -> Self {
        Self { consensus, tasks, order_pool, modules: Default::default() }
    }

    /// Returns all installed methods
    pub fn methods(&self) -> Vec<Methods> {
        self.modules.values().cloned().collect()
    }

    /// Returns a merged RpcModule
    pub fn module(&self) -> RpcModule<()> {
        let mut module = RpcModule::new(());
        for methods in self.modules.values().cloned() {
            module.merge(methods).expect("No conflicts");
        }
        module
    }
}

impl<Consensus, OrderPool, Tasks> GuardModuleRegistry<Consensus, OrderPool, Tasks> {
    /// Helper function to create a [RpcModule] if it's not `None`
    fn maybe_module(&mut self, config: Option<&RpcModuleSelection>) -> Option<RpcModule<()>> {
        let config = config?;
        let module = self.module_for(config);
        Some(module)
    }

    /// Populates a new [RpcModule] based on the selected [GuardRpcModule]s in
    /// the given [RpcModuleSelection]
    pub fn module_for(&mut self, config: &RpcModuleSelection) -> RpcModule<()> {
        let mut module = RpcModule::new(());
        let all_methods = self.reth_methods(config.iter_selection());
        for methods in all_methods {
            module.merge(methods).expect("No conflicts");
        }
        module
    }

    /// Returns the [Methods] for the given [GuardRpcModule]
    ///
    /// If this is the first time the namespace is requested, a new instance of
    /// API implementation will be created.
    pub fn reth_methods(
        &mut self,
        namespaces: impl Iterator<Item = GuardRpcModule>
    ) -> Vec<Methods> {
        // Create a copy, so we can list out all the methods for rpc_ api
        let namespaces: Vec<_> = namespaces.collect();
        namespaces
            .iter()
            .copied()
            .map(|namespace| {
                self.modules
                    .entry(namespace)
                    .or_insert_with(|| match namespace {
                        GuardRpcModule::Order => {
                            todo!()
                        }
                        GuardRpcModule::Quoting => {
                            todo!()
                        }
                        GuardRpcModule::Consensus => {
                            todo!()
                        }
                    })
                    .clone()
            })
            .collect::<Vec<_>>()
    }
}
