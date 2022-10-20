#![deny(warnings, rust_2018_idioms)]

use k8s_openapi::{
    api::coordination::v1 as coordv1,
    apimachinery::pkg::apis::meta::v1::{self as metav1},
};
use kubert::LeaseManager;
use maplit::{btreemap, convert_args};
use tokio::time;

type Api = kube::Api<coordv1::Lease>;

macro_rules! assert_time_eq {
    ($a:expr, $b:expr $(,)?) => {
        assert_eq!(
            $a.to_rfc3339_opts(chrono::SecondsFormat::Millis, true),
            $b.to_rfc3339_opts(chrono::SecondsFormat::Millis, true),
        );
    };
}

#[tokio::test(flavor = "current_thread")]
async fn exclusive() {
    let handle = Handle::setup().await;

    // Create a lease instance and claim it.

    let lease0 = handle.init_new().await;
    let params = kubert::lease::ClaimParams {
        lease_duration: time::Duration::from_secs(3),
        ..Default::default()
    };
    let claim0 = lease0
        .ensure_claimed("alice", &params)
        .await
        .expect("claim");
    assert!(claim0.is_current_for("alice"));

    // Create another lease instance and try to claim it--the prior lease should
    // have precedence.

    let lease1 = handle.init_new().await;
    let claim1 = lease1.ensure_claimed("bob", &params).await.expect("claim");
    assert_eq!(claim0.holder, claim1.holder);
    assert_eq!(claim0.expiry.timestamp(), claim1.expiry.timestamp());
    assert!(claim0.is_current_for("alice"));
    assert!(claim1.is_current_for("alice"));
    assert!(!claim0.is_current_for("bob"));
    assert!(!claim1.is_current_for("bob"));

    // Inspect the lease resource to verify that it has all expected fields.
    let rsrc = handle.get().await;
    assert_eq!(
        rsrc.holder_identity.as_deref().expect("holderIdentity"),
        "alice"
    );
    assert_time_eq!(
        rsrc.renew_time
            .as_ref()
            .map(|metav1::MicroTime(t)| t)
            .expect("renewTime"),
        claim0.expiry - chrono::Duration::from_std(params.lease_duration).unwrap()
    );
    // Since we just acquired this, the acquire time and renew time are the
    // same.
    assert_time_eq!(
        rsrc.acquire_time.as_ref().unwrap().0,
        rsrc.renew_time.as_ref().unwrap().0
    );
    assert_eq!(
        time::Duration::from_secs(
            rsrc.lease_duration_seconds
                .expect("leaseDurationSeconds")
                .try_into()
                .unwrap()
        ),
        params.lease_duration
    );
    assert_eq!(rsrc.lease_transitions, Some(1));

    handle.delete().await;
}

#[tokio::test(flavor = "current_thread")]
async fn expires() {
    let handle = Handle::setup().await;

    let lease = handle.init_new().await;
    let params = kubert::lease::ClaimParams {
        lease_duration: time::Duration::from_secs(3),
        ..Default::default()
    };
    let claim0 = lease.ensure_claimed("alice", &params).await.expect("claim");
    assert!(claim0.is_current_for("alice"));

    // Wait for the claim to expire.
    claim0.expire().await;

    // Claiming with another identity should succeed.
    let claim1 = lease.ensure_claimed("bob", &params).await.expect("claim");
    assert!(claim1.is_current_for("bob"));

    // Inspect the lease resource to verify that it has all expected fields.
    let rsrc = handle.get().await;
    assert_eq!(
        rsrc.holder_identity.as_deref().expect("holderIdentity"),
        "bob"
    );
    assert_time_eq!(
        rsrc.renew_time
            .as_ref()
            .map(|metav1::MicroTime(t)| t)
            .expect("renewTime"),
        claim1.expiry - chrono::Duration::from_std(params.lease_duration).unwrap(),
    );
    // Since we just acquired this, the acquire time and renew time are the
    // same.
    assert_eq!(rsrc.acquire_time, rsrc.renew_time);
    assert_eq!(
        time::Duration::from_secs(
            rsrc.lease_duration_seconds
                .expect("leaseDurationSeconds")
                .try_into()
                .unwrap()
        ),
        params.lease_duration
    );
    assert_eq!(rsrc.lease_transitions, Some(2));

    handle.delete().await;
}

