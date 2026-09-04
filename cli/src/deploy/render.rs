//! Typed Kubernetes manifests for the two Erno topologies.
//!
//! Selectors stay `app: {release}-{component}` so migrating off Helm does not
//! recreate Deployments. Recommended labels sit next to them for prune/status.

use base64::Engine;
use serde::Deserialize;
use serde_json::{json, Value};

use super::config::{
    origin, AppSecrets, EnvConfig, MonitoringSecrets, API_PORT, COLLECTOR_PORT, HTTP_PORT,
};

#[derive(Debug, Clone)]
pub struct Manifest {
    pub kind: String,
    pub name: String,
    /// None for cluster-scoped objects (ClusterIssuer). Used by tests and by
    /// extra-manifest loading; apply uses the env's namespace flag instead.
    #[allow(dead_code)]
    pub namespace: Option<String>,
    pub prune: bool,
    pub deployment: Option<String>,
    pub doc: Value,
}

impl Manifest {
    fn namespaced(
        kind: &str,
        name: impl Into<String>,
        ns: &str,
        component: &str,
        ctx: &LabelCtx,
        doc: Value,
    ) -> Self {
        let name = name.into();
        let deployment = (kind == "Deployment").then(|| name.clone());
        Self {
            kind: kind.into(),
            name,
            namespace: Some(ns.into()),
            prune: true,
            deployment,
            doc: with_meta(doc, ctx, component, Some(ns)),
        }
    }

    fn cluster(kind: &str, name: impl Into<String>, doc: Value) -> Self {
        Self {
            kind: kind.into(),
            name: name.into(),
            namespace: None,
            prune: false,
            deployment: None,
            doc,
        }
    }
}

struct LabelCtx<'a> {
    release: &'a str,
    version: &'a str,
}

fn with_meta(mut doc: Value, ctx: &LabelCtx, component: &str, ns: Option<&str>) -> Value {
    let labels = json!({
        "app.kubernetes.io/managed-by": "erno",
        "app.kubernetes.io/instance": ctx.release,
        "app.kubernetes.io/version": ctx.version,
        "app.kubernetes.io/component": component,
    });
    let meta = doc
        .as_object_mut()
        .expect("manifest object")
        .entry("metadata")
        .or_insert_with(|| json!({}));
    if let Some(ns) = ns {
        meta["namespace"] = json!(ns);
    }
    match meta.get_mut("labels") {
        Some(existing) => {
            if let Some(map) = existing.as_object_mut() {
                if let Some(add) = labels.as_object() {
                    for (k, v) in add {
                        map.entry(k.clone()).or_insert(v.clone());
                    }
                }
            }
        }
        None => meta["labels"] = labels,
    }
    doc
}

pub fn encode_yaml(manifests: &[Manifest]) -> Result<String, String> {
    let mut out = String::new();
    for m in manifests {
        let doc = serde_yaml::to_string(&m.doc).map_err(|e| e.to_string())?;
        let doc = doc.trim_start_matches("---\n");
        if !out.is_empty() {
            out.push_str("---\n");
        }
        out.push_str(doc);
        if !out.ends_with('\n') {
            out.push('\n');
        }
    }
    Ok(out)
}

pub struct AppPlan<'a> {
    pub release: &'a str,
    pub github_repo: &'a str,
    pub version: &'a str,
    pub env: &'a EnvConfig,
    pub secrets: &'a AppSecrets,
    pub include_www: bool,
}

pub struct MonitoringPlan<'a> {
    pub release: &'a str,
    pub github_repo: &'a str,
    pub version: &'a str,
    pub env: &'a EnvConfig,
    pub secrets: &'a MonitoringSecrets,
}

pub fn render_app(plan: &AppPlan<'_>) -> Vec<Manifest> {
    let ctx = LabelCtx {
        release: plan.release,
        version: plan.version,
    };
    let ns = plan.env.namespace.as_str();
    let mut out = vec![
        registry_secret(&ctx, ns, &plan.secrets.registry),
        api_deployment(plan, &ctx, ns),
        service(&ctx, ns, "api", API_PORT, API_PORT),
        static_deployment(
            plan.release,
            plan.github_repo,
            plan.version,
            &ctx,
            ns,
            "app",
            plan.env.workloads.app_replicas,
            &[env(
                "API_URL",
                origin(plan.env.tls.enabled, &plan.env.hosts.api),
            )],
        ),
        service(&ctx, ns, "app", HTTP_PORT, HTTP_PORT),
    ];
    if plan.include_www && plan.env.workloads.www {
        out.push(static_deployment(
            plan.release,
            plan.github_repo,
            plan.version,
            &ctx,
            ns,
            "www",
            plan.env.workloads.www_replicas,
            &[env(
                "APP_URL",
                origin(plan.env.tls.enabled, &plan.env.hosts.app),
            )],
        ));
        out.push(service(&ctx, ns, "www", HTTP_PORT, HTTP_PORT));
    }
    if plan.env.workloads.admin {
        out.push(admin_deployment(plan, &ctx, ns));
        out.push(service(&ctx, ns, "admin", HTTP_PORT, HTTP_PORT));
    }
    out.push(app_ingress(plan, &ctx, ns));
    if plan.env.tls.enabled {
        out.push(cluster_issuer(&plan.env.tls.email));
    }
    out
}

