//! Initial collector schema: one organisation, many projects.
//!
//! Docs: docs/src/content/docs/monitoring/error-reporting.md
//!
//! Pre-1.0 squash. Every existing collector table carries `project_id NOT NULL`
//! from the start so ingest cannot land a row with no owner.

use sea_orm_migration::{
    prelude::*,
    schema::{
        big_integer, boolean, double, integer, integer_null, json_binary, json_binary_null, string,
        string_null, text, text_null, timestamp, timestamp_null, uuid, uuid_null,
    },
};

#[derive(DeriveMigrationName)]
pub struct Migration;

#[async_trait::async_trait]
impl MigrationTrait for Migration {
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        create_project(manager).await?;
        create_error_issue(manager).await?;
        create_error_event(manager).await?;
        create_release(manager).await?;
        create_app_health(manager).await?;
        create_uptime(manager).await?;
        create_status_page(manager).await?;
        create_alert_rule(manager).await
    }

    async fn down(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager
            .drop_table(Table::drop().table(AlertRule::Table).to_owned())
            .await?;
        manager
            .drop_table(Table::drop().table(StatusIncidentUpdate::Table).to_owned())
            .await?;
        manager
            .drop_table(Table::drop().table(StatusIncident::Table).to_owned())
            .await?;
        manager
            .drop_table(Table::drop().table(StatusComponent::Table).to_owned())
            .await?;
        manager
            .drop_table(Table::drop().table(UptimeResult::Table).to_owned())
            .await?;
        manager
            .drop_table(Table::drop().table(UptimeCheck::Table).to_owned())
            .await?;
        manager
            .drop_table(Table::drop().table(AppHealth::Table).to_owned())
            .await?;
        manager
            .drop_table(Table::drop().table(Release::Table).to_owned())
            .await?;
        manager
            .drop_table(Table::drop().table(ErrorEvent::Table).to_owned())
            .await?;
        manager
            .drop_table(Table::drop().table(ErrorIssue::Table).to_owned())
            .await?;
        manager
            .drop_table(Table::drop().table(Project::Table).to_owned())
            .await
    }
}

async fn create_project(manager: &SchemaManager<'_>) -> Result<(), DbErr> {
    manager
        .create_table(
            Table::create()
                .table(Project::Table)
                .if_not_exists()
                .col(
                    uuid(Project::Id)
                        .primary_key()
                        .default(Expr::cust("gen_random_uuid()")),
                )
                .col(string(Project::Slug).not_null())
                .col(string(Project::Name).not_null())
                .col(string(Project::ServerTokenHash).not_null())
                .col(string(Project::BrowserTokenHash).not_null())
                .col(
                    json_binary(Project::CorsOrigins)
                        .not_null()
                        .default(Expr::cust("'[]'::jsonb")),
                )
                .col(text(Project::ScrapeTarget).not_null().default(""))
                .col(string(Project::ScrapeScheme).not_null().default("https"))
                .col(text(Project::ScrapeMetricsToken).not_null().default(""))
                .col(ColumnDef::new(Project::EventRetentionDays).big_integer())
                .col(ColumnDef::new(Project::IssueRetentionDays).big_integer())
                .col(ColumnDef::new(Project::MaxEventsPerIssue).big_integer())
                .col(boolean(Project::StatusEnabled).not_null().default(false))
                .col(string(Project::StatusName).not_null().default(""))
                .col(
                    timestamp(Project::CreatedAt)
                        .not_null()
                        .default(Expr::cust("CURRENT_TIMESTAMP")),
                )
                .check(Expr::cust("server_token_hash <> ''"))
                .check(Expr::cust("browser_token_hash <> ''"))
                .to_owned(),
        )
        .await?;

    manager
        .create_index(
            Index::create()
                .name("uq_project_slug")
                .table(Project::Table)
                .col(Project::Slug)
                .unique()
                .to_owned(),
        )
        .await?;

    manager
        .create_index(
            Index::create()
                .name("uq_project_server_token_hash")
                .table(Project::Table)
                .col(Project::ServerTokenHash)
                .unique()
                .to_owned(),
        )
        .await?;

    manager
        .create_index(
            Index::create()
                .name("uq_project_browser_token_hash")
                .table(Project::Table)
                .col(Project::BrowserTokenHash)
                .unique()
                .to_owned(),
        )
        .await
}

