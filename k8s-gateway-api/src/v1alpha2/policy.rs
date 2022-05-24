use super::*;

#[derive(Clone, Debug, PartialEq, serde::Deserialize, serde::Serialize, schemars::JsonSchema)]
pub struct PolicyTargetReference {
    pub group: Group,
    pub kind: Kind,
    pub name: ObjectName,
    pub namespace: Option<Namespace>,
}