#[tokio::test(flavor = "current_thread")]
async fn renews() {
    let handle = Handle::setup().await;

    let lease = handle.init_new().await;
    let params = kubert::lease::ClaimParams {
        lease_duration: time::Duration::from_secs(8),
        renew_grace_period: time::Duration::from_secs(5),
    };
    let claim0 = lease.ensure_claimed("alice", &params).await.expect("claim");
    assert!(claim0.is_current_for("alice"));

    tokio::time::sleep(time::Duration::from_secs(1)).await;

    // Trying to claim again does not change the expiry.
    let claim1 = lease.ensure_claimed("alice", &params).await.expect("claim");
    assert_eq!(claim0, claim1);

    // Wait for the claim to be renewable.
    claim0.expire_with_grace(params.renew_grace_period).await;

    // Claiming now (before the expiry) should update the expiry.
    let claim2 = lease.ensure_claimed("alice", &params).await.expect("claim");
    assert!(claim2.is_current_for("alice"));
    assert_ne!(claim2, claim0);

    // Inspect the lease resource to verify that it has all expected fields.
    let rsrc = handle.get().await;
    assert_eq!(
        rsrc.holder_identity.as_deref().expect("holderIdentity"),
        "alice"
    );
    assert_time_eq!(
        rsrc.renew_time
            .as_ref()
            .map(|metav1::MicroTime(t)| t)
            .expect("renewTime"),
        claim2.expiry - chrono::Duration::from_std(params.lease_duration).unwrap(),
    );
    assert_time_eq!(
        rsrc.acquire_time
            .as_ref()
            .map(|metav1::MicroTime(t)| t)
            .expect("renewTime"),
        claim0.expiry - chrono::Duration::from_std(params.lease_duration).unwrap(),
    );
    assert_eq!(
        time::Duration::from_secs(
            rsrc.lease_duration_seconds
                .expect("leaseDurationSeconds")
                .try_into()
                .unwrap()
        ),
        params.lease_duration
    );
    assert_eq!(rsrc.lease_transitions, Some(1));

    // Wait for the claim to expire completely.
    claim2.expire().await;

    // Create a new lease that does not share internal state and use it to claim the lease for Bob.
    let lease1 = handle.init_new().await;
    let claim3 = lease1.ensure_claimed("bob", &params).await.expect("claim");
    assert!(claim3.is_current_for("bob"));

    // The original lease must
    let claim4 = lease.ensure_claimed("alice", &params).await.expect("claim");
    assert!(claim4.is_current_for("bob"));

    // Inspect the lease resource to verify that it has all expected fields.
    let rsrc = handle.get().await;
    assert_eq!(
        rsrc.holder_identity.as_deref().expect("holderIdentity"),
        "bob"
    );
    assert_time_eq!(
        rsrc.renew_time
            .as_ref()
            .map(|metav1::MicroTime(t)| t)
            .expect("renewTime"),
        claim3.expiry - chrono::Duration::from_std(params.lease_duration).unwrap(),
    );
    assert_time_eq!(
        rsrc.acquire_time
            .as_ref()
            .map(|metav1::MicroTime(t)| t)
            .expect("renewTime"),
        claim3.expiry - chrono::Duration::from_std(params.lease_duration).unwrap(),
    );
    assert_eq!(
        time::Duration::from_secs(
            rsrc.lease_duration_seconds
                .expect("leaseDurationSeconds")
                .try_into()
                .unwrap()
        ),
        params.lease_duration
    );
    assert_eq!(rsrc.lease_transitions, Some(2));

    handle.delete().await;
}

