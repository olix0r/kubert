use super::*;
use std::collections::BTreeMap;

/// Gateway represents an instance of a service-traffic handling infrastructure
/// by binding Listeners to a set of IP addresses.
#[derive(
    Clone, Debug, kube::CustomResource, serde::Deserialize, serde::Serialize, schemars::JsonSchema,
)]
#[kube(
    group = "gateway.networking.k8s.io/v1alpha2",
    version = "v1alpha2",
    kind = "Gateway",
    status = "GatewayStatus",
    namespaced
)]
#[serde(rename_all = "camelCase")]
pub struct GatewaySpec {
    /// GatewayClassName used for this Gateway. This is the name of a
    /// GatewayClass resource.
    pub gateway_class_name: ObjectName,

    // Listeners associated with this Gateway. Listeners define logical
    /// endpoints that are bound on this Gateway's addresses.  At least one
    /// Listener MUST be specified.
    ///
    /// Each listener in a Gateway must have a unique combination of Hostname,
    /// Port, and Protocol.
    ///
    /// An implementation MAY group Listeners by Port and then collapse each
    /// group of Listeners into a single Listener if the implementation
    /// determines that the Listeners in the group are "compatible". An
    /// implementation MAY also group together and collapse compatible Listeners
    /// belonging to different Gateways.
    ///
    /// For example, an implementation might consider Listeners to be compatible
    /// with each other if all of the following conditions are met:
    ///
    /// 1. Either each Listener within the group specifies the "HTTP" Protocol or
    /// each Listener within the group specifies either the "HTTPS" or "TLS"
    /// Protocol.
    ///
    /// 2. Each Listener within the group specifies a Hostname that is unique
    /// within the group.
    ///
    /// 3. As a special case, one Listener within a group may omit Hostname, in
    /// which case this Listener matches when no other Listener matches.
    ///
    /// If the implementation does collapse compatible Listeners, the hostname
    /// provided in the incoming client request MUST be matched to a Listener to
    /// find the correct set of Routes.  The incoming hostname MUST be matched
    /// using the Hostname field for each Listener in order of most to least
    /// specific.  That is, exact matches must be processed before wildcard
    /// matches.
    ///
    /// If this field specifies multiple Listeners that have the same Port value
    /// but are not compatible, the implementation must raise a "Conflicted"
    /// condition in the Listener status.
    ///
    /// Support: Core
    pub listeners: Vec<Listener>,

    // Addresses requested for this Gateway. This is optional and behavior can
    // depend on the implementation. If a value is set in the spec and the
    // requested address is invalid or unavailable, the implementation MUST
    // indicate this in the associated entry in GatewayStatus.Addresses.
    //
    // The Addresses field represents a request for the address(es) on the
    // "outside of the Gateway", that traffic bound for this Gateway will use.
    // This could be the IP address or hostname of an external load balancer or
    // other networking infrastructure, or some other address that traffic will
    // be sent to.
    //
    // The .listener.hostname field is used to route traffic that has already
    // arrived at the Gateway to the correct in-cluster destination.
    //
    // If no Addresses are specified, the implementation MAY schedule the
    // Gateway in an implementation-specific manner, assigning an appropriate
    // set of Addresses.
    //
    // The implementation MUST bind all Listeners to every GatewayAddress that
    // it assigns to the Gateway and add a corresponding entry in
    // GatewayStatus.Addresses.
    //
    // Support: Extended
    pub addresses: Option<Vec<GatewayAddress>>,
}

/// Listener embodies the concept of a logical endpoint where a Gateway accepts
/// network connections.
#[derive(Clone, Debug, PartialEq, serde::Deserialize, serde::Serialize, schemars::JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct Listener {
    /// Name is the name of the Listener. This name MUST be unique within a
    /// Gateway.
    ///
    /// Support: Core
    pub section_name: String,

    /// Hostname specifies the virtual hostname to match for protocol types that
    /// define this concept. When unspecified, all hostnames are matched. This
    /// field is ignored for protocols that don't require hostname based
    /// matching.
    ///
    /// Implementations MUST apply Hostname matching appropriately for each of
    /// the following protocols:
    ///
    /// * TLS: The Listener Hostname MUST match the SNI.  * HTTP: The Listener
    /// Hostname MUST match the Host header of the request.  * HTTPS: The
    /// Listener Hostname SHOULD match at both the TLS and HTTP protocol layers
    /// as described above. If an implementation does not ensure that both the
    /// SNI and Host header match the Listener hostname, it MUST clearly document
    /// that.
    ///
    /// For HTTPRoute and TLSRoute resources, there is an interaction with the
    /// `spec.hostnames` array. When both listener and route specify hostnames,
    /// there MUST be an intersection between the values for a Route to be
    /// accepted. For more information, refer to the Route specific Hostnames
    /// documentation.
    ///
    /// Support: Core
    pub hostname: Option<Hostname>,

    /// Port is the network port. Multiple listeners may use the same port,
    /// subject to the Listener compatibility rules.
    pub port: PortNumber,

    /// Protocol specifies the network protocol this listener expects to receive.
    ///
    /// Support: Core
    pub protocol: ProtocolType,

    pub tls: Option<GatewayTlsConfig>,

    pub allowed_routes: Option<AllowedRoutes>,
}

