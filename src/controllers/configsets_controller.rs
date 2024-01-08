use crate::api::v1alpha1::configsets_api::{
    ConfigSet, Input, InputWithName, TargetWithName, Templates,
};
use core::fmt;
use futures::StreamExt;
use handlebars::Handlebars;
use k8s_openapi::api::core::v1::{ConfigMap, Secret};
use k8s_openapi::apimachinery::pkg::apis::meta::v1::OwnerReference;
use k8s_openapi::{ByteString, NamespaceResourceScope};
use kube::api::{ListParams, PostParams};
use kube::core::{Object, ObjectMeta};
use kube::error::ErrorResponse;
use kube::runtime::controller::Action;
use kube::runtime::finalizer::Event as Finalizer;
use kube::runtime::watcher::Config;
use kube::runtime::{finalizer, Controller};
use kube::{Api, Client, CustomResource};
use kube_client::core::DynamicObject;
use kube_client::{Resource, ResourceExt};
use log::*;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, HashMap};
use std::str::{from_utf8, Utf8Error};
use std::sync::Arc;
use std::time::Duration;
use thiserror::Error;

static WATCHED_BY_SHU: &str = "badhouseplants.net/watched-by-shu";
static SHU_FINALIZER: &str = "badhouseplants.net/shu-cleanup";

