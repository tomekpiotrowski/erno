//! Docs: docs/src/content/docs/monitoring/status-page.md
use sea_orm_migration::{
    prelude::*,
    schema::{
        integer, json_binary, string, text, text_null, timestamp, timestamp_null, uuid, uuid_null,
    },
};

use super::m20260823_140000_create_uptime::UptimeCheck;

#[derive(DeriveMigrationName)]
pub struct Migration;

#[async_trait::async_trait]
impl MigrationTrait for Migration {
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
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
                    .col(string(StatusComponent::Name).not_null())
                    .col(text_null(StatusComponent::Description))
                    .col(integer(StatusComponent::Position).not_null().default(0))
                    // When set, the component's state follows a probe rather
                    // than an operator's opinion.
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
                            .name("fk_status_component_check_id")
                            .from(StatusComponent::Table, StatusComponent::AutoFromCheckId)
                            .to(UptimeCheck::Table, UptimeCheck::Id)
                            // Deleting a check leaves the component, now manual.
                            .on_delete(ForeignKeyAction::SetNull)
                            .on_update(ForeignKeyAction::Cascade),
                    )
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

    async fn down(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager
            .drop_table(Table::drop().table(StatusIncidentUpdate::Table).to_owned())
            .await?;
        manager
            .drop_table(Table::drop().table(StatusIncident::Table).to_owned())
            .await?;
        manager
            .drop_table(Table::drop().table(StatusComponent::Table).to_owned())
            .await
    }
}

#[derive(DeriveIden)]
pub enum StatusComponent {
    Table,
    Id,
    Name,
    Description,
    Position,
    AutoFromCheckId,
    ManualState,
    CreatedAt,
}

#[derive(DeriveIden)]
pub enum StatusIncident {
    Table,
    Id,
    Title,
    Status,
    Impact,
    ComponentIds,
    StartedAt,
    ResolvedAt,
    CreatedAt,
}

#[derive(DeriveIden)]
pub enum StatusIncidentUpdate {
    Table,
    Id,
    IncidentId,
    Status,
    Body,
    CreatedAt,
}