async fn create_error_issue(manager: &SchemaManager<'_>) -> Result<(), DbErr> {
    manager
        .create_table(
            Table::create()
                .table(ErrorIssue::Table)
                .if_not_exists()
                .col(
                    uuid(ErrorIssue::Id)
                        .primary_key()
                        .default(Expr::cust("gen_random_uuid()")),
                )
                .col(uuid(ErrorIssue::ProjectId).not_null())
                .col(string(ErrorIssue::Fingerprint).not_null())
                .col(string(ErrorIssue::Source).not_null())
                .col(string(ErrorIssue::ErrorType).not_null())
                .col(string(ErrorIssue::Title).not_null())
                .col(string_null(ErrorIssue::Culprit))
                .col(string(ErrorIssue::Level).not_null().default("error"))
                .col(string(ErrorIssue::Status).not_null().default("unresolved"))
                .col(big_integer(ErrorIssue::TimesSeen).not_null().default(0))
                .col(timestamp(ErrorIssue::FirstSeen).not_null())
                .col(timestamp(ErrorIssue::LastSeen).not_null())
                .col(string_null(ErrorIssue::FirstRelease))
                .col(string_null(ErrorIssue::LastRelease))
                .col(string_null(ErrorIssue::Environment))
                .col(timestamp_null(ErrorIssue::ResolvedAt))
                .col(timestamp_null(ErrorIssue::AlertSentAt))
                .col(
                    timestamp(ErrorIssue::CreatedAt)
                        .not_null()
                        .default(Expr::cust("CURRENT_TIMESTAMP")),
                )
                .foreign_key(
                    ForeignKey::create()
                        .name("fk_error_issue_project_id")
                        .from(ErrorIssue::Table, ErrorIssue::ProjectId)
                        .to(Project::Table, Project::Id)
                        .on_delete(ForeignKeyAction::Cascade)
                        .on_update(ForeignKeyAction::Cascade),
                )
                .to_owned(),
        )
        .await?;

    manager
        .create_index(
            Index::create()
                .name("uq-error_issue-project_fingerprint")
                .table(ErrorIssue::Table)
                .col(ErrorIssue::ProjectId)
                .col(ErrorIssue::Fingerprint)
                .unique()
                .to_owned(),
        )
        .await?;

    manager
        .create_index(
            Index::create()
                .name("idx-error_issue-project_status_last_seen")
                .table(ErrorIssue::Table)
                .col(ErrorIssue::ProjectId)
                .col(ErrorIssue::Status)
                .col(ErrorIssue::LastSeen)
                .to_owned(),
        )
        .await?;

    manager
        .create_index(
            Index::create()
                .name("idx-error_issue-project_source_last_seen")
                .table(ErrorIssue::Table)
                .col(ErrorIssue::ProjectId)
                .col(ErrorIssue::Source)
                .col(ErrorIssue::LastSeen)
                .to_owned(),
        )
        .await?;

    manager
        .create_index(
            Index::create()
                .name("idx-error_issue-last_seen")
                .table(ErrorIssue::Table)
                .col(ErrorIssue::LastSeen)
                .to_owned(),
        )
        .await
}

