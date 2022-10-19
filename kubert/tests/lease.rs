#![cfg(feature = "lease")]
#![deny(warnings, rust_2018_idioms)]

use k8s_openapi::{
    api::coordination::v1 as coordv1,
    apimachinery::pkg::apis::meta::v1::{self as metav1, MicroTime},
};
use kubert::Lease;
use maplit::{btreemap, convert_args};
use tokio::time;

type Api = kube::Api<coordv1::Lease>;

#[tokio::test(flavor = "current_thread")]
async fn lease() {
    let handle = Handle::setup().await;

    // Create a lease instance and claim it.

    let lease0 = handle.init_new().await;
    let params0 = kubert::lease::ClaimParams {
        identity: "id-0".into(),
        lease_duration: time::Duration::from_secs(3),
        renew_grace_period: None,
    };
    let claim0 = lease0.claim(&params0).await.expect("claim0");
    assert!(claim0.is_currently_held_by(&*params0.identity));

    // Create another lease instance and try to claim it--the prior lease should
    // have precedence.

    let lease1 = handle.init_new().await;
    let params1 = kubert::lease::ClaimParams {
        identity: "id-1".into(),
        lease_duration: time::Duration::from_secs(5),
        renew_grace_period: Some(time::Duration::from_secs(3)),
    };
    let claim1 = lease1.claim(&params1).await.expect("claim1");
    assert_eq!(claim0.holder, claim1.holder);
    assert_eq!(claim0.expiry.timestamp(), claim1.expiry.timestamp());
    assert!(claim0.is_currently_held_by(&*params0.identity));
    assert!(claim1.is_currently_held_by(&*params0.identity));
    assert!(!claim0.is_currently_held_by(&*params1.identity));
    assert!(!claim1.is_currently_held_by(&*params1.identity));

    // Wait for the claim to expire.
    claim0.sleep_until_expiry().await;

    // Now, trying to claim the lease should succeed.
    let claim2 = lease1.claim(&params1).await.expect("claim1");
    assert!(!claim2.is_currently_held_by(&*params0.identity));
    assert!(claim2.is_currently_held_by(&*params1.identity));

    // Inspect the lease resource to verify that it has all expected fields.
    let rsrc = handle.get().await;
    assert_eq!(
        rsrc.holder_identity.as_deref().expect("holderIdentity"),
        claim2.holder
    );
    assert_eq!(
        rsrc.renew_time
            .as_ref()
            .map(|MicroTime(t)| t)
            .expect("renewTime")
            .to_rfc3339_opts(chrono::SecondsFormat::Secs, true),
        (claim2.expiry - chrono::Duration::from_std(params1.lease_duration).unwrap())
            .to_rfc3339_opts(chrono::SecondsFormat::Secs, true),
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
        params1.lease_duration
    );
    assert_eq!(rsrc.lease_transitions, Some(2));

    // Wait until we can renew the lease (before the claim expires).
    claim2
        .sleep_until_before_expiry(time::Duration::from_secs(3))
        .await;
    let claim3 = lease1.claim(&params1).await.expect("claim3");
    assert!(claim3.is_currently_held_by(&*params1.identity));

    // Ensure that the lease's renew time is updated but the acquire time is
    // not.
    let rsrc = handle.get().await;
    assert_eq!(
        rsrc.holder_identity.as_deref().expect("holderIdentity"),
        params1.identity
    );
    assert_eq!(
        rsrc.renew_time
            .as_ref()
            .map(|MicroTime(t)| t)
            .expect("renewTime")
            .to_rfc3339_opts(chrono::SecondsFormat::Secs, true),
        (claim3.expiry - chrono::Duration::from_std(params1.lease_duration).unwrap())
            .to_rfc3339_opts(chrono::SecondsFormat::Secs, true),
    );
    assert!(rsrc.acquire_time < rsrc.renew_time);
    assert_eq!(
        time::Duration::from_secs(
            rsrc.lease_duration_seconds
                .expect("leaseDurationSeconds")
                .try_into()
                .unwrap()
        ),
        params1.lease_duration
    );
    assert_eq!(rsrc.lease_transitions, Some(2));

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
    const FIELD_MANAGER: &'static str = "kubert-test";
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

    async fn init_new(&self) -> Lease {
        Lease::init(self.api.clone(), &self.name, Self::FIELD_MANAGER)
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