#[tokio::test(flavor = "current_thread")]
async fn abidcates() {
    let handle = Handle::setup().await;

    let lease = handle.init_new().await;
    let params = kubert::lease::ClaimParams {
        lease_duration: time::Duration::from_secs(3),
        ..Default::default()
    };
    let claim0 = lease.ensure_claimed("id", &params).await.expect("claim");
    assert!(claim0.is_current_for("id"));
    let released = lease.abdicate("id").await.expect("release");
    assert!(released);

    // Inspect the lease resource to verify that it has all expected fields.
    let rsrc = handle.get().await;
    assert_eq!(rsrc.holder_identity, None,);
    assert_eq!(rsrc.renew_time, None,);
    assert_eq!(rsrc.acquire_time, None);
    assert_eq!(rsrc.lease_duration_seconds, None);
    assert_eq!(rsrc.lease_transitions, Some(1));

    handle.delete().await;
}

// === Utils ===

struct Handle {
    api: Api,
    name: String,
    _guard: tracing::subscriber::DefaultGuard,
}

impl Handle {
    const NS: &'static str = "default";
    const LABEL: &'static str = "kubert.olix0r.net/test";

    async fn setup() -> Self {
        let _guard = Self::init_tracing();
        let client = kube::Client::try_default().await.expect("client");
        let api = Api::namespaced(client, Self::NS);
        let name = Self::rand_name("kubert-test");
        api.create(
            &Default::default(),
            &coordv1::Lease {
                metadata: metav1::ObjectMeta {
                    name: Some(name.clone()),
                    namespace: Some(Self::NS.to_string()),
                    labels: Some(convert_args!(btreemap!(
                        Self::LABEL => std::thread::current().name().expect("thread name"),
                    ))),
                    ..Default::default()
                },
                ..Default::default()
            },
        )
        .await
        .expect("create lease");
        Handle { api, name, _guard }
    }

    async fn init_new(&self) -> LeaseManager {
        LeaseManager::init(self.api.clone(), &self.name)
            .await
            .expect("lease must initialize")
    }

    async fn get(&self) -> coordv1::LeaseSpec {
        self.api
            .get(&self.name)
            .await
            .expect("get")
            .spec
            .expect("spec")
    }

    async fn delete(self) {
        self.api
            .delete(&self.name, &Default::default())
            .await
            .expect("delete");
    }

    fn rand_name(base: impl std::fmt::Display) -> String {
        use rand::Rng;

        struct LowercaseAlphanumeric;

        // Modified from `rand::distributions::Alphanumeric`
        //
        // Copyright 2018 Developers of the Rand project
        // Copyright (c) 2014 The Rust Project Developers
        impl rand::distributions::Distribution<u8> for LowercaseAlphanumeric {
            fn sample<R: rand::Rng + ?Sized>(&self, rng: &mut R) -> u8 {
                const RANGE: u32 = 26 + 10;
                const CHARSET: &[u8] = b"abcdefghijklmnopqrstuvwxyz0123456789";
                loop {
                    let var = rng.next_u32() >> (32 - 6);
                    if var < RANGE {
                        return CHARSET[var as usize];
                    }
                }
            }
        }

        let suffix = rand::thread_rng()
            .sample_iter(&LowercaseAlphanumeric)
            .take(5)
            .map(char::from)
            .collect::<String>();
        format!("{}-{}", base, suffix)
    }

    fn init_tracing() -> tracing::subscriber::DefaultGuard {
        tracing::subscriber::set_default(
            tracing_subscriber::fmt()
                .with_test_writer()
                .with_thread_names(true)
                // .without_time()
                .with_env_filter(
                    tracing_subscriber::EnvFilter::try_from_default_env().unwrap_or_else(|_| {
                        "trace,tower=info,hyper=info,kube=info,h2=info"
                            .parse()
                            .unwrap()
                    }),
                )
                .finish(),
        )
    }
}
