use reqwest::Client;
use serde::Deserialize;

#[derive(Clone, Debug, Deserialize)]
pub struct MockEmail {
    pub id: String,
    pub to: String,
    pub subject: String,
}

#[derive(Clone, Debug, Deserialize)]
pub struct DevJob {
    pub id: String,
    #[serde(rename = "type")]
    pub job_type: String,
    pub status: String,
    #[serde(default)]
    pub executions: Vec<DevJobExec>,
}

#[derive(Clone, Debug, Deserialize)]
pub struct DevJobExec {
    pub result: String,
    pub execution_time_ms: i64,
    pub failure_reason: Option<String>,
}

#[derive(Clone, Debug, Default, Deserialize)]
pub struct MigrationStatus {
    pub head: Option<String>,
    #[serde(default)]
    pub applied: Vec<String>,
    #[serde(default)]
    pub pending: Vec<String>,
}

pub async fn emails(client: &Client, api: &str) -> Vec<MockEmail> {
    get_json(client, &format!("{}/dev/emails", trim(api))).await
}

pub async fn jobs(client: &Client, api: &str) -> Vec<DevJob> {
    get_json(client, &format!("{}/dev/jobs", trim(api))).await
}

pub async fn migrations(client: &Client, api: &str) -> MigrationStatus {
    get_json(client, &format!("{}/dev/migrations", trim(api))).await
}

pub async fn retry_job(client: &Client, api: &str, id: &str) {
    let url = format!("{}/dev/jobs/{id}/retry", trim(api));
    let _ = client.post(url).send().await;
}

pub async fn delete_email(client: &Client, api: &str, id: &str) {
    let url = format!("{}/dev/emails/{id}", trim(api));
    let _ = client.delete(url).send().await;
}

pub async fn migrate_up(client: &Client, api: &str) -> Result<String, String> {
    post_step(
        client,
        &format!("{}/dev/migrations/up", trim(api)),
        "applied",
    )
    .await
}

pub async fn migrate_down(client: &Client, api: &str) -> Result<String, String> {
    post_step(
        client,
        &format!("{}/dev/migrations/down", trim(api)),
        "reverted",
    )
    .await
}

async fn post_step(client: &Client, url: &str, field: &str) -> Result<String, String> {
    let res = client.post(url).send().await.map_err(|e| e.to_string())?;
    if !res.status().is_success() {
        let status = res.status();
        let body = res.text().await.unwrap_or_default();
        return Err(if body.is_empty() {
            status.to_string()
        } else {
            body
        });
    }
    let v: serde_json::Value = res.json().await.map_err(|e| e.to_string())?;
    Ok(v.get(field)
        .and_then(|x| x.as_str())
        .unwrap_or("ok")
        .to_string())
}

async fn get_json<T: for<'de> Deserialize<'de> + Default>(client: &Client, url: &str) -> T {
    let Ok(res) = client.get(url).send().await else {
        return T::default();
    };
    res.json().await.unwrap_or_default()
}

fn trim(api: &str) -> &str {
    api.trim_end_matches('/')
}
