use super::*;

#[derive(Clone, Debug, serde::Deserialize, serde::Serialize, schemars::JsonSchema)]
pub struct ReferencePolicy {
    pub from: Vec<ReferencePolicyFrom>,
    pub to: Vec<ReferencePolicyFrom>,
}

#[derive(Clone, Debug, serde::Deserialize, serde::Serialize, schemars::JsonSchema)]
pub struct ReferencePolicyFrom {
    pub group: Group,
    pub kind: Kind,
    pub namespace: Namespace,
}

#[derive(Clone, Debug, serde::Deserialize, serde::Serialize, schemars::JsonSchema)]
pub struct ReferencePolicyTo {
    pub group: Group,
    pub kind: Kind,
    pub name: Option<ObjectName>,
}
