use super::*;

#[derive(Clone, Debug, PartialEq, serde::Deserialize, serde::Serialize, schemars::JsonSchema)]
pub struct LocalObjectReference {
    pub group: Group,
    pub kind: Kind,
    pub name: ObjectName,
}

#[derive(Clone, Debug, PartialEq, serde::Deserialize, serde::Serialize, schemars::JsonSchema)]
pub struct SecretObjectReference {
    pub group: Option<Group>,
    pub kind: Option<Kind>,
    pub name: ObjectName,
    pub namespace: Option<Namespace>,
}

#[derive(Clone, Debug, PartialEq, serde::Deserialize, serde::Serialize, schemars::JsonSchema)]
pub struct BackendObjectReference {
    pub group: Option<Group>,
    pub kind: Option<Kind>,
    pub name: ObjectName,
    pub namespace: Option<Namespace>,
    pub port: Option<PortNumber>,
}