async fn create_error_event(manager: &SchemaManager<'_>) -> Result<(), DbErr> {
    manager
        .create_table(
            Table::create()
                .table(ErrorEvent::Table)
                .if_not_exists()
                .col(
                    uuid(ErrorEvent::Id)
                        .primary_key()
                        .default(Expr::cust("gen_random_uuid()")),
                )
                .col(uuid(ErrorEvent::ProjectId).not_null())
                .col(uuid(ErrorEvent::IssueId).not_null())
                .col(string(ErrorEvent::Source).not_null())
                .col(string(ErrorEvent::Level).not_null())
                .col(string(ErrorEvent::ErrorType).not_null())
                .col(text(ErrorEvent::Message).not_null())
                .col(text_null(ErrorEvent::Stack))
                .col(json_binary_null(ErrorEvent::Frames))
                .col(json_binary(ErrorEvent::Context).not_null())
                .col(string_null(ErrorEvent::Release))
                .col(string_null(ErrorEvent::Environment))
                .col(uuid_null(ErrorEvent::UserId))
                .col(string_null(ErrorEvent::UserEmail))
                .col(string_null(ErrorEvent::ClientIp))
                .col(timestamp(ErrorEvent::CreatedAt).not_null())
                .foreign_key(
                    ForeignKey::create()
                        .name("fk_error_event_project_id")
                        .from(ErrorEvent::Table, ErrorEvent::ProjectId)
                        .to(Project::Table, Project::Id)
                        .on_delete(ForeignKeyAction::Cascade)
                        .on_update(ForeignKeyAction::Cascade),
                )
                .foreign_key(
                    ForeignKey::create()
                        .name("fk_error_event_issue_id")
                        .from(ErrorEvent::Table, ErrorEvent::IssueId)
                        .to(ErrorIssue::Table, ErrorIssue::Id)
                        .on_delete(ForeignKeyAction::Cascade)
                        .on_update(ForeignKeyAction::Cascade),
                )
                .to_owned(),
        )
        .await?;

    manager
        .create_index(
            Index::create()
                .name("idx-error_event-issue_id_created_at")
                .table(ErrorEvent::Table)
                .col(ErrorEvent::IssueId)
                .col(ErrorEvent::CreatedAt)
                .to_owned(),
        )
        .await?;

    manager
        .create_index(
            Index::create()
                .name("idx-error_event-created_at")
                .table(ErrorEvent::Table)
                .col(ErrorEvent::CreatedAt)
                .to_owned(),
        )
        .await?;

    manager
        .create_index(
            Index::create()
                .name("idx-error_event-user_id")
                .table(ErrorEvent::Table)
                .col(ErrorEvent::UserId)
                .to_owned(),
        )
        .await?;

    manager
        .create_index(
            Index::create()
                .name("idx-error_event-project_id")
                .table(ErrorEvent::Table)
                .col(ErrorEvent::ProjectId)
                .to_owned(),
        )
        .await
}

async fn create_release(manager: &SchemaManager<'_>) -> Result<(), DbErr> {
    manager
        .create_table(
            Table::create()
                .table(Release::Table)
                .if_not_exists()
                .col(
                    uuid(Release::Id)
                        .primary_key()
                        .default(Expr::cust("gen_random_uuid()")),
                )
                .col(uuid(Release::ProjectId).not_null())
                .col(string(Release::Version).not_null())
                .col(string(Release::Environment).not_null())
                .col(string_null(Release::CommitSha))
                .col(string_null(Release::Source))
                .col(timestamp(Release::DeployedAt).not_null())
                .col(
                    timestamp(Release::CreatedAt)
                        .not_null()
                        .default(Expr::cust("CURRENT_TIMESTAMP")),
                )
                .foreign_key(
                    ForeignKey::create()
                        .name("fk_release_project_id")
                        .from(Release::Table, Release::ProjectId)
                        .to(Project::Table, Project::Id)
                        .on_delete(ForeignKeyAction::Cascade)
                        .on_update(ForeignKeyAction::Cascade),
                )
                .to_owned(),
        )
        .await?;

    manager
        .create_index(
            Index::create()
                .name("uq-release-project_version_environment")
                .table(Release::Table)
                .col(Release::ProjectId)
                .col(Release::Version)
                .col(Release::Environment)
                .unique()
                .to_owned(),
        )
        .await?;

    manager
        .create_index(
            Index::create()
                .name("idx-release-deployed_at")
                .table(Release::Table)
                .col(Release::DeployedAt)
                .to_owned(),
        )
        .await
}