/// ProtocolType defines the application protocol accepted by a Listener.
/// Implementations are not required to accept all the defined protocols.
/// If an implementation does not support a specified protocol, it
/// should raise a "Detached" condition for the affected Listener with
/// a reason of "UnsupportedProtocol".
///
/// Core ProtocolType values are listed in the table below.
///
/// Implementations can define their own protocols if a core ProtocolType does not
/// exist. Such definitions must use prefixed name, such as
/// `mycompany.com/my-custom-protocol`. Un-prefixed names are reserved for core
/// protocols. Any protocol defined by implementations will fall under custom
/// conformance.
///
/// Valid values include:
///
/// * "HTTP" - Core support
/// * "example.com/bar" - Implementation-specific support
///
/// Invalid values include:
///
/// * "example.com" - must include path if domain is used
/// * "foo.example.com" - must include path if domain is used
///
pub type ProtocolType = String;

/// GatewayTLSConfig describes a TLS configuration.
#[derive(Clone, Debug, PartialEq, serde::Deserialize, serde::Serialize, schemars::JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct GatewayTlsConfig {
    /// Mode defines the TLS behavior for the TLS session initiated by the
    /// client.  There are two possible modes:
    ///
    /// - Terminate: The TLS session between the downstream client and the
    ///   Gateway is terminated at the Gateway. This mode requires
    ///   certificateRefs to be set and contain at least one element.
    /// - Passthrough: The TLS session is NOT terminated by the Gateway. This
    ///   implies that the Gateway can't decipher the TLS stream except for the
    ///   ClientHello message of the TLS protocol. CertificateRefs field is
    ///   ignored in this mode.
    ///
    /// Support: Core
    pub mode: Option<TlsModeType>,

    /// CertificateRefs contains a series of references to Kubernetes objects
    /// that contains TLS certificates and private keys. These certificates are
    /// used to establish a TLS handshake for requests that match the hostname
    /// of the associated listener.
    ///
    /// A single CertificateRef to a Kubernetes Secret has "Core" support.
    /// Implementations MAY choose to support attaching multiple certificates to
    /// a Listener, but this behavior is implementation-specific.
    ///
    /// References to a resource in different namespace are invalid UNLESS there
    /// is a ReferencePolicy in the target namespace that allows the certificate
    /// to be attached. If a ReferencePolicy does not allow this reference, the
    /// "ResolvedRefs" condition MUST be set to False for this listener with the
    /// "InvalidCertificateRef" reason.
    ///
    /// This field is required to have at least one element when the mode is set
    /// to "Terminate" (default) and is optional otherwise.
    ///
    /// CertificateRefs can reference to standard Kubernetes resources, i.e.
    /// Secret, or implementation-specific custom resources.
    ///
    /// Support: Core - A single reference to a Kubernetes Secret of type
    /// kubernetes.io/tls
    ///
    /// Support: Implementation-specific (More than one reference or other
    /// resource types)
    pub certifcate_refs: Option<Vec<SecretObjectReference>>,

    /// Options are a list of key/value pairs to enable extended TLS
    /// configuration for each implementation. For example, configuring the
    /// minimum TLS version or supported cipher suites.
    ///
    /// A set of common keys MAY be defined by the API in the future. To avoid
    /// any ambiguity, implementation-specific definitions MUST use
    /// domain-prefixed names, such as `example.com/my-custom-option`.
    /// Un-prefixed names are reserved for key names defined by Gateway API.
    ///
    /// Support: Implementation-specific
    pub options: Option<BTreeMap<String, String>>,
}