#[derive(Error, Debug)]
pub enum Error {
    #[error("Kube Error: {0}")]
    KubeError(#[source] kube::Error),

    #[error("Finalizer Error: {0}")]
    // NB: awkward type because finalizer::Error embeds the reconciler error (which is this)
    // so boxing this error to break cycles
    FinalizerError(#[source] Box<kube::runtime::finalizer::Error<Error>>),

    #[error("IllegalConfigSet: {0}")]
    IllegalConfigSet(#[source] Box<dyn std::error::Error + Send + Sync>),
}

pub(crate) type Result<T, E = Error> = std::result::Result<T, E>;

// Context for our reconciler
#[derive(Clone)]
pub struct Context {
    /// Kubernetes client
    pub client: Client,
}

async fn reconcile(csupstream: Arc<ConfigSet>, ctx: Arc<Context>) -> Result<Action> {
    let ns = csupstream.namespace().unwrap();
    let confset: Api<ConfigSet> = Api::namespaced(ctx.client.clone(), &ns);
    finalizer(&confset, SHU_FINALIZER, csupstream.clone(), |event| async {
        info!(
            "reconciling {} - {}",
            csupstream.metadata.name.clone().unwrap(),
            csupstream.metadata.namespace.clone().unwrap()
        );
        match event {
            Finalizer::Apply(doc) => match csupstream.reconcile(ctx.clone()).await {
                Ok(res) => {
                    info!("reconciled successfully");
                    Ok(res)
                }
                Err(err) => {
                    error!("reconciliation has failed with error: {}", err);
                    Err(err)
                }
            },
            Finalizer::Cleanup(doc) => match csupstream.cleanup(ctx.clone()).await {
                Ok(res) => {
                    info!("cleaned up successfully");
                    Ok(res)
                }
                Err(err) => {
                    error!("cleanup has failed with error: {}", err);
                    Err(err)
                }
            },
        }
    })
    .await
    .map_err(|e| Error::FinalizerError(Box::new(e)))
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

fn get_secret_api(client: Client, namespace: String) -> Api<Secret> {
    Api::namespaced(client, &namespace)
}

fn get_configmap_api(client: Client, namespace: String) -> Api<ConfigMap> {
    Api::namespaced(client, &namespace)
}

async fn gather_inputs(
    client: Client,
    namespace: String,
    inputs: Vec<InputWithName>,
) -> Result<HashMap<String, String>> {
    let mut result: HashMap<String, String> = HashMap::new();
    for i in inputs {
        info!("populating data from input {}", i.name);
        match i.from.kind {
            crate::api::v1alpha1::configsets_api::Kinds::Secret => {
                let secret: String = match get_secret_api(client.clone(), namespace.clone())
                    .get(&i.from.name)
                    .await
                {
                    Ok(s) => {
                        let data = s.data.clone().unwrap();
                        let value = match data.get(i.from.key.as_str()) {
                            Some(data) => match from_utf8(&data.0) {
                                Ok(data) => data,
                                Err(err) => return Err(Error::IllegalConfigSet(Box::from(err))),
                            },
                            None => {
                                return Err(Error::IllegalConfigSet(Box::from(format!(
                                    "value is not set for the key: {}",
                                    i.from.key
                                ))))
                            }
                        };
                        value.to_string()
                    }
                    Err(err) => {
                        error!("{err}");
                        return Err(Error::KubeError(err));
                    }
                };
                result.insert(i.from.key, secret);
            }
            crate::api::v1alpha1::configsets_api::Kinds::ConfigMap => {
                let configmap: String = match get_configmap_api(client.clone(), namespace.clone())
                    .get(&i.from.name)
                    .await
                {
                    Ok(cm) => {
                        let data = cm.data.unwrap();
                        let value = match data.get(i.from.key.as_str()) {
                            Some(data) => data,
                            None => {
                                return Err(Error::IllegalConfigSet(Box::from(format!(
                                    "value is not set for the key: {}",
                                    i.from.key
                                ))))
                            }
                        };
                        value.to_string()
                    }
                    Err(err) => {
                        error!("{err}");
                        return Err(Error::KubeError(err));
                    }
                };
                result.insert(i.name, configmap);
            }
        }
    }
    Ok(result)
}

async fn gather_targets(
    client: Client,
    namespace: String,
    targets: Vec<TargetWithName>,
    owner_reference: Vec<OwnerReference>,
) -> Result<(HashMap<String, Secret>, HashMap<String, ConfigMap>)> {
    let mut target_secrets: HashMap<String, Secret> = HashMap::new();
    let mut target_configmaps: HashMap<String, ConfigMap> = HashMap::new();
    for target in targets {
        match target.target.kind {
            crate::api::v1alpha1::configsets_api::Kinds::Secret => {
                let api = get_secret_api(client.clone(), namespace.clone());
                match api.get_opt(&target.target.name).await {
                    Ok(sec_opt) => match sec_opt {
                        Some(sec) => target_secrets.insert(target.name, sec),
                        None => {
                            let empty_data: BTreeMap<String, ByteString> = BTreeMap::new();
                            let new_secret: Secret = Secret {
                                data: Some(empty_data),
                                metadata: ObjectMeta {
                                    name: Some(target.target.name),
                                    namespace: Some(namespace.clone()),
                                    owner_references: Some(owner_reference.clone()),
                                    ..Default::default()
                                },
                                ..Default::default()
                            };
                            match api.create(&PostParams::default(), &new_secret).await {
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
                let api = get_configmap_api(client.clone(), namespace.clone());
                match api.get_opt(&target.target.name).await {
                    Ok(cm_opt) => match cm_opt {
                        Some(cm) => target_configmaps.insert(target.name, cm),
                        None => {
                            let empty_data: BTreeMap<String, String> = BTreeMap::new();
                            let new_configmap: ConfigMap = ConfigMap {
                                data: Some(empty_data),
                                metadata: ObjectMeta {
                                    name: Some(target.target.name),
                                    namespace: Some(namespace.clone()),
                                    owner_references: Some(owner_reference.clone()),
                                    ..Default::default()
                                },
                                ..Default::default()
                            };
                            match api.create(&PostParams::default(), &new_configmap).await {
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
    Ok((target_secrets, target_configmaps))
}

fn build_owner_refenerce(object: ConfigSet) -> Vec<OwnerReference> {
    let owner_reference = OwnerReference {
        api_version: ConfigSet::api_version(&()).to_string(),
        kind: ConfigSet::kind(&()).to_string(),
        name: object.metadata.name.unwrap(),
        uid: object.metadata.uid.unwrap(),
        ..Default::default()
    };
    vec![owner_reference]
}

fn build_templates(
    templates: Vec<Templates>,
    target_secrets: &mut HashMap<String, Secret>,
    target_configmaps: &mut HashMap<String, ConfigMap>,
    targets: Vec<TargetWithName>,
    inputs: HashMap<String, String>,
    confset_name: String,
) -> Result<()> {
    for template in templates {
        let reg = Handlebars::new();
        info!("building template {}", template.name);
        let var = match reg.render_template(template.template.as_str(), &inputs) {
            Ok(var) => var,
            Err(err) => return Err(Error::IllegalConfigSet(Box::from(err))),
        };

        let target = match targets.iter().find(|target| target.name == template.target) {
            Some(target) => target,
            None => {
                return Err(Error::IllegalConfigSet(Box::from(format!(
                    "target not found {}",
                    template.target
                ))));
            }
        };

        match target.target.kind {
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
                let mut existing_annotations = match sec.metadata.annotations.clone() {
                    Some(ann) => ann,
                    None => BTreeMap::new(),
                };
                existing_annotations.insert(WATCHED_BY_SHU.to_string(), confset_name.clone());
                sec.metadata.annotations = Some(existing_annotations);
            }
            crate::api::v1alpha1::configsets_api::Kinds::ConfigMap => {
                let cm = target_configmaps.get_mut(&template.target).unwrap();
                let mut existing_data = match cm.clone().data {
                    Some(cm) => cm,
                    None => BTreeMap::new(),
                };
                existing_data.insert(template.name, var);
                cm.data = Some(existing_data);
                let mut existing_annotations = match cm.metadata.annotations.clone() {
                    Some(ann) => ann,
                    None => BTreeMap::new(),
                };
                existing_annotations.insert(WATCHED_BY_SHU.to_string(), confset_name.clone());
                cm.metadata.annotations = Some(existing_annotations);
            }
        }
    }
    Ok(())
}

fn cleanup_templates(
    templates: Vec<Templates>,
    target_secrets: &mut HashMap<String, Secret>,
    target_configmaps: &mut HashMap<String, ConfigMap>,
    targets: Vec<TargetWithName>,
) -> Result<()> {
    for template in templates {
        info!("cleaning template {}", template.name);
        let target = match targets.iter().find(|target| target.name == template.target) {
            Some(target) => target,
            None => {
                return Err(Error::IllegalConfigSet(Box::from(format!(
                    "target not found {}",
                    template.target
                ))));
            }
        };

        match target.target.kind {
            crate::api::v1alpha1::configsets_api::Kinds::Secret => {
                let sec = target_secrets.get_mut(&template.target).unwrap();
                if let Some(mut existing_data) = sec.clone().data {
                    existing_data.remove(&template.name);
                    sec.data = Some(existing_data)
                }
                if let Some(mut existing_annotations) = sec.metadata.clone().annotations {
                    existing_annotations.remove(WATCHED_BY_SHU);
                    sec.metadata.annotations = Some(existing_annotations);
                }
            }
            crate::api::v1alpha1::configsets_api::Kinds::ConfigMap => {
                let cm = target_configmaps.get_mut(&template.target).unwrap();
                if let Some(mut existing_data) = cm.clone().data {
                    existing_data.remove(&template.name);
                    cm.data = Some(existing_data);
                }
                if let Some(mut existing_annotations) = cm.metadata.clone().annotations {
                    existing_annotations.remove(WATCHED_BY_SHU);
                    cm.metadata.annotations = Some(existing_annotations);
                }
            }
        }
    }
    Ok(())
}

impl ConfigSet {
    // Reconcile (for non-finalizer related changes)
    async fn reconcile(&self, ctx: Arc<Context>) -> Result<Action> {
        /*
         * First we need to get inputs and write them to the map
         * Then use them to build new values with templates
         * And then write those values to targets
         */

        let inputs: HashMap<String, String> = gather_inputs(
            ctx.client.clone(),
            self.metadata.namespace.clone().unwrap(),
            self.spec.inputs.clone(),
        )
        .await?;

        let owner_reference = build_owner_refenerce(self.clone());

        let (mut target_secrets, mut target_configmaps) = gather_targets(
            ctx.client.clone(),
            self.metadata.namespace.clone().unwrap(),
            self.spec.targets.clone(),
            owner_reference,
        )
        .await?;

        build_templates(
            self.spec.templates.clone(),
            &mut target_secrets,
            &mut target_configmaps,
            self.spec.targets.clone(),
            inputs.clone(),
            self.metadata.name.clone().unwrap(),
        )?;

        for (_, value) in target_secrets {
            let secrets =
                get_secret_api(ctx.client.clone(), self.metadata.namespace.clone().unwrap());
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
            let configmaps =
                get_configmap_api(ctx.client.clone(), self.metadata.namespace.clone().unwrap());
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
        let inputs: HashMap<String, String> = gather_inputs(
            ctx.client.clone(),
            self.metadata.namespace.clone().unwrap(),
            self.spec.inputs.clone(),
        )
        .await?;

        let owner_reference = build_owner_refenerce(self.clone());

        let (mut target_secrets, mut target_configmaps) = gather_targets(
            ctx.client.clone(),
            self.metadata.namespace.clone().unwrap(),
            self.spec.targets.clone(),
            owner_reference,
        )
        .await?;
        cleanup_templates(
            self.spec.templates.clone(),
            &mut target_secrets,
            &mut target_configmaps,
            self.spec.targets.clone(),
        )?;

        for (_, value) in target_secrets {
            let secrets =
                get_secret_api(ctx.client.clone(), self.metadata.namespace.clone().unwrap());
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
            let configmaps =
                get_configmap_api(ctx.client.clone(), self.metadata.namespace.clone().unwrap());
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
}