async fn create_app_health(manager: &SchemaManager<'_>) -> Result<(), DbErr> {
    manager
        .create_table(
            Table::create()
                .table(AppHealth::Table)
                .if_not_exists()
                .col(
                    uuid(AppHealth::Id)
                        .primary_key()
                        .default(Expr::cust("gen_random_uuid()")),
                )
                .col(uuid(AppHealth::ProjectId).not_null())
                .col(string(AppHealth::Instance).not_null())
                .col(string(AppHealth::Environment).not_null())
                .col(string_null(AppHealth::Release))
                .col(timestamp(AppHealth::ReportedAt).not_null())
                .col(json_binary(AppHealth::Payload).not_null())
                .foreign_key(
                    ForeignKey::create()
                        .name("fk_app_health_project_id")
                        .from(AppHealth::Table, AppHealth::ProjectId)
                        .to(Project::Table, Project::Id)
                        .on_delete(ForeignKeyAction::Cascade)
                        .on_update(ForeignKeyAction::Cascade),
                )
                .to_owned(),
        )
        .await?;

    manager
        .create_index(
            Index::create()
                .name("uq-app_health-project_instance")
                .table(AppHealth::Table)
                .col(AppHealth::ProjectId)
                .col(AppHealth::Instance)
                .unique()
                .to_owned(),
        )
        .await
}

async fn create_uptime(manager: &SchemaManager<'_>) -> Result<(), DbErr> {
    manager
        .create_table(
            Table::create()
                .table(UptimeCheck::Table)
                .if_not_exists()
                .col(
                    uuid(UptimeCheck::Id)
                        .primary_key()
                        .default(Expr::cust("gen_random_uuid()")),
                )
                .col(uuid(UptimeCheck::ProjectId).not_null())
                .col(string(UptimeCheck::Name).not_null())
                .col(string(UptimeCheck::Url).not_null())
                .col(string(UptimeCheck::Method).not_null().default("GET"))
                .col(integer(UptimeCheck::ExpectedStatus).not_null().default(200))
                .col(integer(UptimeCheck::TimeoutMs).not_null().default(10_000))
                .col(integer(UptimeCheck::IntervalSeconds).not_null().default(60))
                .col(boolean(UptimeCheck::Enabled).not_null().default(true))
                .col(text_null(UptimeCheck::AssertBodyContains))
                .col(integer(UptimeCheck::FailureThreshold).not_null().default(2))
                .col(
                    string(UptimeCheck::CurrentState)
                        .not_null()
                        .default("unknown"),
                )
                .col(
                    integer(UptimeCheck::ConsecutiveFailures)
                        .not_null()
                        .default(0),
                )
                .col(timestamp_null(UptimeCheck::StateChangedAt))
                .col(timestamp_null(UptimeCheck::LastCheckedAt))
                .col(
                    timestamp(UptimeCheck::CreatedAt)
                        .not_null()
                        .default(Expr::cust("CURRENT_TIMESTAMP")),
                )
                .foreign_key(
                    ForeignKey::create()
                        .name("fk_uptime_check_project_id")
                        .from(UptimeCheck::Table, UptimeCheck::ProjectId)
                        .to(Project::Table, Project::Id)
                        .on_delete(ForeignKeyAction::Cascade)
                        .on_update(ForeignKeyAction::Cascade),
                )
                .to_owned(),
        )
        .await?;

    manager
        .create_index(
            Index::create()
                .name("uq-uptime_check-project_name")
                .table(UptimeCheck::Table)
                .col(UptimeCheck::ProjectId)
                .col(UptimeCheck::Name)
                .unique()
                .to_owned(),
        )
        .await?;

    manager
        .create_table(
            Table::create()
                .table(UptimeResult::Table)
                .if_not_exists()
                .col(
                    uuid(UptimeResult::Id)
                        .primary_key()
                        .default(Expr::cust("gen_random_uuid()")),
                )
                .col(uuid(UptimeResult::CheckId).not_null())
                .col(boolean(UptimeResult::Ok).not_null())
                .col(integer_null(UptimeResult::StatusCode))
                .col(integer(UptimeResult::DurationMs).not_null())
                .col(string_null(UptimeResult::Error))
                .col(timestamp(UptimeResult::CheckedAt).not_null())
                .foreign_key(
                    ForeignKey::create()
                        .name("fk_uptime_result_check_id")
                        .from(UptimeResult::Table, UptimeResult::CheckId)
                        .to(UptimeCheck::Table, UptimeCheck::Id)
                        .on_delete(ForeignKeyAction::Cascade)
                        .on_update(ForeignKeyAction::Cascade),
                )
                .to_owned(),
        )
        .await?;

    manager
        .create_index(
            Index::create()
                .name("idx-uptime_result-check_id_checked_at")
                .table(UptimeResult::Table)
                .col(UptimeResult::CheckId)
                .col(UptimeResult::CheckedAt)
                .to_owned(),
        )
        .await?;

    manager
        .create_index(
            Index::create()
                .name("idx-uptime_result-checked_at")
                .table(UptimeResult::Table)
                .col(UptimeResult::CheckedAt)
                .to_owned(),
        )
        .await
}

