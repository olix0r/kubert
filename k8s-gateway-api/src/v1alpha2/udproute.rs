use super::*;

#[derive(
    Clone, Debug, kube::CustomResource, serde::Deserialize, serde::Serialize, schemars::JsonSchema,
)]
#[kube(
    group = "gateway.networking.k8s.io/v1alpha2",
    version = "v1alpha2",
    kind = "UdpRoute",
    status = "UdpRouteStatus",
    namespaced
)]
pub struct UdpRouteSpec {
    #[serde(flatten)]
    pub inner: CommonRouteSpec,

    pub rules: Vec<UdpRouteRule>,
}

#[derive(Clone, Debug, PartialEq, serde::Deserialize, serde::Serialize, schemars::JsonSchema)]
pub struct UdpRouteStatus {
    #[serde(flatten)]
    pub inner: RouteStatus,
}

#[derive(Clone, Debug, PartialEq, serde::Deserialize, serde::Serialize, schemars::JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct UdpRouteRule {
    pub backend_refs: Vec<BackendRef>,
}