/// TLSModeType type defines how a Gateway handles TLS sessions.
pub type TlsModeType = String;

#[derive(Clone, Debug, PartialEq, serde::Deserialize, serde::Serialize, schemars::JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct AllowedRoutes {
    /// Namespaces indicates namespaces from which Routes may be attached to
    /// this Listener. This is restricted to the namespace of this Gateway by
    /// default.
    ///
    /// Support: Core
    pub namespaces: Option<RouteNamespaces>,

    /// Kinds specifies the groups and kinds of Routes that are allowed to bind
    /// to this Gateway Listener. When unspecified or empty, the kinds of Routes
    /// selected are determined using the Listener protocol.
    ///
    /// A RouteGroupKind MUST correspond to kinds of Routes that are compatible
    /// with the application protocol specified in the Listener's Protocol
    /// field.  If an implementation does not support or recognize this resource
    /// type, it MUST set the "ResolvedRefs" condition to False for this
    /// Listener with the "InvalidRouteKinds" reason.
    ///
    /// Support: Core
    pub kinds: Option<Vec<RouteGroupKind>>,
}

/// FromNamespaces specifies namespace from which Routes may be attached to a
/// Gateway.
pub type FromNamespaces = String;

#[derive(Clone, Debug, PartialEq, serde::Deserialize, serde::Serialize, schemars::JsonSchema)]
pub struct RouteNamespaces {
    pub from: Option<FromNamespaces>,

    pub selector: metav1::LabelSelector,
}

#[derive(Clone, Debug, PartialEq, serde::Deserialize, serde::Serialize, schemars::JsonSchema)]
pub struct RouteGroupKind {
    /// Group is the group of the Route.
    pub group: Option<String>,

    /// Kind is the kind of the Route.
    pub kind: String,
}

#[derive(Clone, Debug, PartialEq, serde::Deserialize, serde::Serialize, schemars::JsonSchema)]
pub struct GatewayAddress {
    /// Type of the address.
    pub r#type: Option<AddressType>,

    /// Value of the address. The validity of the values will depend on the type
    /// and support by the controller.
    ///
    /// Examples: `1.2.3.4`, `128::1`, `my-ip-address`.
    pub value: String,
}

#[derive(Clone, Debug, PartialEq, serde::Deserialize, serde::Serialize, schemars::JsonSchema)]
pub struct GatewayStatus {
    /// Addresses lists the IP addresses that have actually been bound to the
    /// Gateway. These addresses may differ from the addresses in the Spec, e.g.
    /// if the Gateway automatically assigns an address from a reserved pool.
    pub addresses: Option<Vec<GatewayAddress>>,

    /// Conditions describe the current conditions of the Gateway.
    ///
    /// Implementations should prefer to express Gateway conditions using the
    /// `GatewayConditionType` and `GatewayConditionReason` constants so that
    /// operators and tools can converge on a common vocabulary to describe
    /// Gateway state.
    ///
    /// Known condition types are:
    ///
    /// * "Scheduled"
    /// * "Ready"
    pub conditions: Option<Vec<metav1::Condition>>,

    /// Routes is a list of routes bound to the Gateway.
    pub listeners: Option<Vec<ListenerStatus>>,
}

pub type GatewayConditionType = String;

pub type GatewayConditionReason = String;

#[derive(Clone, Debug, PartialEq, serde::Deserialize, serde::Serialize, schemars::JsonSchema)]
pub struct ListenerStatus {
    /// Name is the name of the Listener that this status corresponds to.
    pub name: SectionName,

    /// SupportedKinds is the list indicating the Kinds supported by this
    /// listener. This MUST represent the kinds an implementation supports for
    /// that Listener configuration.
    ///
    /// If kinds are specified in Spec that are not supported, they MUST NOT
    /// appear in this list and an implementation MUST set the "ResolvedRefs"
    /// condition to "False" with the "InvalidRouteKinds" reason. If both valid
    /// and invalid Route kinds are specified, the implementation MUST reference
    /// the valid Route kinds that have been specified.
    pub supported_kinds: Vec<RouteGroupKind>,

    /// AttachedRoutes represents the total number of Routes that have been
    /// successfully attached to this Listener.
    pub attached_routes: u16,

    /// Conditions describe the current condition of this listener.
    pub conditions: Vec<metav1::Condition>,
}

pub type ListenerConditionType = String;

pub type ListenerConditionReason = String;