async fn create_status_page(manager: &SchemaManager<'_>) -> Result<(), DbErr> {
    manager
        .create_table(
            Table::create()
                .table(StatusComponent::Table)
                .if_not_exists()
                .col(
                    uuid(StatusComponent::Id)
                        .primary_key()
                        .default(Expr::cust("gen_random_uuid()")),
                )
                .col(uuid(StatusComponent::ProjectId).not_null())
                .col(string(StatusComponent::Name).not_null())
                .col(text_null(StatusComponent::Description))
                .col(integer(StatusComponent::Position).not_null().default(0))
                .col(uuid_null(StatusComponent::AutoFromCheckId))
                .col(
                    string(StatusComponent::ManualState)
                        .not_null()
                        .default("operational"),
                )
                .col(
                    timestamp(StatusComponent::CreatedAt)
                        .not_null()
                        .default(Expr::cust("CURRENT_TIMESTAMP")),
                )
                .foreign_key(
                    ForeignKey::create()
                        .name("fk_status_component_project_id")
                        .from(StatusComponent::Table, StatusComponent::ProjectId)
                        .to(Project::Table, Project::Id)
                        .on_delete(ForeignKeyAction::Cascade)
                        .on_update(ForeignKeyAction::Cascade),
                )
                .foreign_key(
                    ForeignKey::create()
                        .name("fk_status_component_check_id")
                        .from(StatusComponent::Table, StatusComponent::AutoFromCheckId)
                        .to(UptimeCheck::Table, UptimeCheck::Id)
                        .on_delete(ForeignKeyAction::SetNull)
                        .on_update(ForeignKeyAction::Cascade),
                )
                .to_owned(),
        )
        .await?;

    manager
        .create_index(
            Index::create()
                .name("uq-status_component-project_name")
                .table(StatusComponent::Table)
                .col(StatusComponent::ProjectId)
                .col(StatusComponent::Name)
                .unique()
                .to_owned(),
        )
        .await?;

    manager
        .create_table(
            Table::create()
                .table(StatusIncident::Table)
                .if_not_exists()
                .col(
                    uuid(StatusIncident::Id)
                        .primary_key()
                        .default(Expr::cust("gen_random_uuid()")),
                )
                .col(uuid(StatusIncident::ProjectId).not_null())
                .col(string(StatusIncident::Title).not_null())
                .col(string(StatusIncident::Status).not_null())
                .col(string(StatusIncident::Impact).not_null())
                .col(json_binary(StatusIncident::ComponentIds).not_null())
                .col(timestamp(StatusIncident::StartedAt).not_null())
                .col(timestamp_null(StatusIncident::ResolvedAt))
                .col(
                    timestamp(StatusIncident::CreatedAt)
                        .not_null()
                        .default(Expr::cust("CURRENT_TIMESTAMP")),
                )
                .foreign_key(
                    ForeignKey::create()
                        .name("fk_status_incident_project_id")
                        .from(StatusIncident::Table, StatusIncident::ProjectId)
                        .to(Project::Table, Project::Id)
                        .on_delete(ForeignKeyAction::Cascade)
                        .on_update(ForeignKeyAction::Cascade),
                )
                .to_owned(),
        )
        .await?;

    manager
        .create_index(
            Index::create()
                .name("idx-status_incident-started_at")
                .table(StatusIncident::Table)
                .col(StatusIncident::StartedAt)
                .to_owned(),
        )
        .await?;

    manager
        .create_table(
            Table::create()
                .table(StatusIncidentUpdate::Table)
                .if_not_exists()
                .col(
                    uuid(StatusIncidentUpdate::Id)
                        .primary_key()
                        .default(Expr::cust("gen_random_uuid()")),
                )
                .col(uuid(StatusIncidentUpdate::IncidentId).not_null())
                .col(string(StatusIncidentUpdate::Status).not_null())
                .col(text(StatusIncidentUpdate::Body).not_null())
                .col(timestamp(StatusIncidentUpdate::CreatedAt).not_null())
                .foreign_key(
                    ForeignKey::create()
                        .name("fk_status_incident_update_incident_id")
                        .from(
                            StatusIncidentUpdate::Table,
                            StatusIncidentUpdate::IncidentId,
                        )
                        .to(StatusIncident::Table, StatusIncident::Id)
                        .on_delete(ForeignKeyAction::Cascade)
                        .on_update(ForeignKeyAction::Cascade),
                )
                .to_owned(),
        )
        .await?;

    manager
        .create_index(
            Index::create()
                .name("idx-status_incident_update-incident_id_created_at")
                .table(StatusIncidentUpdate::Table)
                .col(StatusIncidentUpdate::IncidentId)
                .col(StatusIncidentUpdate::CreatedAt)
                .to_owned(),
        )
        .await
}