pub fn render_monitoring(plan: &MonitoringPlan<'_>) -> Vec<Manifest> {
    let ctx = LabelCtx {
        release: plan.release,
        version: plan.version,
    };
    let ns = plan.env.namespace.as_str();
    let mut out = vec![
        registry_secret(&ctx, ns, &plan.secrets.registry),
        collector_deployment(plan, &ctx, ns),
        service(&ctx, ns, "collector", COLLECTOR_PORT, COLLECTOR_PORT),
        console_deployment(plan, &ctx, ns),
        service(&ctx, ns, "console", HTTP_PORT, HTTP_PORT),
    ];
    out.push(monitoring_ingress(plan, &ctx, ns));
    if plan.env.tls.enabled {
        out.push(cluster_issuer(&plan.env.tls.email));
    }
    out
}

fn registry_secret(ctx: &LabelCtx, ns: &str, registry: &super::config::Registry) -> Manifest {
    let docker = json!({
        "auths": {
            &registry.server: {
                "username": registry.username,
                "password": registry.password,
            }
        }
    });
    let encoded = base64::engine::general_purpose::STANDARD
        .encode(serde_json::to_vec(&docker).expect("dockerconfigjson"));
    let name = format!("{}-registry", ctx.release);
    Manifest::namespaced(
        "Secret",
        name.clone(),
        ns,
        "registry",
        ctx,
        json!({
            "apiVersion": "v1",
            "kind": "Secret",
            "metadata": { "name": name },
            "type": "kubernetes.io/dockerconfigjson",
            "data": { ".dockerconfigjson": encoded },
        }),
    )
}

fn api_deployment(plan: &AppPlan<'_>, ctx: &LabelCtx, ns: &str) -> Manifest {
    let name = format!("{}-api", plan.release);
    let image = image(plan.github_repo, "api", plan.version);
    let mut env_vars = vec![
        env("DATABASE_URL", &plan.secrets.api.database_url),
        env("APP__SERVER__PORT", API_PORT),
        env(
            "APP__API_URL",
            origin(plan.env.tls.enabled, &plan.env.hosts.api),
        ),
        env("APP__AUTH__SECRET", &plan.secrets.api.jwt_secret),
    ];
    if !plan.secrets.api.admin_password_hash.is_empty() {
        env_vars.push(env(
            "APP__ADMIN__PASSWORD_HASH",
            &plan.secrets.api.admin_password_hash,
        ));
        env_vars.push(env("APP__ADMIN__USERNAME", "admin"));
    }
    if !plan.secrets.api.metrics_auth_token.is_empty() {
        env_vars.push(env(
            "APP__METRICS__AUTH_TOKEN",
            &plan.secrets.api.metrics_auth_token,
        ));
    }
    env_vars.push(env("APP__TRACING__LOG_LEVEL", &plan.secrets.api.log_level));
    if !plan.env.monitoring_url.is_empty() {
        env_vars.push(env(
            "APP__ERROR_REPORTING__COLLECTOR_URL",
            plan.env.monitoring_url.trim_end_matches('/'),
        ));
        env_vars.push(env(
            "APP__ERROR_REPORTING__INGEST_TOKEN",
            &plan.secrets.api.ingest_token,
        ));
        // Straight to the collector's OTLP receiver: it authenticates the
        // bearer itself, so there is no nginx auth_request in the path and
        // traces and logs keep flowing while the console is down.
        let otlp = format!("{}/api/otlp", plan.env.monitoring_url.trim_end_matches('/'));
        env_vars.push(env("APP__TRACING__OTEL__ENDPOINT", &otlp));
        env_vars.push(env("APP__TRACING__OTEL__LOGS_ENDPOINT", &otlp));
        env_vars.push(env(
            "APP__TRACING__OTEL__TOKEN",
            &plan.secrets.api.ingest_token,
        ));
        env_vars.push(env("APP__TRACING__OTEL__SAMPLE_RATIO", "0.1"));
        env_vars.push(env("APP__TRACING__OTEL__LOG_LEVEL", "warn"));
    }
    env_vars.extend([
        env("APP__EMAIL__TYPE", "smtp"),
        env("APP__EMAIL__HOST", &plan.secrets.api.smtp_host),
        env("APP__EMAIL__PORT", plan.secrets.api.smtp_port),
        env("APP__EMAIL__USERNAME", &plan.secrets.api.smtp_username),
        env("APP__EMAIL__PASSWORD", &plan.secrets.api.smtp_password),
        env("APP__EMAIL__FROM", &plan.secrets.api.smtp_from),
    ]);
    for (k, v) in &plan.secrets.env {
        env_vars.push(env(k, v));
    }
    Manifest::namespaced(
        "Deployment",
        name.clone(),
        ns,
        "api",
        ctx,
        workload(
            &name,
            "api",
            plan.release,
            plan.version,
            plan.env.workloads.api_replicas,
            &image,
            API_PORT,
            &env_vars,
            WorkloadOpts {
                grace: true,
                pre_stop: true,
                readiness: Some(("/health", API_PORT, 5, 10)),
                liveness: Some(("/health", API_PORT, 15, 30)),
                volumes: vec![],
                args: None,
            },
        ),
    )
}

fn admin_deployment(plan: &AppPlan<'_>, ctx: &LabelCtx, ns: &str) -> Manifest {
    let name = format!("{}-admin", plan.release);
    let image = image(plan.github_repo, "admin", plan.version);
    Manifest::namespaced(
        "Deployment",
        name.clone(),
        ns,
        "admin",
        ctx,
        workload(
            &name,
            "admin",
            plan.release,
            plan.version,
            plan.env.workloads.admin_replicas,
            &image,
            HTTP_PORT,
            &[
                env("API_HOST", format!("{}-api", plan.release)),
                env("API_PORT", API_PORT),
            ],
            WorkloadOpts {
                grace: false,
                pre_stop: false,
                readiness: None,
                liveness: None,
                volumes: vec![],
                args: None,
            },
        ),
    )
}

