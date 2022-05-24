use super::*;

#[derive(
    Clone, Debug, kube::CustomResource, serde::Deserialize, serde::Serialize, schemars::JsonSchema,
)]
#[kube(
    group = "gateway.networking.k8s.io/v1alpha2",
    version = "v1alpha2",
    kind = "HttpRoute",
    status = "HttpRouteStatus",
    namespaced
)]
#[serde(rename_all = "camelCase")]
pub struct HttpRouteSpec {
    #[serde(flatten)]
    pub inner: CommonRouteSpec,
    pub hostnames: Option<Vec<Hostname>>,
    pub rules: Option<Vec<HttpRouteRule>>,
}

#[derive(Clone, Debug, PartialEq, serde::Deserialize, serde::Serialize, schemars::JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct HttpRouteRule {
    pub matches: Option<HttpRouteMatch>,
    pub filters: Option<Vec<HttpRouteFilter>>,
    pub backend_refs: Option<Vec<HttpBackendRef>>,
}

pub type PathMatchType = String;

#[derive(Clone, Debug, PartialEq, serde::Deserialize, serde::Serialize, schemars::JsonSchema)]
pub struct HttpPathMatch {
    pub r#type: Option<PathMatchType>,
    pub value: Option<String>,
}

pub type HeaderMatchType = String;

pub type HttpHeaderName = String;

#[derive(Clone, Debug, PartialEq, serde::Deserialize, serde::Serialize, schemars::JsonSchema)]
pub struct HttpHeaderMatch {
    pub r#type: HeaderMatchType,
    pub name: HttpHeaderName,
    pub value: String,
}

pub type QueryParamMatchType = String;

#[derive(Clone, Debug, PartialEq, serde::Deserialize, serde::Serialize, schemars::JsonSchema)]
pub struct HttpQueryParamMatch {
    pub r#type: QueryParamMatchType,
    pub name: String,
    pub value: String,
}

pub type HttpMethod = String;

#[derive(Clone, Debug, PartialEq, serde::Deserialize, serde::Serialize, schemars::JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct HttpRouteMatch {
    pub path: Option<HttpPathMatch>,
    pub headers: Option<Vec<HttpHeaderMatch>>,
    pub query_params: Option<Vec<HttpQueryParamMatch>>,
    pub method: Option<HttpMethod>,
}

#[derive(Clone, Debug, PartialEq, serde::Deserialize, serde::Serialize, schemars::JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct HttpRouteFilter {
    pub r#type: HttpRouteFilterType,
    pub request_header_modifier: Option<HttpRequestHeaderFilter>,
    pub request_mirror: Option<HttpRequestMirrorFilter>,
    pub request_redirect: Option<HttpRequestRedirectFilter>,
    pub url_rewrite: Option<HttpUrlRewriteFilter>,
    pub extension_ref: Option<LocalObjectReference>,
}

pub type HttpRouteFilterType = String;

#[derive(Clone, Debug, PartialEq, serde::Deserialize, serde::Serialize, schemars::JsonSchema)]
pub struct HttpHeader {
    pub name: HttpHeaderName,
    pub value: String,
}

#[derive(Clone, Debug, PartialEq, serde::Deserialize, serde::Serialize, schemars::JsonSchema)]
pub struct HttpRequestHeaderFilter {
    pub set: Option<Vec<HttpHeader>>,
    pub add: Option<Vec<HttpHeader>>,
    pub remove: Option<Vec<String>>,
}

pub type HttpPathModifierType = String;

#[derive(Clone, Debug, PartialEq, serde::Deserialize, serde::Serialize, schemars::JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct HttpPathModifier {
    pub r#type: HttpPathModifierType,
    pub replace_full_path: Option<String>,
    pub replace_prefix_match: Option<String>,
}

#[derive(Clone, Debug, PartialEq, serde::Deserialize, serde::Serialize, schemars::JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct HttpRequestRedirectFilter {
    pub r#type: HttpPathModifierType,
    pub scheme: Option<String>,
    pub hostname: Option<PreciseHostname>,
    pub path: Option<HttpPathModifier>,
    pub port: Option<PortNumber>,
    pub status_code: Option<u16>,
}

#[derive(Clone, Debug, PartialEq, serde::Deserialize, serde::Serialize, schemars::JsonSchema)]
pub struct HttpUrlRewriteFilter {
    pub hostname: Option<PreciseHostname>,
    pub path: Option<HttpPathModifier>,
}

#[derive(Clone, Debug, PartialEq, serde::Deserialize, serde::Serialize, schemars::JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct HttpRequestMirrorFilter {
    pub backend_ref: BackendObjectReference,
}

#[derive(Clone, Debug, PartialEq, serde::Deserialize, serde::Serialize, schemars::JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct HttpBackendRef {
    pub backend_ref: BackendObjectReference,
    pub filters: Option<Vec<HttpRouteFilter>>,
}

#[derive(Clone, Debug, PartialEq, serde::Deserialize, serde::Serialize, schemars::JsonSchema)]
pub struct HttpRouteStatus {
    #[serde(flatten)]
    pub inner: RouteStatus,
}