async fn create_alert_rule(manager: &SchemaManager<'_>) -> Result<(), DbErr> {
    manager
        .create_table(
            Table::create()
                .table(AlertRule::Table)
                .if_not_exists()
                .col(
                    uuid(AlertRule::Id)
                        .primary_key()
                        .default(Expr::cust("gen_random_uuid()")),
                )
                .col(uuid(AlertRule::ProjectId).not_null())
                .col(string(AlertRule::Name).not_null())
                .col(boolean(AlertRule::Enabled).not_null().default(true))
                .col(string(AlertRule::Source).not_null())
                .col(text(AlertRule::Selector).not_null().default(""))
                .col(string(AlertRule::Comparator).not_null().default("gt"))
                .col(double(AlertRule::Threshold).not_null())
                .col(
                    big_integer(AlertRule::WindowSeconds)
                        .not_null()
                        .default(300),
                )
                .col(big_integer(AlertRule::ForSeconds).not_null().default(120))
                .col(
                    big_integer(AlertRule::RepeatSeconds)
                        .not_null()
                        .default(14_400),
                )
                .col(string(AlertRule::Severity).not_null().default("warning"))
                .col(string_null(AlertRule::NotifyEmail))
                .col(string_null(AlertRule::NotifyWebhook))
                .col(timestamp_null(AlertRule::SilenceUntil))
                .col(string(AlertRule::State).not_null().default("ok"))
                .col(timestamp_null(AlertRule::StateSince))
                .col(timestamp_null(AlertRule::LastNotifiedAt))
                .col(timestamp_null(AlertRule::LastEvaluatedAt))
                .col(string_null(AlertRule::LastValue))
                .col(
                    timestamp(AlertRule::CreatedAt)
                        .not_null()
                        .default(Expr::cust("CURRENT_TIMESTAMP")),
                )
                .foreign_key(
                    ForeignKey::create()
                        .name("fk_alert_rule_project_id")
                        .from(AlertRule::Table, AlertRule::ProjectId)
                        .to(Project::Table, Project::Id)
                        .on_delete(ForeignKeyAction::Cascade)
                        .on_update(ForeignKeyAction::Cascade),
                )
                .to_owned(),
        )
        .await?;

    manager
        .create_index(
            Index::create()
                .name("uq-alert_rule-project_name")
                .table(AlertRule::Table)
                .col(AlertRule::ProjectId)
                .col(AlertRule::Name)
                .unique()
                .to_owned(),
        )
        .await
}