#[allow(clippy::too_many_arguments)]
fn static_deployment(
    release: &str,
    github_repo: &str,
    version: &str,
    ctx: &LabelCtx,
    ns: &str,
    component: &str,
    replicas: i32,
    env_vars: &[Value],
) -> Manifest {
    let name = format!("{release}-{component}");
    let image = image(github_repo, component, version);
    Manifest::namespaced(
        "Deployment",
        name.clone(),
        ns,
        component,
        ctx,
        workload(
            &name,
            component,
            release,
            version,
            replicas,
            &image,
            HTTP_PORT,
            env_vars,
            WorkloadOpts {
                grace: false,
                pre_stop: false,
                readiness: Some(("/", HTTP_PORT, 5, 10)),
                liveness: None,
                volumes: vec![],
                args: None,
            },
        ),
    )
}

fn collector_deployment(plan: &MonitoringPlan<'_>, ctx: &LabelCtx, ns: &str) -> Manifest {
    let name = format!("{}-collector", plan.release);
    let image = image(plan.github_repo, "monitoring", plan.version);
    let c = &plan.secrets.collector;
    let mut env_vars = vec![
        env("DATABASE_URL", &c.database_url),
        env("APP__SERVER__PORT", COLLECTOR_PORT),
        env(
            "APP__API_URL",
            origin(plan.env.tls.enabled, &plan.env.hosts.monitoring),
        ),
        env("APP__AUTH__SECRET", &c.jwt_secret),
        env("APP__TRACING__LOG_LEVEL", &c.log_level),
    ];
    if !c.admin_password_hash.is_empty() {
        env_vars.push(env("APP__ADMIN__USERNAME", &c.admin_username));
        env_vars.push(env("APP__ADMIN__PASSWORD_HASH", &c.admin_password_hash));
    }
    if !c.metrics_auth_token.is_empty() {
        env_vars.push(env("APP__METRICS__AUTH_TOKEN", &c.metrics_auth_token));
    }
    env_vars.push(env("APP__COLLECTOR__ENABLED", "true"));
    if !plan.secrets.error_reporting.ingest_token.is_empty() {
        env_vars.push(env(
            "APP__ERROR_REPORTING__INGEST_TOKEN",
            &plan.secrets.error_reporting.ingest_token,
        ));
        env_vars.push(env(
            "APP__ERROR_REPORTING__COLLECTOR_URL",
            format!("http://{}-collector:{COLLECTOR_PORT}", plan.release),
        ));
    }
    if !c.alerts_recipient.is_empty() {
        env_vars.push(env("APP__COLLECTOR__ALERTS__ENABLED", "true"));
        env_vars.push(env(
            "APP__COLLECTOR__ALERTS__RECIPIENT",
            &c.alerts_recipient,
        ));
    }
    env_vars.push(env("APP__COLLECTOR__STATUS__ENABLED", "true"));
    env_vars.push(env("APP__COLLECTOR__STATUS__NAME", &c.status_name));
    env_vars.push(env("APP__COLLECTOR__STATUS__OUTPUT_PATH", "/app/status"));
    if !plan.secrets.error_reporting.ingest_token.is_empty() {
        // Self-telemetry through the collector's own OTLP receiver, in-pod:
        // one path to debug, and the seed project's token authenticates it.
        env_vars.push(env(
            "APP__TRACING__OTEL__ENDPOINT",
            format!("http://127.0.0.1:{COLLECTOR_PORT}/api/otlp"),
        ));
        env_vars.push(env(
            "APP__TRACING__OTEL__TOKEN",
            &plan.secrets.error_reporting.ingest_token,
        ));
        env_vars.push(env("APP__TRACING__OTEL__SAMPLE_RATIO", "0.1"));
        env_vars.push(env("APP__TRACING__OTEL__LOG_LEVEL", "warn"));
    }
    env_vars.extend([
        env("APP__EMAIL__TYPE", "smtp"),
        env("APP__EMAIL__HOST", &c.smtp_host),
        env("APP__EMAIL__PORT", c.smtp_port),
        env("APP__EMAIL__USERNAME", &c.smtp_username),
        env("APP__EMAIL__PASSWORD", &c.smtp_password),
        env("APP__EMAIL__FROM", &c.smtp_from),
    ]);
    for (k, v) in &plan.secrets.env {
        env_vars.push(env(k, v));
    }
    Manifest::namespaced(
        "Deployment",
        name.clone(),
        ns,
        "collector",
        ctx,
        workload(
            &name,
            "collector",
            plan.release,
            plan.version,
            plan.env.workloads.collector_replicas,
            &image,
            COLLECTOR_PORT,
            &env_vars,
            WorkloadOpts {
                grace: true,
                pre_stop: true,
                readiness: Some(("/readiness", COLLECTOR_PORT, 5, 10)),
                liveness: Some(("/liveness", COLLECTOR_PORT, 15, 30)),
                volumes: vec![Volume {
                    name: "status",
                    empty_dir: true,
                    config_map: None,
                    pvc: None,
                    mount: "/app/status",
                    sub_path: None,
                }],
                args: None,
            },
        ),
    )
}

