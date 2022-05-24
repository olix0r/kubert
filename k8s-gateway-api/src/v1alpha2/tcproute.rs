use super::*;

#[derive(
    Clone, Debug, kube::CustomResource, serde::Deserialize, serde::Serialize, schemars::JsonSchema,
)]
#[kube(
    group = "gateway.networking.k8s.io/v1alpha2",
    version = "v1alpha2",
    kind = "TcpRoute",
    status = "TcpRouteStatus",
    namespaced
)]
pub struct TcpRouteSpec {
    #[serde(flatten)]
    pub inner: CommonRouteSpec,

    pub rules: Vec<TcpRouteRule>,
}

#[derive(Clone, Debug, PartialEq, serde::Deserialize, serde::Serialize, schemars::JsonSchema)]
pub struct TcpRouteStatus {
    #[serde(flatten)]
    pub inner: RouteStatus,
}

#[derive(Clone, Debug, PartialEq, serde::Deserialize, serde::Serialize, schemars::JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct TcpRouteRule {
    pub backend_refs: Vec<BackendRef>,
}

