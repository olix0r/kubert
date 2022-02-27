//! A controller runtime

#[cfg(feature = "server")]
use crate::server::{self, ServerArgs};
use crate::{
    admin::{self, AdminArgs, Readiness},
    client::{self, Client, ClientArgs},
    errors,
    initialized::{self, Initialized},
    shutdown, LogFilter, LogFormat, LogInitError,
};
use futures_core::Stream;
use kube_core::{params::ListParams, Resource};
use kube_runtime::{reflector, watcher};
use serde::de::DeserializeOwned;
use std::{fmt::Debug, hash::Hash, time::Duration};

pub use kube_client::Api;
pub use reflector::Store;

/// Configures a controller [`Runtime`]
#[derive(Debug, Default)]
#[must_use]
pub struct Builder<S = NoServer> {
    admin: Option<AdminArgs>,
    client: Option<ClientArgs>,
    error_delay: Option<Duration>,
    log: Option<LogSettings>,

    #[cfg(feature = "server")]
    server: S,
    #[cfg(not(feature = "server"))]
    server: std::marker::PhantomData<S>,
}

/// A configured runtime that can be used to instrument and run a controller
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
#[derive(Debug)]
pub struct NoServer(());

/// Indicates that the [`Builder`] could not configure a [`Runtime`]
#[derive(Debug, thiserror::Error)]
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

    /// Configures the runtime to use the given [`AdminArgs`]
    pub fn with_admin(mut self, admin: AdminArgs) -> Self {
        self.admin = Some(admin);
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
    async fn try_build_inner(self) -> Result<Runtime<S>, BuildError> {
        self.log.unwrap_or_default().try_init()?;
        let admin = self.admin.unwrap_or_default().into_builder().bind()?;
        let client = self.client.unwrap_or_default().try_client().await?;
        let (shutdown, shutdown_rx) = shutdown::try_register()?;
        Ok(Runtime {
            client,
            shutdown_rx,
            shutdown,
            admin,
            error_delay: self.error_delay.unwrap_or(Self::DEFAULT_ERROR_DELAY),
            initialized: Initialized::default(),
            server: self.server,
        })
    }
}

impl Builder<NoServer> {
    #[cfg(feature = "server")]
    /// Configures the runtime to start a server with the given [`ServerArgs`]
    pub fn with_server(self, server: ServerArgs) -> Builder<ServerArgs> {
        Builder {
            server,
            admin: self.admin,
            client: self.client,
            error_delay: self.error_delay,
            log: self.log,
        }
    }

    /// Attempts to build a runtime
    pub async fn try_build(self) -> Result<Runtime<NoServer>, BuildError> {
        self.try_build_inner().await
    }
}

#[cfg(feature = "server")]
impl Builder<ServerArgs> {
    /// Attempts to build a runtime
    pub async fn try_build(self) -> Result<Runtime<server::Bound>, BuildError> {
        let rt = self.try_build_inner().await?;
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
    pub fn shutdown_handle(&self) -> drain::Watch {
        self.shutdown_rx.clone()
    }

    /// Creates a watch with the given [`Api`]
    pub fn watch<T>(
        &mut self,
        api: Api<T>,
        params: ListParams,
    ) -> impl Stream<Item = watcher::Event<T>>
    where
        T: Resource + DeserializeOwned + Clone + Debug + Default + Send + 'static,
        T::DynamicType: Default,
    {
        self.initialized
            .add_handle()
            .release_on_ready(errors::LogAndSleep::fixed_delay(
                self.error_delay,
                watcher::watcher(api, params),
            ))
    }

    /// Creates a cached watch with the given [`Api`]
    pub fn cache<T>(
        &mut self,
        api: Api<T>,
        params: ListParams,
    ) -> (Store<T>, impl Stream<Item = watcher::Event<T>>)
    where
        T: Resource + DeserializeOwned + Clone + Debug + Default + Send + 'static,
        T::DynamicType: Clone + Default + Eq + Hash + Clone,
    {
        let writer = reflector::store::Writer::<T>::default();
        let store = writer.as_reader();
        let stream =
            self.initialized
                .add_handle()
                .release_on_ready(errors::LogAndSleep::fixed_delay(
                    self.error_delay,
                    reflector::reflector(writer, watcher::watcher(api, params)),
                ));
        (store, stream)
    }

    /// Creates a cluster-level watch on the default Kubernetes client
    #[inline]
    pub fn watch_all<T>(&mut self, params: ListParams) -> impl Stream<Item = watcher::Event<T>>
    where
        T: Resource + DeserializeOwned + Clone + Debug + Default + Send + 'static,
        T::DynamicType: Default,
    {
        self.watch(Api::all(self.client()), params)
    }

    /// Creates a namespace-level watch on the default Kubernetes client
    #[inline]
    pub fn watch_namespaced<T>(
        &mut self,
        ns: impl AsRef<str>,
        params: ListParams,
    ) -> impl Stream<Item = watcher::Event<T>>
    where
        T: Resource + DeserializeOwned + Clone + Debug + Default + Send + 'static,
        T::DynamicType: Default,
    {
        let api = Api::namespaced(self.client(), ns.as_ref());
        self.watch(api, params)
    }

    /// Creates a cached cluster-level watch on the default Kubernetes client
    #[inline]
    pub fn cache_all<T>(
        &mut self,
        params: ListParams,
    ) -> (Store<T>, impl Stream<Item = watcher::Event<T>>)
    where
        T: Resource + DeserializeOwned + Clone + Debug + Default + Send + 'static,
        T::DynamicType: Clone + Default + Eq + Hash + Clone,
    {
        self.cache(Api::all(self.client()), params)
    }

    /// Creates a cached namespace-level watch on the default Kubernetes client
    #[inline]
    pub fn cache_namespaced<T>(
        &mut self,
        ns: impl AsRef<str>,
        params: ListParams,
    ) -> (Store<T>, impl Stream<Item = watcher::Event<T>>)
    where
        T: Resource + DeserializeOwned + Clone + Debug + Default + Send + 'static,
        T::DynamicType: Clone + Default + Eq + Hash + Clone,
    {
        let api = Api::namespaced(self.client(), ns.as_ref());
        self.cache(api, params)
    }
}

#[cfg(feature = "server")]
impl Runtime<server::Bound> {
    /// Spawns the HTTPS server with the given `service`, returning the runtime.
    pub fn spawn_server<S, B>(self, service: S) -> Runtime<NoServer>
    where
        S: tower_service::Service<hyper::Request<hyper::Body>, Response = hyper::Response<B>>
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

impl Runtime<NoServer> {
    /// Runs the runtime until it is shutdown
    pub async fn run(self) -> Result<(), shutdown::Aborted> {
        let Self {
            admin,
            initialized,
            shutdown,
            ..
        } = self;

        let admin = admin.spawn();

        // Set the admin readiness to succeed once all initilization handles have been released.
        let ready = admin.readiness();
        tokio::spawn(async move {
            initialized.initialized().await;
            ready.set(true);
            tracing::debug!("initialized")
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