fn console_deployment(plan: &MonitoringPlan<'_>, ctx: &LabelCtx, ns: &str) -> Manifest {
    let name = format!("{}-console", plan.release);
    let image = image(plan.github_repo, "monitoring-ui", plan.version);
    Manifest::namespaced(
        "Deployment",
        name.clone(),
        ns,
        "console",
        ctx,
        workload(
            &name,
            "console",
            plan.release,
            plan.version,
            plan.env.workloads.console_replicas,
            &image,
            HTTP_PORT,
            &[
                env("COLLECTOR_HOST", format!("{}-collector", plan.release)),
                env("COLLECTOR_PORT", COLLECTOR_PORT),
            ],
            WorkloadOpts {
                grace: false,
                pre_stop: false,
                readiness: None,
                liveness: None,
                volumes: vec![],
                args: None,
            },
        ),
    )
}

fn service(ctx: &LabelCtx, ns: &str, component: &str, port: i32, target: i32) -> Manifest {
    let name = format!("{}-{component}", ctx.release);
    Manifest::namespaced(
        "Service",
        name.clone(),
        ns,
        component,
        ctx,
        json!({
            "apiVersion": "v1",
            "kind": "Service",
            "metadata": { "name": name },
            "spec": {
                "selector": selector(ctx.release, component),
                "ports": [{ "port": port, "targetPort": target }],
            },
        }),
    )
}

fn app_ingress(plan: &AppPlan<'_>, ctx: &LabelCtx, ns: &str) -> Manifest {
    let name = plan.release.to_string();
    let mut rules = vec![
        ingress_rule(
            &plan.env.hosts.api,
            format!("{}-api", plan.release),
            API_PORT,
        ),
        ingress_rule(
            &plan.env.hosts.app,
            format!("{}-app", plan.release),
            HTTP_PORT,
        ),
    ];
    if plan.include_www && plan.env.workloads.www {
        rules.push(ingress_rule(
            &plan.env.hosts.www,
            format!("{}-www", plan.release),
            HTTP_PORT,
        ));
    }
    if plan.env.workloads.admin {
        rules.push(ingress_rule(
            &plan.env.hosts.admin,
            format!("{}-admin", plan.release),
            HTTP_PORT,
        ));
    }
    let mut hosts: Vec<&str> = vec![&plan.env.hosts.api, &plan.env.hosts.app];
    if plan.include_www && plan.env.workloads.www {
        hosts.push(&plan.env.hosts.www);
    }
    if plan.env.workloads.admin {
        hosts.push(&plan.env.hosts.admin);
    }
    Manifest::namespaced(
        "Ingress",
        name.clone(),
        ns,
        "ingress",
        ctx,
        ingress_doc(
            &name,
            &plan.env.tls,
            plan.release,
            &hosts,
            rules,
            None,
            None,
        ),
    )
}

fn monitoring_ingress(plan: &MonitoringPlan<'_>, ctx: &LabelCtx, ns: &str) -> Manifest {
    let name = plan.release.to_string();
    let host = plan.env.hosts.monitoring.as_str();
    let rps = if plan.env.ingress.rate_limit_rps == 0 {
        20
    } else {
        plan.env.ingress.rate_limit_rps
    };
    Manifest::namespaced(
        "Ingress",
        name.clone(),
        ns,
        "ingress",
        ctx,
        ingress_doc(
            &name,
            &plan.env.tls,
            plan.release,
            &[host],
            vec![ingress_rule(
                host,
                format!("{}-console", plan.release),
                HTTP_PORT,
            )],
            Some("2m"),
            Some(rps),
        ),
    )
}

fn ingress_doc(
    name: &str,
    tls: &super::config::Tls,
    release: &str,
    hosts: &[&str],
    rules: Vec<Value>,
    body_size: Option<&str>,
    rate_limit_rps: Option<u32>,
) -> Value {
    let mut annotations = serde_json::Map::new();
    annotations.insert("kubernetes.io/ingress.class".into(), json!("nginx"));
    if let Some(size) = body_size {
        annotations.insert(
            "nginx.ingress.kubernetes.io/proxy-body-size".into(),
            json!(size),
        );
    }
    if let Some(rps) = rate_limit_rps {
        annotations.insert(
            "nginx.ingress.kubernetes.io/limit-rps".into(),
            json!(rps.to_string()),
        );
    }
    if tls.enabled {
        annotations.insert("cert-manager.io/cluster-issuer".into(), json!(tls.issuer));
        annotations.insert(
            "nginx.ingress.kubernetes.io/ssl-redirect".into(),
            json!("true"),
        );
    }
    let mut spec = json!({ "rules": rules });
    if tls.enabled {
        spec["tls"] = json!([{
            "hosts": hosts,
            "secretName": format!("{release}-tls"),
        }]);
    }
    json!({
        "apiVersion": "networking.k8s.io/v1",
        "kind": "Ingress",
        "metadata": {
            "name": name,
            "annotations": annotations,
        },
        "spec": spec,
    })
}

fn ingress_rule(host: &str, service: String, port: i32) -> Value {
    json!({
        "host": host,
        "http": {
            "paths": [{
                "path": "/",
                "pathType": "Prefix",
                "backend": {
                    "service": {
                        "name": service,
                        "port": { "number": port },
                    }
                }
            }]
        }
    })
}

