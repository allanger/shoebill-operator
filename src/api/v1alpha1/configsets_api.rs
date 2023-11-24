use futures::StreamExt;
use kube::api::ListParams;
use kube::runtime::controller::Action;
use kube::runtime::watcher::Config;
use kube::runtime::Controller;
use kube::{Api, Client, CustomResource};
use log::*;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use std::time::Duration;
use thiserror::Error;

/// ConfigSet is the main CRD of the shoebill-operator.
/// During the reconciliation, the controller will get the data
/// from Secrets and ConfigMaps defined in inputs, use them for
/// building new variables, that are defined in templates, and
/// put them to target Secrets or ConfigMaps
#[derive(CustomResource, Deserialize, Serialize, Clone, Debug, JsonSchema)]
#[cfg_attr(test, derive(Default))]
#[kube(
    kind = "ConfigSet",
    group = "shoebill.badhouseplants.net",
    version = "v1alpha1",
    namespaced
)]
#[kube(status = "ConfigSetStatus", shortname = "confset")]
pub struct ConfigSetSpec {
    pub targets: Vec<TargetWithName>,
    pub inputs: Vec<InputWithName>,
    pub templates: Vec<Templates>,
}

#[derive(Deserialize, Serialize, Clone, Debug, JsonSchema)]
pub struct ConfigSetStatus {
    ready: bool,
}

#[derive(Deserialize, Serialize, Clone, Debug, JsonSchema)]
pub struct TargetWithName {
    pub name: String,
    pub target: Target,
}

#[derive(Deserialize, Serialize, Clone, Debug, JsonSchema)]
pub struct Target {
    pub kind: Kinds,
    pub name: String,
}

#[derive(Deserialize, Serialize, Clone, Debug, JsonSchema)]
pub struct InputWithName {
    pub name: String,
    pub from: Input,
}

#[derive(Deserialize, Serialize, Clone, Debug, JsonSchema)]
pub enum Kinds {
    Secret,
    ConfigMap,
}

#[derive(Deserialize, Serialize, Clone, Debug, JsonSchema)]
pub struct Input {
    pub kind: Kinds,
    pub name: String,
    pub key: String,
}

#[derive(Deserialize, Serialize, Clone, Debug, JsonSchema)]
pub struct Templates {
    pub name: String,
    pub template: String,
    pub target: String,
}
