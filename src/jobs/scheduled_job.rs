/// Scheduled job configuration
#[derive(Debug, Clone)]
pub struct ScheduledJob {
    pub name: String,
    pub job_name: &'static str,
    pub arguments: serde_json::Value,
    pub cron_expression: String,
}