fn cluster_issuer(email: &str) -> Manifest {
    Manifest::cluster(
        "ClusterIssuer",
        "letsencrypt",
        json!({
            "apiVersion": "cert-manager.io/v1",
            "kind": "ClusterIssuer",
            "metadata": { "name": "letsencrypt" },
            "spec": {
                "acme": {
                    "server": "https://acme-v02.api.letsencrypt.org/directory",
                    "email": email,
                    "privateKeySecretRef": { "name": "letsencrypt-account-key" },
                    "solvers": [{
                        "http01": { "ingress": { "class": "nginx" } }
                    }],
                }
            }
        }),
    )
}

struct Volume<'a> {
    name: &'a str,
    empty_dir: bool,
    config_map: Option<&'a str>,
    pvc: Option<&'a str>,
    mount: &'a str,
    /// Mount one key as a file rather than the whole ConfigMap as a directory.
    ///
    /// Needed wherever something else mounts *inside* that directory: kubelet
    /// remounts a ConfigMap volume wholesale, and a nested mount under it
    /// disappears when it does.
    sub_path: Option<&'a str>,
}

struct WorkloadOpts<'a> {
    grace: bool,
    pre_stop: bool,
    readiness: Option<(&'a str, i32, i32, i32)>,
    liveness: Option<(&'a str, i32, i32, i32)>,
    volumes: Vec<Volume<'a>>,
    args: Option<Vec<String>>,
}

#[allow(clippy::too_many_arguments)]
fn workload(
    name: &str,
    component: &str,
    release: &str,
    version: &str,
    replicas: i32,
    image: &str,
    port: i32,
    env_vars: &[Value],
    opts: WorkloadOpts<'_>,
) -> Value {
    let mut container = json!({
        "name": component,
        "image": image,
        "imagePullPolicy": "IfNotPresent",
        "ports": [{ "containerPort": port }],
    });
    if !env_vars.is_empty() {
        container["env"] = Value::Array(env_vars.to_vec());
    }
    if let Some(args) = opts.args {
        container["args"] = json!(args);
    }
    if opts.pre_stop {
        container["lifecycle"] = json!({
            "preStop": { "exec": { "command": ["sleep", "5"] } }
        });
    }
    if let Some((path, p, initial, period)) = opts.readiness {
        container["readinessProbe"] = probe(path, p, initial, period);
    }
    if let Some((path, p, initial, period)) = opts.liveness {
        container["livenessProbe"] = probe(path, p, initial, period);
    }
    if !opts.volumes.is_empty() {
        container["volumeMounts"] = Value::Array(
            opts.volumes
                .iter()
                .map(|v| match v.sub_path {
                    Some(sub) => {
                        json!({ "name": v.name, "mountPath": v.mount, "subPath": sub })
                    }
                    None => json!({ "name": v.name, "mountPath": v.mount }),
                })
                .collect(),
        );
    }
    let mut pod_spec = json!({
        "imagePullSecrets": [{ "name": format!("{release}-registry") }],
        "containers": [container],
    });
    if opts.grace {
        pod_spec["terminationGracePeriodSeconds"] = json!(30);
    }
    if !opts.volumes.is_empty() {
        pod_spec["volumes"] = Value::Array(
            opts.volumes
                .iter()
                .map(|v| {
                    if v.empty_dir {
                        json!({ "name": v.name, "emptyDir": {} })
                    } else if let Some(cm) = v.config_map {
                        json!({ "name": v.name, "configMap": { "name": cm } })
                    } else if let Some(pvc) = v.pvc {
                        json!({ "name": v.name, "persistentVolumeClaim": { "claimName": pvc } })
                    } else {
                        json!({ "name": v.name })
                    }
                })
                .collect(),
        );
    }
    json!({
        "apiVersion": "apps/v1",
        "kind": "Deployment",
        "metadata": { "name": name },
        "spec": {
            "replicas": replicas,
            "selector": { "matchLabels": selector(release, component) },
            "template": {
                "metadata": {
                    "labels": {
                        "app": format!("{release}-{component}"),
                        "app.kubernetes.io/managed-by": "erno",
                        "app.kubernetes.io/instance": release,
                        "app.kubernetes.io/version": version,
                        "app.kubernetes.io/component": component,
                    }
                },
                "spec": pod_spec,
            }
        }
    })
}

fn probe(path: &str, port: i32, initial: i32, period: i32) -> Value {
    json!({
        "httpGet": { "path": path, "port": port },
        "initialDelaySeconds": initial,
        "periodSeconds": period,
    })
}

fn selector(release: &str, component: &str) -> Value {
    json!({ "app": format!("{release}-{component}") })
}

fn env(name: &str, value: impl ToString) -> Value {
    json!({ "name": name, "value": value.to_string() })
}

fn image(github_repo: &str, name: &str, version: &str) -> String {
    format!("ghcr.io/{github_repo}/{name}:{version}")
}

/// `deploy/extra/*.yaml`: interpolate `{{release}}` / `{{version}}` /
/// `{{namespace}}` / `{{env.NAME}}` and stamp instance labels so prune owns
/// them. ClusterIssuer is left unlabeled. Unknown `{{env.NAME}}` keys become
/// empty, so a tree can reference a secret the operator has not set yet.
pub fn load_extra(
    dir: &std::path::Path,
    release: &str,
    version: &str,
    namespace: &str,
    extra_env: &std::collections::BTreeMap<String, String>,
) -> Result<Vec<Manifest>, String> {
    if !dir.exists() {
        return Ok(Vec::new());
    }
    let mut files: Vec<_> = std::fs::read_dir(dir)
        .map_err(|e| format!("could not read {}: {e}", dir.display()))?
        .filter_map(|e| e.ok())
        .map(|e| e.path())
        .filter(|p| {
            p.extension()
                .and_then(|e| e.to_str())
                .is_some_and(|e| e == "yaml" || e == "yml")
        })
        .collect();
    files.sort();
    let mut out = Vec::new();
    for path in files {
        let raw = std::fs::read_to_string(&path)
            .map_err(|e| format!("could not read {}: {e}", path.display()))?;
        let raw = interpolate_extra(&raw, release, version, namespace, extra_env);
        for doc in serde_yaml::Deserializer::from_str(&raw) {
            let value: serde_yaml::Value = serde_yaml::Value::deserialize(doc)
                .map_err(|e| format!("{}: {e}", path.display()))?;
            if value.is_null() {
                continue;
            }
            let json_doc: Value =
                serde_json::to_value(&value).map_err(|e| format!("{}: {e}", path.display()))?;
            out.push(manifest_from_extra(json_doc, release, version, namespace)?);
        }
    }
    Ok(out)
}

fn interpolate_extra(
    raw: &str,
    release: &str,
    version: &str,
    namespace: &str,
    extra_env: &std::collections::BTreeMap<String, String>,
) -> String {
    let mut raw = raw
        .replace("{{release}}", release)
        .replace("{{version}}", version)
        .replace("{{namespace}}", namespace);
    for (k, v) in extra_env {
        raw = raw.replace(&format!("{{{{env.{k}}}}}"), v);
    }
    replace_unknown_env_placeholders(&raw)
}

fn replace_unknown_env_placeholders(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut rest = s;
    while let Some(start) = rest.find("{{env.") {
        out.push_str(&rest[..start]);
        let after = &rest[start + 6..];
        match after.find("}}") {
            Some(end) => rest = &after[end + 2..],
            None => {
                out.push_str("{{env.");
                rest = after;
                break;
            }
        }
    }
    out.push_str(rest);
    out
}

fn manifest_from_extra(
    mut doc: Value,
    release: &str,
    version: &str,
    namespace: &str,
) -> Result<Manifest, String> {
    let kind = doc
        .get("kind")
        .and_then(|v| v.as_str())
        .ok_or_else(|| "extra manifest is missing kind".to_string())?
        .to_string();
    let name = doc
        .get("metadata")
        .and_then(|m| m.get("name"))
        .and_then(|v| v.as_str())
        .ok_or_else(|| format!("extra {kind} is missing metadata.name"))?
        .to_string();
    if kind == "ClusterIssuer" {
        return Ok(Manifest::cluster(&kind, name, doc));
    }
    let component = doc
        .pointer("/metadata/labels/app.kubernetes.io/component")
        .and_then(|v| v.as_str())
        .unwrap_or("extra")
        .to_string();
    let ctx = LabelCtx { release, version };
    doc = with_meta(doc, &ctx, &component, Some(namespace));
    Ok(Manifest::namespaced(
        &kind, name, namespace, &component, &ctx, doc,
    ))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::deploy::config::{
        parse_app_secrets, parse_deploy_file, parse_monitoring_secrets, EnvConfig,
    };

    fn app_env() -> (String, EnvConfig, AppSecrets) {
        let file = parse_deploy_file(
            r#"
github_repo = "acme/acme"
[production]
kubernetes_context = "prod"
monitoring_url = "https://monitoring.example.com"
[production.hosts]
api = "api.example.com"
app = "app.example.com"
www = "example.com"
admin = "admin.example.com"
"#,
        )
        .unwrap();
        let env = file.envs["production"].clone();
        let secrets = parse_app_secrets(
            r#"
registry:
  server: ghcr.io
  username: user
  password: pass
api:
  database_url: postgres://u:p@db/acme
  jwt_secret: jwt
  admin_password_hash: $argon2id$hash
  metrics_auth_token: metrics
  smtp_host: smtp.example.com
  smtp_username: mailer
  smtp_password: pw
  smtp_from: erno@example.com
  ingest_token: ingest
"#,
        )
        .unwrap();
        (file.github_repo, env, secrets)
    }

    fn mon_env() -> (String, EnvConfig, MonitoringSecrets) {
        let file = parse_deploy_file(
            r#"
github_repo = "acme/acme"
[production]
kubernetes_context = "mon"
[production.hosts]
monitoring = "monitoring.example.com"
"#,
        )
        .unwrap();
        let env = file.envs["production"].clone();
        let secrets = parse_monitoring_secrets(
            r#"
registry:
  server: ghcr.io
  username: user
  password: pass
collector:
  database_url: postgres://u:p@db/mon
  jwt_secret: jwt
  admin_password_hash: $argon2id$hash
  metrics_auth_token: cmetrics
  alerts_recipient: ops@example.com
  status_name: Acme
  smtp_from: mon@example.com
api:
  metrics_auth_token: ametrics
"#,
        )
        .unwrap();
        (file.github_repo, env, secrets)
    }

    fn kinds(ms: &[Manifest]) -> Vec<(&str, &str)> {
        ms.iter()
            .map(|m| (m.kind.as_str(), m.name.as_str()))
            .collect()
    }

    #[test]
    fn app_default_topology() {
        let (repo, env, secrets) = app_env();
        let plan = AppPlan {
            release: "acme",
            github_repo: &repo,
            version: "v1.2.3",
            env: &env,
            secrets: &secrets,
            include_www: true,
        };
        let ms = render_app(&plan);
        assert_eq!(
            kinds(&ms),
            vec![
                ("Secret", "acme-registry"),
                ("Deployment", "acme-api"),
                ("Service", "acme-api"),
                ("Deployment", "acme-app"),
                ("Service", "acme-app"),
                ("Deployment", "acme-www"),
                ("Service", "acme-www"),
                ("Deployment", "acme-admin"),
                ("Service", "acme-admin"),
                ("Ingress", "acme"),
                ("ClusterIssuer", "letsencrypt"),
            ]
        );
        let issuer = ms.iter().find(|m| m.kind == "ClusterIssuer").unwrap();
        assert!(!issuer.prune);
        assert!(issuer.namespace.is_none());
        assert!(issuer.doc.pointer("/metadata/labels").is_none());

        let yaml = encode_yaml(&ms).unwrap();
        assert!(yaml.contains("ghcr.io/acme/acme/api:v1.2.3"));
        assert!(yaml.contains("ghcr.io/acme/acme/app:v1.2.3"));
        assert!(yaml.contains("ghcr.io/acme/acme/www:v1.2.3"));
        assert!(yaml.contains("ghcr.io/acme/acme/admin:v1.2.3"));
        assert!(yaml.contains("APP__ERROR_REPORTING__COLLECTOR_URL"));
        assert!(yaml.contains("https://monitoring.example.com"));
        assert!(yaml.contains("APP__ERROR_REPORTING__INGEST_TOKEN"));
        assert!(yaml.contains("APP__TRACING__OTEL__ENDPOINT"));
        assert!(yaml.contains("https://monitoring.example.com/api/otlp"));
        assert!(yaml.contains("APP__TRACING__OTEL__TOKEN"));
        assert!(yaml.contains("app.kubernetes.io/managed-by: erno"));
        // Selectors stay on the Helm label so a migrate does not recreate pods.
        assert!(yaml.contains("app: acme-api"));
        assert!(yaml.contains("terminationGracePeriodSeconds: 30"));
        assert!(yaml.contains("preStop"));
        assert!(yaml.contains("/health"));
        // ClusterIssuer must not be in the prune set.
        assert!(ms
            .iter()
            .filter(|m| !m.prune)
            .all(|m| m.kind == "ClusterIssuer"));
    }

    #[test]
    fn disabling_admin_and_www_drops_them_from_ingress() {
        let (repo, mut env, secrets) = app_env();
        env.workloads.admin = false;
        env.workloads.www = false;
        let plan = AppPlan {
            release: "acme",
            github_repo: &repo,
            version: "v1",
            env: &env,
            secrets: &secrets,
            include_www: false,
        };
        let ms = render_app(&plan);
        let names: Vec<_> = ms.iter().map(|m| m.name.as_str()).collect();
        assert!(!names.iter().any(|n| n.contains("admin")));
        assert!(!names.iter().any(|n| n.contains("www")));
        let yaml = encode_yaml(&ms).unwrap();
        assert!(!yaml.contains("admin.example.com"));
        assert!(!yaml.contains("host: example.com\n"));
        assert!(yaml.contains("api.example.com"));
    }

    #[test]
    fn tls_off_omits_issuer_and_tls_block() {
        let (repo, mut env, secrets) = app_env();
        env.tls.enabled = false;
        let plan = AppPlan {
            release: "acme",
            github_repo: &repo,
            version: "v1",
            env: &env,
            secrets: &secrets,
            include_www: true,
        };
        let ms = render_app(&plan);
        assert!(ms.iter().all(|m| m.kind != "ClusterIssuer"));
        let yaml = encode_yaml(&ms).unwrap();
        assert!(!yaml.contains("tls:"));
        assert!(!yaml.contains("cluster-issuer"));
        assert!(yaml.contains("http://api.example.com"));
    }

    #[test]
    fn empty_monitoring_url_skips_error_reporting_env() {
        let (repo, mut env, secrets) = app_env();
        env.monitoring_url.clear();
        let plan = AppPlan {
            release: "acme",
            github_repo: &repo,
            version: "v1",
            env: &env,
            secrets: &secrets,
            include_www: false,
        };
        let yaml = encode_yaml(&render_app(&plan)).unwrap();
        assert!(!yaml.contains("ERROR_REPORTING"));
        assert!(!yaml.contains("TRACING__OTEL"));
    }

    #[test]
    fn monitoring_topology_is_monitoring_api_and_console() {
        let (repo, env, secrets) = mon_env();
        let plan = MonitoringPlan {
            release: "acme-monitoring",
            github_repo: &repo,
            version: "v0.1.0",
            env: &env,
            secrets: &secrets,
        };
        let ms = render_monitoring(&plan);
        assert_eq!(
            kinds(&ms),
            vec![
                ("Secret", "acme-monitoring-registry"),
                ("Deployment", "acme-monitoring-collector"),
                ("Service", "acme-monitoring-collector"),
                ("Deployment", "acme-monitoring-console"),
                ("Service", "acme-monitoring-console"),
                ("Ingress", "acme-monitoring"),
                ("ClusterIssuer", "letsencrypt"),
            ]
        );
        let yaml = encode_yaml(&ms).unwrap();
        assert!(yaml.contains("ghcr.io/acme/acme/monitoring:v0.1.0"));
        assert!(yaml.contains("ghcr.io/acme/acme/monitoring-ui:v0.1.0"));
        assert!(yaml.contains("/readiness"));
        assert!(yaml.contains("/liveness"));
        assert!(yaml.contains("emptyDir"));
        assert!(!yaml.contains("APP__COLLECTOR__SERVER_TOKEN"));
        assert!(!yaml.contains("APP__COLLECTOR__BROWSER_TOKEN"));
        assert!(!yaml.contains("APP__ERROR_REPORTING__INGEST_TOKEN"));
        assert!(!yaml.contains("APP__ERROR_REPORTING__COLLECTOR_URL"));
        assert!(yaml.contains("APP__COLLECTOR__STATUS__OUTPUT_PATH"));
        assert!(yaml.contains("/app/status"));
        assert!(!yaml.contains("/app/status/status.json"));
        assert!(!yaml.contains("APP__COLLECTOR__OTLP__ENABLED"));
        // Without the self-report secret there is no self-telemetry env
        // either: a URL without a token would push and 401.
        assert!(!yaml.contains("APP__TRACING__OTEL__ENDPOINT"));
        assert!(yaml.contains("limit-rps"));
        assert!(yaml.contains("proxy-body-size"));
    }

    #[test]
    fn monitoring_chart_injects_self_report_env_only_when_the_secret_is_set() {
        let (repo, env, mut secrets) = mon_env();
        secrets.error_reporting.ingest_token = "erns_secret".to_string();
        let plan = MonitoringPlan {
            release: "acme-monitoring",
            github_repo: &repo,
            version: "v0.1.0",
            env: &env,
            secrets: &secrets,
        };
        let yaml = encode_yaml(&render_monitoring(&plan)).unwrap();
        assert!(yaml.contains("APP__ERROR_REPORTING__INGEST_TOKEN"));
        assert!(yaml.contains("erns_secret"));
        assert!(yaml.contains("APP__ERROR_REPORTING__COLLECTOR_URL"));
        assert!(yaml.contains("http://acme-monitoring-collector:3001"));
        // Self-telemetry rides the same secret: in-pod OTLP with that token.
        assert!(yaml.contains("APP__TRACING__OTEL__ENDPOINT"));
        assert!(yaml.contains("http://127.0.0.1:3001/api/otlp"));
        assert!(yaml.contains("APP__TRACING__OTEL__TOKEN"));
        assert!(!yaml.contains("APP__COLLECTOR__SERVER_TOKEN"));
        assert!(!yaml.contains("APP__COLLECTOR__BROWSER_TOKEN"));
    }

    #[test]
    fn extra_yaml_gets_instance_labels() {
        let dir = std::env::temp_dir().join(format!("erno-extra-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(
            dir.join("cm.yaml"),
            "apiVersion: v1\nkind: ConfigMap\nmetadata:\n  name: {{release}}-extra\ndata:\n  v: {{version}}\n",
        )
        .unwrap();
        let extra = load_extra(&dir, "acme", "v9", "default", &Default::default()).unwrap();
        let _ = std::fs::remove_dir_all(&dir);
        assert_eq!(extra.len(), 1);
        assert_eq!(extra[0].name, "acme-extra");
        assert_eq!(extra[0].namespace.as_deref(), Some("default"));
        assert_eq!(
            extra[0].doc["metadata"]["labels"]["app.kubernetes.io/instance"],
            json!("acme")
        );
        assert_eq!(extra[0].doc.pointer("/data/v"), Some(&json!("v9")));
        assert!(extra[0].prune);
    }

    #[test]
    fn extra_yaml_interpolates_env_and_blanks_unknown_keys() {
        let dir = std::env::temp_dir().join(format!("erno-extra-env-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(
            dir.join("cm.yaml"),
            "apiVersion: v1\nkind: ConfigMap\nmetadata:\n  name: {{release}}-extra\ndata:\n  set: {{env.FOO}}\n  missing: '{{env.UNSET}}'\n",
        )
        .unwrap();
        let mut extra_env = std::collections::BTreeMap::new();
        extra_env.insert("FOO".into(), "bar".into());
        let extra = load_extra(&dir, "acme", "v9", "default", &extra_env).unwrap();
        let _ = std::fs::remove_dir_all(&dir);
        assert_eq!(extra[0].doc.pointer("/data/set"), Some(&json!("bar")));
        assert_eq!(extra[0].doc.pointer("/data/missing"), Some(&json!("")));
    }

    #[test]
    fn extra_env_reaches_the_api_container() {
        let (repo, env, mut secrets) = app_env();
        secrets
            .env
            .insert("APP__CUSTOM".into(), "from-secrets".into());
        let plan = AppPlan {
            release: "acme",
            github_repo: &repo,
            version: "v1",
            env: &env,
            secrets: &secrets,
            include_www: false,
        };
        let yaml = encode_yaml(&render_app(&plan)).unwrap();
        assert!(yaml.contains("APP__CUSTOM"));
        assert!(yaml.contains("from-secrets"));
    }

    #[test]
    fn extra_env_reaches_the_monitoring_api_container() {
        let (repo, env, mut secrets) = mon_env();
        secrets
            .env
            .insert("APP__CUSTOM".into(), "from-secrets".into());
        let plan = MonitoringPlan {
            release: "acme-monitoring",
            github_repo: &repo,
            version: "v0.1.0",
            env: &env,
            secrets: &secrets,
        };
        let yaml = encode_yaml(&render_monitoring(&plan)).unwrap();
        assert!(yaml.contains("APP__CUSTOM"));
        assert!(yaml.contains("from-secrets"));
    }
}
