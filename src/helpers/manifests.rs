use std::{collections::BTreeMap, default};

use k8s_openapi::{
    api::{
        apps::v1::{Deployment, DeploymentSpec},
        core::v1::{Container, EnvVar, PodSpec, PodTemplate, PodTemplateSpec, ServiceAccount},
        rbac::v1::{ClusterRole, ClusterRoleBinding, PolicyRule, Role, RoleRef, Subject},
    },
    apimachinery::pkg::apis::meta::v1::LabelSelector,
};
use kube::{core::ObjectMeta, CustomResourceExt, ResourceExt};

use crate::api::v1alpha1::configsets_api::ConfigSet;

pub fn generate_kube_manifests(namespace: String, image: String, image_tag: String) {
    print!("---\n{}", serde_yaml::to_string(&ConfigSet::crd()).unwrap());
    print!(
        "---\n{}",
        serde_yaml::to_string(&prepare_cluster_role(namespace.clone())).unwrap()
    );
    print!(
        "---\n{}",
        serde_yaml::to_string(&prepare_service_account(namespace.clone())).unwrap()
    );
    print!(
        "---\n{}",
        serde_yaml::to_string(&prepare_cluster_role_binding(namespace.clone())).unwrap()
    );

    print!(
        "---\n{}",
        serde_yaml::to_string(&prepare_deployment(
            namespace.clone(),
            image.clone(),
            image_tag.clone()
        ))
        .unwrap()
    )
}

fn prepare_cluster_role(namespace: String) -> ClusterRole {
    let rules: Vec<PolicyRule> = vec![
        PolicyRule {
            api_groups: Some(vec!["shoebill.badhouseplants.net".to_string()]),
            resources: Some(vec!["configsets".to_string()]),
            verbs: vec![
                "get".to_string(),
                "list".to_string(),
                "patch".to_string(),
                "update".to_string(),
                "watch".to_string(),
            ],
            ..Default::default()
        },
        PolicyRule {
            api_groups: Some(vec!["shoebill.badhouseplants.net".to_string()]),
            resources: Some(vec!["configsets/finalizers".to_string()]),
            verbs: vec![
                "get".to_string(),
                "list".to_string(),
                "patch".to_string(),
                "update".to_string(),
                "watch".to_string(),
                "create".to_string(),
                "delete".to_string(),
            ],
            ..Default::default()
        },
        PolicyRule {
            api_groups: Some(vec!["".to_string()]),
            resources: Some(vec!["secrets".to_string(), "configmaps".to_string()]),
            verbs: vec![
                "get".to_string(),
                "list".to_string(),
                "watch".to_string(),
                "update".to_string(),
                "create".to_string(),
                "delete".to_string(),
            ],
            ..Default::default()
        },
    ];

    ClusterRole {
        metadata: ObjectMeta {
            name: Some("shoebill-controller".to_string()),
            namespace: Some(namespace),
            ..Default::default()
        },
        rules: Some(rules),
        ..Default::default()
    }
}

fn prepare_service_account(namespace: String) -> ServiceAccount {
    ServiceAccount {
        metadata: ObjectMeta {
            name: Some("shoebill-controller".to_string()),
            namespace: Some(namespace),
            ..Default::default()
        },
        ..Default::default()
    }
}

fn prepare_cluster_role_binding(namespace: String) -> ClusterRoleBinding {
    ClusterRoleBinding {
        metadata: ObjectMeta {
            name: Some("shoebill-controller".to_string()),
            namespace: Some(namespace.clone()),
            ..Default::default()
        },
        role_ref: RoleRef {
            api_group: "rbac.authorization.k8s.io".to_string(),
            kind: "ClusterRole".to_string(),
            name: "shoebill-controller".to_string(),
        },
        subjects: Some(vec![Subject {
            kind: "ServiceAccount".to_string(),
            name: "shoebill-controller".to_string(),
            namespace: Some(namespace.clone()),
            ..Default::default()
        }]),
    }
}

fn prepare_deployment(namespace: String, image: String, image_tag: String) -> Deployment {
    let mut labels: BTreeMap<String, String> = BTreeMap::new();
    labels.insert("container".to_string(), "shoebill-controller".to_string());

    Deployment {
        metadata: ObjectMeta {
            name: Some("shoebill-controller".to_string()),
            namespace: Some(namespace.clone()),
            ..Default::default()
        },
        spec: Some(DeploymentSpec {
            replicas: Some(1),
            selector: LabelSelector {
                match_labels: Some(labels.clone()),
                ..Default::default()
            },
            template: PodTemplateSpec {
                metadata: Some(ObjectMeta {
                    labels: Some(labels.clone()),
                    ..Default::default()
                }),
                spec: Some(PodSpec {
                    automount_service_account_token: Some(true),
                    containers: vec![Container {
                        command: Some(vec!["/shoebill".to_string()]),
                        args: Some(vec!["controller".to_string()]),
                        image: Some(format!("{}:{}", image, image_tag)),
                        image_pull_policy: Some("IfNotPresent".to_string()),
                        name: "shoebill-controller".to_string(),
                        env: Some(vec![EnvVar {
                            name: "RUST_LOG".to_string(),
                            value: Some("info".to_string()),
                            ..Default::default()
                        }]),
                        ..Default::default()
                    }],
                    service_account_name: Some("shoebill-controller".to_string()),
                    ..Default::default()
                }),
            },
            ..Default::default()
        }),
        ..Default::default()
    }
}
