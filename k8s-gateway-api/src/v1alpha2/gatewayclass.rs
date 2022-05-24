use super::*;

/// Gateway represents an instance of a service-traffic handling infrastructure
/// by binding Listeners to a set of IP addresses.
#[derive(
    Clone, Debug, kube::CustomResource, serde::Deserialize, serde::Serialize, schemars::JsonSchema,
)]
#[kube(
    group = "gateway.networking.k8s.io/v1alpha2",
    version = "v1alpha2",
    kind = "GatewayClass",
    status = "GatewayClassStatus",
    namespaced
)]
#[serde(rename_all = "camelCase")]
pub struct GatewayClassSpec {
    /// ControllerName is the name of the controller that is managing Gateways
    /// of this class. The value of this field MUST be a domain prefixed path.
    ///
    /// Example: "example.net/gateway-controller".
    ///
    /// This field is not mutable and cannot be empty.
    pub controller_name: GatewayController,

    /// ParametersRef is a reference to a resource that contains the
    /// configuration parameters corresponding to the GatewayClass. This is
    /// optional if the controller does not require any additional
    /// configuration.
    ///
    /// ParametersRef can reference a standard Kubernetes resource, i.e.
    /// ConfigMap, or an implementation-specific custom resource. The resource
    /// can be cluster-scoped or namespace-scoped.
    ///
    /// If the referent cannot be found, the GatewayClass's "InvalidParameters"
    /// status condition will be true.
    ///
    /// Support: Custom
    pub paramters_ref: Option<ParametersReference>,

    /// Description helps describe a GatewayClass with more details.
    pub description: Option<String>,
}

#[derive(Clone, Debug, PartialEq, serde::Deserialize, serde::Serialize, schemars::JsonSchema)]
pub struct ParametersReference {
    pub group: Group,
    pub kind: Kind,
    pub name: String,
    pub namespace: Option<String>,
}

pub type GatewayClassConditionType = String;
pub type GatewayClassConditionReason = String;

pub type GatewayController = String;

#[derive(Clone, Debug, PartialEq, serde::Deserialize, serde::Serialize, schemars::JsonSchema)]
pub struct GatewayClassStatus {
    /// Conditions is the current status from the controller for this
    /// GatewayClass.
    ///
    /// Controllers should prefer to publish conditions using values of
    /// GatewayClassConditionType for the type of each Condition.
    pub conditions: Option<Vec<metav1::Condition>>,
}
