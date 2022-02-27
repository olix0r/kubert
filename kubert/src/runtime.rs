//! A controller runtime

#![allow(missing_docs)]

#[cfg(feature = "server")]
use crate::server::{self, ServerArgs};
use crate::{
    admin::{self, AdminArgs, Readiness},
    client::{self, Client, ClientArgs},
    errors,
    initialized::{self, Initialized},
    log, shutdown,
};
use futures_core::Stream;
use kube_core::{params::ListParams, Resource};
use kube_runtime::{reflector, watcher};
use serde::de::DeserializeOwned;
use std::{fmt::Debug, hash::Hash, time::Duration};

pub use kube_client::Api;
pub use reflector::Store;

#[derive(Debug, Default)]
pub struct Builder<S = NoServer> {
    admin: Option<AdminArgs>,
    client: Option<ClientArgs>,
    error_delay: Option<Duration>,
    log: Option<LogSettings>,
    server: S,
}

pub struct Runtime<S = NoServer> {
    admin: admin::Builder,
    client: Client,
    error_delay: Duration,
    initialized: Initialized,
    shutdown_rx: drain::Watch,
    shutdown: shutdown::Shutdown,

    #[cfg_attr(not(feature = "server"), allow(dead_code))]
    server: S,
}

#[derive(Debug)]
pub struct NoServer(());

#[derive(Debug, thiserror::Error)]
pub enum BuildError {
    #[error(transparent)]
    LogInit(#[from] log::TryInitError),

    #[error(transparent)]
    Client(#[from] client::ConfigError),

    #[cfg(feature = "server")]
    #[error(transparent)]
    Server(#[from] server::Error),

    #[error(transparent)]
    Signal(#[from] shutdown::RegisterError),
}

#[derive(Debug, thiserror::Error)]
pub enum RunError {
    #[error(transparent)]
    BindAdmin(#[from] admin::Error),

    #[error(transparent)]
    SignalRegister(#[from] shutdown::RegisterError),

    #[error(transparent)]
    Aborted(#[from] shutdown::Aborted),
}

#[derive(Debug)]
struct LogSettings {
    filter: log::EnvFilter,
    format: log::LogFormat,
}

// === impl Builder ===

impl<S> Builder<S> {
    const DEFAULT_ERROR_DELAY: Duration = Duration::from_secs(5);

    pub fn with_admin(mut self, admin: AdminArgs) -> Self {
        self.admin = Some(admin);
        self
    }

    pub fn with_client(mut self, client: ClientArgs) -> Self {
        self.client = Some(client);
        self
    }

    pub fn with_log(mut self, filter: log::EnvFilter, format: log::LogFormat) -> Self {
        self.log = Some(LogSettings { filter, format });
        self
    }

    pub fn with_fixed_delay_on_error(mut self, delay: Duration) -> Self {
        self.error_delay = Some(delay);
        self
    }

    #[inline]
    async fn try_build_inner(self) -> Result<Runtime<S>, BuildError> {
        self.log.unwrap_or_default().try_init()?;
        let client = self.client.unwrap_or_default().try_client().await?;
        let (shutdown, shutdown_rx) = shutdown::try_register()?;
        Ok(Runtime {
            client,
            shutdown_rx,
            shutdown,
            admin: self.admin.unwrap_or_default().into_builder(),
            error_delay: self.error_delay.unwrap_or(Self::DEFAULT_ERROR_DELAY),
            initialized: Initialized::default(),
            server: self.server,
        })
    }
}

impl Builder<NoServer> {
    #[cfg(feature = "server")]
    pub fn with_server(self, server: ServerArgs) -> Builder<ServerArgs> {
        Builder {
            server,
            admin: self.admin,
            client: self.client,
            error_delay: self.error_delay,
            log: self.log,
        }
    }

    pub async fn try_build(self) -> Result<Runtime<NoServer>, BuildError> {
        self.try_build_inner().await
    }
}

#[cfg(feature = "server")]
impl Builder<ServerArgs> {
    pub async fn try_bind_and_build(self) -> Result<Runtime<server::Bound>, BuildError> {
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
    #[inline]
    pub fn client(&self) -> Client {
        self.client.clone()
    }

    #[inline]
    pub fn initialized_handle(&mut self) -> initialized::Handle {
        self.initialized.add_handle()
    }

    #[inline]
    pub fn readiness(&self) -> Readiness {
        self.admin.readiness()
    }

    #[inline]
    pub fn shutdown_handle(&self) -> drain::Watch {
        self.shutdown_rx.clone()
    }

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

    #[inline]
    pub fn watch_all<T>(&mut self, params: ListParams) -> impl Stream<Item = watcher::Event<T>>
    where
        T: Resource + DeserializeOwned + Clone + Debug + Default + Send + 'static,
        T::DynamicType: Default,
    {
        self.watch(Api::all(self.client()), params)
    }

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
    pub async fn spawn_server<S, B>(self, service: S) -> Runtime<NoServer>
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
        self.server.spawn(service, self.shutdown_rx.clone()).await;

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
    pub async fn run(self) -> Result<(), RunError> {
        let Self {
            admin,
            initialized,
            shutdown,
            ..
        } = self;

        let admin = admin.spawn()?;

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
            filter: log::EnvFilter::from_default_env(),
            format: log::LogFormat::default(),
        }
    }
}

impl LogSettings {
    fn try_init(self) -> Result<(), log::TryInitError> {
        self.format.try_init(self.filter)
    }
}
