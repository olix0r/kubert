//! A controller runtime

#[cfg(feature = "server")]
use crate::server::{self, ServerArgs};
use crate::{
    admin::{self, Readiness},
    client::{self, Client, ClientArgs},
    errors,
    initialized::{self, Initialized},
    shutdown, LogFilter, LogFormat, LogInitError,
};
use futures_core::Stream;
use kube_core::{NamespaceResourceScope, Resource};
use kube_runtime::{reflector, watcher};
use serde::de::DeserializeOwned;
use std::{fmt::Debug, hash::Hash, time::Duration};
#[cfg(feature = "server")]
use tower_service::Service;

pub use kube_client::Api;
pub use reflector::Store;

/// Configures a controller [`Runtime`]
#[derive(Debug, Default)]
#[cfg_attr(docsrs, doc(cfg(feature = "runtime")))]
#[must_use]
pub struct Builder<S = NoServer> {
    admin: admin::Builder,
    client: Option<ClientArgs>,
    error_delay: Option<Duration>,
    log: Option<LogSettings>,

    #[cfg(feature = "server")]
    server: S,
    #[cfg(not(feature = "server"))]
    server: std::marker::PhantomData<S>,
}

/// Provides infrastructure for running:
///
/// * a Kubernetes controller including logging
/// * a default Kubernetes client
/// * signal handling and graceful shutdown
/// * an admin server with readiness and liveness probe endpoints
///
/// The runtime facilitates creating watches (with and without caches) that include error handling
/// and graceful shutdown.
#[cfg_attr(docsrs, doc(cfg(feature = "runtime")))]
#[must_use]
pub struct Runtime<S = NoServer> {
    admin: admin::Bound,
    client: Client,
    error_delay: Duration,
    initialized: Initialized,
    shutdown_rx: drain::Watch,
    shutdown: shutdown::Shutdown,

    #[cfg(feature = "server")]
    server: S,
    #[cfg(not(feature = "server"))]
    server: std::marker::PhantomData<S>,
}

/// Indicates that no HTTPS server is configured
#[derive(Debug, Default)]
pub struct NoServer(());

