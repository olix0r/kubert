use k8s_openapi::apimachinery::pkg::apis::meta::v1 as metav1;

mod gateway;
mod gatewayclass;
mod httproute;
mod object_reference;
mod policy;
mod referencepolicy;
mod shared;
mod tcproute;
mod tlsroute;

pub use self::{
    gateway::*, gatewayclass::*, httproute::*, object_reference::*, policy::*, referencepolicy::*,
    shared::*, tcproute::*, tlsroute::*,
};