#[derive(DeriveIden)]
enum Project {
    Table,
    Id,
    Slug,
    Name,
    ServerTokenHash,
    BrowserTokenHash,
    CorsOrigins,
    ScrapeTarget,
    ScrapeScheme,
    ScrapeMetricsToken,
    EventRetentionDays,
    IssueRetentionDays,
    MaxEventsPerIssue,
    StatusEnabled,
    StatusName,
    CreatedAt,
}

#[derive(DeriveIden)]
enum ErrorIssue {
    Table,
    Id,
    ProjectId,
    Fingerprint,
    Source,
    ErrorType,
    Title,
    Culprit,
    Level,
    Status,
    TimesSeen,
    FirstSeen,
    LastSeen,
    FirstRelease,
    LastRelease,
    Environment,
    ResolvedAt,
    AlertSentAt,
    CreatedAt,
}

#[derive(DeriveIden)]
enum ErrorEvent {
    Table,
    Id,
    ProjectId,
    IssueId,
    Source,
    Level,
    ErrorType,
    Message,
    Stack,
    Frames,
    Context,
    Release,
    Environment,
    UserId,
    UserEmail,
    ClientIp,
    CreatedAt,
}

#[derive(DeriveIden)]
enum Release {
    Table,
    Id,
    ProjectId,
    Version,
    Environment,
    CommitSha,
    Source,
    DeployedAt,
    CreatedAt,
}

#[derive(DeriveIden)]
enum AppHealth {
    Table,
    Id,
    ProjectId,
    Instance,
    Environment,
    Release,
    ReportedAt,
    Payload,
}

#[derive(DeriveIden)]
enum UptimeCheck {
    Table,
    Id,
    ProjectId,
    Name,
    Url,
    Method,
    ExpectedStatus,
    TimeoutMs,
    IntervalSeconds,
    Enabled,
    AssertBodyContains,
    FailureThreshold,
    CurrentState,
    ConsecutiveFailures,
    StateChangedAt,
    LastCheckedAt,
    CreatedAt,
}

#[derive(DeriveIden)]
enum UptimeResult {
    Table,
    Id,
    CheckId,
    Ok,
    StatusCode,
    DurationMs,
    Error,
    CheckedAt,
}

#[derive(DeriveIden)]
enum StatusComponent {
    Table,
    Id,
    ProjectId,
    Name,
    Description,
    Position,
    AutoFromCheckId,
    ManualState,
    CreatedAt,
}

#[derive(DeriveIden)]
enum StatusIncident {
    Table,
    Id,
    ProjectId,
    Title,
    Status,
    Impact,
    ComponentIds,
    StartedAt,
    ResolvedAt,
    CreatedAt,
}

#[derive(DeriveIden)]
enum StatusIncidentUpdate {
    Table,
    Id,
    IncidentId,
    Status,
    Body,
    CreatedAt,
}

#[derive(DeriveIden)]
enum AlertRule {
    Table,
    Id,
    ProjectId,
    Name,
    Enabled,
    Source,
    Selector,
    Comparator,
    Threshold,
    WindowSeconds,
    ForSeconds,
    RepeatSeconds,
    Severity,
    NotifyEmail,
    NotifyWebhook,
    SilenceUntil,
    State,
    StateSince,
    LastNotifiedAt,
    LastEvaluatedAt,
    LastValue,
    CreatedAt,
}