/// Indicates that the [`Builder`] could not configure a [`Runtime`]
#[derive(Debug, thiserror::Error)]
#[cfg_attr(docsrs, doc(cfg(feature = "runtime")))]
pub enum BuildError {
    /// Indicates that logging could not be initialized
    #[error(transparent)]
    LogInit(#[from] LogInitError),

    /// Indicates that the admin server could not be bound
    #[error(transparent)]
    Admin(#[from] admin::Error),

    /// Indicates that the Kubernetes client could not be iniialized.
    #[error(transparent)]
    Client(#[from] client::ConfigError),

    #[cfg(feature = "server")]
    /// Indicates that the HTTPS server could not be initialized
    #[error(transparent)]
    Server(#[from] server::Error),

    /// Indicates that a signal handler could not be registered
    #[error(transparent)]
    Signal(#[from] shutdown::RegisterError),
}

#[derive(Debug)]
struct LogSettings {
    filter: LogFilter,
    format: LogFormat,
}

// === impl Builder ===

impl<S> Builder<S> {
    const DEFAULT_ERROR_DELAY: Duration = Duration::from_secs(5);

    /// Configures the runtime to use the given [`Builder`]
    pub fn with_admin(mut self, admin: impl Into<admin::Builder>) -> Self {
        self.admin = admin.into();
        self
    }

    /// Configures the runtime to use the given [`ClientArgs`]
    pub fn with_client(mut self, client: ClientArgs) -> Self {
        self.client = Some(client);
        self
    }

    /// Configures the runtime to use the given logging configuration
    pub fn with_log(mut self, filter: LogFilter, format: LogFormat) -> Self {
        self.log = Some(LogSettings { filter, format });
        self
    }

    /// Configures the runtime to use the given fixed delay when a stream fails
    pub fn with_fixed_delay_on_error(mut self, delay: Duration) -> Self {
        self.error_delay = Some(delay);
        self
    }

    #[inline]
    async fn build_inner(self) -> Result<Runtime<S>, BuildError> {
        self.log.unwrap_or_default().try_init()?;
        let client = self.client.unwrap_or_default().try_client().await?;
        let (shutdown, shutdown_rx) = shutdown::sigint_or_sigterm()?;
        let admin = self.admin.bind()?;
        Ok(Runtime {
            client,
            shutdown_rx,
            shutdown,
            admin,
            error_delay: self.error_delay.unwrap_or(Self::DEFAULT_ERROR_DELAY),
            initialized: Initialized::default(),
            // Server must be built by `Builder::build`
            server: self.server,
        })
    }
}

#[cfg(feature = "server")]
impl Builder<NoServer> {
    /// Configures the runtime to start a server with the given [`ServerArgs`]
    #[cfg_attr(docsrs, doc(cfg(all(features = "runtime", feature = "server"))))]
    pub fn with_server(self, server: ServerArgs) -> Builder<ServerArgs> {
        Builder {
            server,
            admin: self.admin,
            client: self.client,
            error_delay: self.error_delay,
            log: self.log,
        }
    }

    /// Configures the runtime to optionally start a server with the given [`ServerArgs`]
    ///
    /// This is useful for runtimes that usually run an admission controller, but may want to
    /// support running without it when running outside the cluster.
    #[cfg_attr(docsrs, doc(cfg(all(features = "runtime", feature = "server"))))]
    pub fn with_optional_server(self, server: Option<ServerArgs>) -> Builder<Option<ServerArgs>> {
        Builder {
            server,
            admin: self.admin,
            client: self.client,
            error_delay: self.error_delay,
            log: self.log,
        }
    }
}

impl Builder<NoServer> {
    /// Attempts to build a runtime by initializing logs, loading the default Kubernetes client,
    /// registering signal handlers and binding an admin server
    pub async fn build(self) -> Result<Runtime<NoServer>, BuildError> {
        self.build_inner().await
    }
}

#[cfg(feature = "server")]
impl Builder<ServerArgs> {
    /// Attempts to build a runtime by initializing logs, loading the default Kubernetes client,
    /// registering signal handlers and binding admin and HTTPS servers
    #[cfg_attr(docsrs, doc(cfg(all(features = "runtime", feature = "server"))))]
    pub async fn build(self) -> Result<Runtime<server::Bound>, BuildError> {
        let rt = self.build_inner().await?;
        let server = rt.server.bind().await?;

        Ok(Runtime {
            server,
            admin: rt.admin,
            client: rt.client,
            error_delay: rt.error_delay,
            initialized: rt.initialized,
            shutdown_rx: rt.shutdown_rx,
            shutdown: rt.shutdown,
        })
    }
}

#[cfg(feature = "server")]
impl Builder<Option<ServerArgs>> {
    /// Attempts to build a runtime by initializing logs, loading the default Kubernetes client,
    /// registering signal handlers and binding admin and HTTPS servers
    #[cfg_attr(docsrs, doc(cfg(all(features = "runtime", feature = "server"))))]
    pub async fn build(self) -> Result<Runtime<Option<server::Bound>>, BuildError> {
        let rt = self.build_inner().await?;
        let server = match rt.server {
            Some(s) => Some(s.bind().await?),
            None => None,
        };

        Ok(Runtime {
            server,
            admin: rt.admin,
            client: rt.client,
            error_delay: rt.error_delay,
            initialized: rt.initialized,
            shutdown_rx: rt.shutdown_rx,
            shutdown: rt.shutdown,
        })
    }
}

// === impl Runtime ===

impl<S> Runtime<S> {
    /// Obtains the runtime's default Kubernetes client.
    #[inline]
    pub fn client(&self) -> Client {
        self.client.clone()
    }

    /// Creates a new initization handle used to block readiness
    #[inline]
    pub fn initialized_handle(&mut self) -> initialized::Handle {
        self.initialized.add_handle()
    }

    /// Obtains a handle to he admin server's readiness state
    #[inline]
    pub fn readiness(&self) -> Readiness {
        self.admin.readiness()
    }

    /// Obtains a handle that can be used to instrument graceful shutdown
    #[inline]
    pub fn shutdown_handle(&self) -> shutdown::Watch {
        self.shutdown_rx.clone()
    }

    /// Wraps the given `Future` or `Stream` so that it completes when the runtime is shutdown
    pub fn cancel_on_shutdown<T>(&self, inner: T) -> shutdown::CancelOnShutdown<T> {
        shutdown::CancelOnShutdown::new(self.shutdown_rx.clone(), inner)
    }

    #[cfg(feature = "requeue")]
    #[cfg_attr(docsrs, doc(cfg(all(features = "runtime", feature = "requeue"))))]
    /// Wraps the given `Future` or `Stream` so that it completes when the runtime is shutdown
    pub fn requeue<T>(
        &self,
        capacity: usize,
    ) -> (
        crate::requeue::Sender<T>,
        shutdown::CancelOnShutdown<crate::requeue::Receiver<T>>,
    )
    where
        T: Eq + std::hash::Hash,
    {
        let (tx, rx) = crate::requeue::channel(capacity);
        let rx = shutdown::CancelOnShutdown::new(self.shutdown_rx.clone(), rx);
        (tx, rx)
    }

    /// Creates a watch with the given [`Api`]
    ///
    /// If the underlying stream encounters errors, the request is retried (potentially after a
    /// delay).
    ///
    /// The runtime is not considered initialized until the returned stream returns at least one
    /// event.
    ///
    /// The return stream terminates when the runtime receives a shutdown signal.
    pub fn watch<T>(
        &mut self,
        api: Api<T>,
        watcher_config: watcher::Config,
    ) -> impl Stream<Item = watcher::Event<T>>
    where
        T: Resource + DeserializeOwned + Clone + Debug + Send + 'static,
        T::DynamicType: Default,
    {
        let watch = watcher::watcher(api, watcher_config);
        let successful = errors::LogAndSleep::fixed_delay(self.error_delay, watch);
        let initialized = self.initialized.add_handle().release_on_ready(successful);
        shutdown::CancelOnShutdown::new(self.shutdown_rx.clone(), initialized)
    }

    /// Creates a cluster-level watch on the default Kubernetes client
    ///
    /// See [`Runtime::watch`] for more details.
    #[inline]
    pub fn watch_all<T>(
        &mut self,
        watcher_config: watcher::Config,
    ) -> impl Stream<Item = watcher::Event<T>>
    where
        T: Resource + DeserializeOwned + Clone + Debug + Send + 'static,
        T::DynamicType: Default,
    {
        self.watch(Api::all(self.client()), watcher_config)
    }

    /// Creates a namespace-level watch on the default Kubernetes client
    ///
    /// See [`Runtime::watch`] for more details.
    #[inline]
    pub fn watch_namespaced<T>(
        &mut self,
        ns: impl AsRef<str>,
        watcher_config: watcher::Config,
    ) -> impl Stream<Item = watcher::Event<T>>
    where
        T: Resource<Scope = NamespaceResourceScope>,
        T: DeserializeOwned + Clone + Debug + Send + 'static,
        T::DynamicType: Default,
    {
        let api = Api::namespaced(self.client(), ns.as_ref());
        self.watch(api, watcher_config)
    }

    /// Creates a cached watch with the given [`Api`]
    ///
    /// The returned [`Store`] is updated as the returned stream is polled. If the underlying stream
    /// encounters errors, the request is retried (potentially after a delay).
    ///
    /// The runtime is not considered initialized until the returned stream returns at least one
    /// event.
    ///
    /// The return stream terminates when the runtime receives a shutdown signal.
    pub fn cache<T>(
        &mut self,
        api: Api<T>,
        watcher_config: watcher::Config,
    ) -> (Store<T>, impl Stream<Item = watcher::Event<T>>)
    where
        T: Resource + DeserializeOwned + Clone + Debug + Send + 'static,
        T::DynamicType: Clone + Default + Eq + Hash + Clone,
    {
        let writer = reflector::store::Writer::<T>::default();
        let store = writer.as_reader();

        let watch = watcher::watcher(api, watcher_config);
        let cached = reflector::reflector(writer, watch);
        let successful = errors::LogAndSleep::fixed_delay(self.error_delay, cached);
        let initialized = self.initialized.add_handle().release_on_ready(successful);
        let graceful = shutdown::CancelOnShutdown::new(self.shutdown_rx.clone(), initialized);

        (store, graceful)
    }

    /// Creates a cached cluster-level watch on the default Kubernetes client
    ///
    /// See [`Runtime::cache`] for more details.
    #[inline]
    pub fn cache_all<T>(
        &mut self,
        watcher_config: watcher::Config,
    ) -> (Store<T>, impl Stream<Item = watcher::Event<T>>)
    where
        T: Resource + DeserializeOwned + Clone + Debug + Send + 'static,
        T::DynamicType: Clone + Default + Eq + Hash + Clone,
    {
        self.cache(Api::all(self.client()), watcher_config)
    }

    /// Creates a cached namespace-level watch on the default Kubernetes client
    ///
    /// See [`Runtime::cache`] for more details.
    #[inline]
    pub fn cache_namespaced<T>(
        &mut self,
        ns: impl AsRef<str>,
        watcher_config: watcher::Config,
    ) -> (Store<T>, impl Stream<Item = watcher::Event<T>>)
    where
        T: Resource<Scope = NamespaceResourceScope>,
        T: DeserializeOwned + Clone + Debug + Send + 'static,
        T::DynamicType: Clone + Default + Eq + Hash + Clone,
    {
        let api = Api::namespaced(self.client(), ns.as_ref());
        self.cache(api, watcher_config)
    }
}

#[cfg(feature = "server")]
impl Runtime<server::Bound> {
    /// Returns the bound local address of the server
    pub fn server_addr(&self) -> std::net::SocketAddr {
        self.server.local_addr()
    }

    /// Spawns the HTTPS server with the given `service`. A runtime handle without the bound server
    /// configuration is returned.
    ///
    /// The server shuts down gracefully when the runtime is shutdown.
    pub fn spawn_server<S, B>(self, service: S) -> Runtime<NoServer>
    where
        S: Service<hyper::Request<hyper::Body>, Response = hyper::Response<B>>
            + Clone
            + Send
            + 'static,
        S::Error: std::error::Error + Send + Sync,
        S::Future: Send,
        B: hyper::body::HttpBody + Send + 'static,
        B::Data: Send,
        B::Error: std::error::Error + Send + Sync,
    {
        self.server.spawn(service, self.shutdown_rx.clone());

        Runtime {
            admin: self.admin,
            client: self.client,
            error_delay: self.error_delay,
            initialized: self.initialized,
            server: NoServer(()),
            shutdown_rx: self.shutdown_rx,
            shutdown: self.shutdown,
        }
    }
}

#[cfg(feature = "server")]
impl Runtime<Option<server::Bound>> {
    /// Returns the bound local address of the server
    pub fn server_addr(&self) -> Option<std::net::SocketAddr> {
        self.server.as_ref().map(|s| s.local_addr())
    }

    /// Spawns the HTTPS server, if bound, with the given `service`. A runtime handle without the
    /// bound server configuration is returned.
    ///
    /// The server shuts down gracefully when the runtime is shutdown.
    pub fn spawn_server<S, B, F>(self, mk: F) -> Runtime<NoServer>
    where
        F: FnOnce() -> S,
        S: Service<hyper::Request<hyper::Body>, Response = hyper::Response<B>>
            + Clone
            + Send
            + 'static,
        S::Error: std::error::Error + Send + Sync,
        S::Future: Send,
        B: hyper::body::HttpBody + Send + 'static,
        B::Data: Send,
        B::Error: std::error::Error + Send + Sync,
    {
        if let Some(s) = self.server {
            s.spawn(mk(), self.shutdown_rx.clone());
        } else {
            tracing::debug!("No server is configured")
        }

        Runtime {
            admin: self.admin,
            client: self.client,
            error_delay: self.error_delay,
            initialized: self.initialized,
            server: NoServer(()),
            shutdown_rx: self.shutdown_rx,
            shutdown: self.shutdown,
        }
    }
}

impl Runtime<NoServer> {
    /// Creates a runtime builder
    pub fn builder() -> Builder<NoServer> {
        Builder::default()
    }

    /// Runs the runtime until it is shutdown
    ///
    /// Shutdown starts when a SIGINT or SIGTERM signal is received and completes when all
    /// components have terminated gracefully or when a subsequent signal is received.
    ///
    /// The admin server's readiness endpoint returns success only once all watches (and other
    /// initalized components) have become ready and then returns an error after shutdown is
    /// initiated.
    pub async fn run(self) -> Result<(), shutdown::Aborted> {
        let Self {
            admin,
            initialized,
            shutdown,
            shutdown_rx,
            ..
        } = self;

        let admin = admin.spawn();

        // Set the admin readiness to succeed once all initilization handles have been released.
        let ready = admin.readiness();
        tokio::spawn(async move {
            initialized.initialized().await;
            ready.set(true);
            tracing::debug!("initialized");

            drop(shutdown_rx.signaled().await);
            ready.set(false);
            tracing::debug!("shutdown");
        });

        shutdown.signaled().await?;

        Ok(())
    }
}

// === impl LogSettings ===

impl Default for LogSettings {
    fn default() -> Self {
        Self {
            filter: LogFilter::from_default_env(),
            format: LogFormat::default(),
        }
    }
}

impl LogSettings {
    fn try_init(self) -> Result<(), LogInitError> {
        self.format.try_init(self.filter)
    }
}
