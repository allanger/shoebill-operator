use crate::api::v1alpha1::configsets_api::ConfigSet;
use futures::StreamExt;
use handlebars::Handlebars;
use k8s_openapi::api::core::v1::{ConfigMap, Secret};
use k8s_openapi::ByteString;
use kube::api::{ListParams, PostParams};
use kube::core::{Object, ObjectMeta};
use kube::error::ErrorResponse;
use kube::runtime::controller::Action;
use kube::runtime::watcher::Config;
use kube::runtime::Controller;
use kube::{Api, Client, CustomResource};
use log::*;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, HashMap};
use std::str::{from_utf8, Utf8Error};
use std::sync::Arc;
use std::time::Duration;
use thiserror::Error;
#[derive(Error, Debug)]
pub enum Error {
    #[error("SerializationError: {0}")]
    SerializationError(#[source] serde_json::Error),

    #[error("Kube Error: {0}")]
    KubeError(#[source] kube::Error),

    #[error("Finalizer Error: {0}")]
    // NB: awkward type because finalizer::Error embeds the reconciler error (which is this)
    // so boxing this error to break cycles
    FinalizerError(#[source] Box<kube::runtime::finalizer::Error<Error>>),

    #[error("IllegalDocument")]
    IllegalDocument,
}
pub type Result<T, E = Error> = std::result::Result<T, E>;
impl Error {
    pub fn metric_label(&self) -> String {
        format!("{self:?}").to_lowercase()
    }
}

// Context for our reconciler
#[derive(Clone)]
pub struct Context {
    /// Kubernetes client
    pub client: Client,
}

async fn reconcile(csupstream: Arc<ConfigSet>, ctx: Arc<Context>) -> Result<Action> {
    let cs = csupstream.clone();
    info!(
        "reconciling {} - {}",
        cs.metadata.name.clone().unwrap(),
        cs.metadata.namespace.clone().unwrap()
    );
    match cs.metadata.deletion_timestamp {
        Some(_) => return cs.cleanup(ctx).await,
        None => return cs.reconcile(ctx).await,
    }
}

/// Initialize the controller and shared state (given the crd is installed)
pub async fn setup() {
    info!("starting the configset controller");
    let client = Client::try_default()
        .await
        .expect("failed to create kube Client");
    let docs = Api::<ConfigSet>::all(client.clone());
    if let Err(e) = docs.list(&ListParams::default().limit(1)).await {
        error!("{}", e);
        std::process::exit(1);
    }
    let ctx = Arc::new(Context { client });
    Controller::new(docs, Config::default().any_semantic())
        .shutdown_on_signal()
        .run(reconcile, error_policy, ctx)
        .filter_map(|x| async move { std::result::Result::ok(x) })
        .for_each(|_| futures::future::ready(()))
        .await;
}

fn error_policy(doc: Arc<ConfigSet>, error: &Error, ctx: Arc<Context>) -> Action {
    Action::requeue(Duration::from_secs(5 * 60))
}

impl ConfigSet {
    // Reconcile (for non-finalizer related changes)
    async fn reconcile(&self, ctx: Arc<Context>) -> Result<Action> {
        /*
         * First we need to get inputs and write them to the map
         * Then use them to build new values with templates
         * And then write those values to targets
         */
        let mut inputs: HashMap<String, String> = HashMap::new();
        for input in self.spec.inputs.clone() {
            info!("populating data from input {}", input.name);
            match input.from.kind {
                crate::api::v1alpha1::configsets_api::Kinds::Secret => {
                    let secrets: Api<Secret> = Api::namespaced(
                        ctx.client.clone(),
                        self.metadata.namespace.clone().unwrap().as_str(),
                    );
                    let secret: String = match secrets.get(&input.from.name).await {
                        Ok(s) => from_utf8(&s.data.clone().unwrap()[input.from.key.as_str()].0)
                            .unwrap()
                            .to_string(),
                        Err(err) => {
                            error!("{err}");
                            return Err(Error::KubeError(err));
                        }
                    };
                    inputs.insert(input.from.key, secret);
                }
                crate::api::v1alpha1::configsets_api::Kinds::ConfigMap => {
                    let configmaps: Api<ConfigMap> = Api::namespaced(
                        ctx.client.clone(),
                        self.metadata.namespace.clone().unwrap().as_str(),
                    );
                    let configmap: String = match configmaps.get(&input.from.name).await {
                        Ok(cm) => {
                            let data = &cm.data.unwrap()[input.from.key.as_str()];
                            data.to_string()
                        }
                        Err(err) => {
                            error!("{err}");
                            return Err(Error::KubeError(err));
                        }
                    };
                    inputs.insert(input.name, configmap);
                }
            }
        }

        let mut target_secrets: HashMap<String, Secret> = HashMap::new();
        let mut target_configmaps: HashMap<String, ConfigMap> = HashMap::new();

        for target in self.spec.targets.clone() {
            match target.target.kind {
                crate::api::v1alpha1::configsets_api::Kinds::Secret => {
                    let secrets: Api<Secret> = Api::namespaced(
                        ctx.client.clone(),
                        self.metadata.namespace.clone().unwrap().as_str(),
                    );
                    match secrets.get_opt(&target.target.name).await {
                        Ok(sec_opt) => match sec_opt {
                            Some(sec) => target_secrets.insert(target.name, sec),
                            None => {
                                let empty_data: BTreeMap<String, ByteString> = BTreeMap::new();
                                let new_secret: Secret = Secret {
                                    data: Some(empty_data),
                                    metadata: ObjectMeta {
                                        name: Some(target.target.name),
                                        namespace: self.metadata.namespace.clone(),
                                        ..Default::default()
                                    },
                                    ..Default::default()
                                };
                                match secrets.create(&PostParams::default(), &new_secret).await {
                                    Ok(sec) => target_secrets.insert(target.name, sec),
                                    Err(err) => {
                                        error!("{err}");
                                        return Err(Error::KubeError(err));
                                    }
                                }
                            }
                        },
                        Err(err) => {
                            error!("{err}");
                            return Err(Error::KubeError(err));
                        }
                    };
                }
                crate::api::v1alpha1::configsets_api::Kinds::ConfigMap => {
                    let configmaps: Api<ConfigMap> = Api::namespaced(
                        ctx.client.clone(),
                        self.metadata.namespace.clone().unwrap().as_str(),
                    );
                    match configmaps.get_opt(&target.target.name).await {
                        Ok(cm_opt) => match cm_opt {
                            Some(cm) => target_configmaps.insert(target.name, cm),
                            None => {
                                let empty_data: BTreeMap<String, String> = BTreeMap::new();
                                let new_configmap: ConfigMap = ConfigMap {
                                    data: Some(empty_data),
                                    metadata: ObjectMeta {
                                        name: Some(target.target.name),
                                        namespace: self.metadata.namespace.clone(),
                                        ..Default::default()
                                    },
                                    ..Default::default()
                                };
                                match configmaps
                                    .create(&PostParams::default(), &new_configmap)
                                    .await
                                {
                                    Ok(cm) => target_configmaps.insert(target.name, cm),
                                    Err(err) => {
                                        error!("{err}");
                                        return Err(Error::KubeError(err));
                                    }
                                }
                            }
                        },
                        Err(err) => {
                            error!("{err}");
                            return Err(Error::KubeError(err));
                        }
                    };
                }
            }
        }

        let mut templates: HashMap<String, String> = HashMap::new();
        for template in self.spec.templates.clone() {
            let reg = Handlebars::new();
            info!("building template {}", template.name);
            let var = reg
                .render_template(template.template.as_str(), &inputs)
                .unwrap();
            info!("result is {}", var);
            match self
                .spec
                .targets
                .iter()
                .find(|target| target.name == template.target)
                .unwrap()
                .target
                .kind
            {
                crate::api::v1alpha1::configsets_api::Kinds::Secret => {
                    let sec = target_secrets.get_mut(&template.target).unwrap();
                    let mut byte_var: ByteString = ByteString::default();
                    byte_var.0 = var.as_bytes().to_vec();

                    let mut existing_data = match sec.clone().data {
                        Some(sec) => sec,
                        None => BTreeMap::new(),
                    };
                    existing_data.insert(template.name, byte_var);
                    sec.data = Some(existing_data);
                }
                crate::api::v1alpha1::configsets_api::Kinds::ConfigMap => {
                    let cm = target_configmaps.get_mut(&template.target).unwrap();
                    let mut existing_data = match cm.clone().data {
                        Some(cm) => cm,
                        None => BTreeMap::new(),
                    };
                    existing_data.insert(template.name, var);
                    cm.data = Some(existing_data);
                }
            }
        }

        for (_, value) in target_secrets {
            let secrets: Api<Secret> = Api::namespaced(
                ctx.client.clone(),
                self.metadata.namespace.clone().unwrap().as_str(),
            );
            match secrets
                .replace(
                    value.metadata.name.clone().unwrap().as_str(),
                    &PostParams::default(),
                    &value,
                )
                .await
            {
                Ok(sec) => {
                    info!("secret {} is updated", sec.metadata.name.unwrap());
                }
                Err(err) => {
                    error!("{}", err);
                    return Err(Error::KubeError(err));
                }
            };
        }
        for (_, value) in target_configmaps {
            let configmaps: Api<ConfigMap> = Api::namespaced(
                ctx.client.clone(),
                self.metadata.namespace.clone().unwrap().as_str(),
            );
            match configmaps
                .replace(
                    value.metadata.name.clone().unwrap().as_str(),
                    &PostParams::default(),
                    &value,
                )
                .await
            {
                Ok(sec) => {
                    info!("secret {} is updated", sec.metadata.name.unwrap());
                }
                Err(err) => {
                    error!("{}", err);
                    return Err(Error::KubeError(err));
                }
            };
        }
        Ok::<Action, Error>(Action::await_change())
    }

    // Finalizer cleanup (the object was deleted, ensure nothing is orphaned)
    async fn cleanup(&self, ctx: Arc<Context>) -> Result<Action> {
        info!("removing, not installing");
        Ok::<Action, Error>(Action::await_change())
    }
}
