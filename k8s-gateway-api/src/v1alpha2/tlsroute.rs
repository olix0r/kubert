use super::*;

#[derive(
    Clone, Debug, kube::CustomResource, serde::Deserialize, serde::Serialize, schemars::JsonSchema,
)]
#[kube(
    group = "gateway.networking.k8s.io/v1alpha2",
    version = "v1alpha2",
    kind = "TlsRoute",
    status = "TlsRouteStatus",
    namespaced
)]
pub struct TlsRouteSpec {
    #[serde(flatten)]
    pub inner: CommonRouteSpec,

    pub hostnames: Vec<Hostname>,

    pub rules: Vec<TlsRouteRule>,
}

#[derive(Clone, Debug, PartialEq, serde::Deserialize, serde::Serialize, schemars::JsonSchema)]
pub struct TlsRouteStatus {
    #[serde(flatten)]
    pub inner: RouteStatus,
}

#[derive(Clone, Debug, PartialEq, serde::Deserialize, serde::Serialize, schemars::JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct TlsRouteRule {
    pub backend_refs: Vec<BackendRef>,
}
