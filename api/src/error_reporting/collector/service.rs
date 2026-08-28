//! Operator queries.
//!
//! Docs: docs/src/content/docs/monitoring/error-reporting.md
//!
//! Pagination and window clamps mirror `admin::service::list_events`, so the
//! monitoring console behaves like the rest of Erno's operator surfaces.

use chrono::{Duration, NaiveDateTime, Utc};
use sea_orm::{
    ColumnTrait, Condition, ConnectionTrait, DatabaseConnection, DbBackend, DbErr, EntityTrait,
    PaginatorTrait, QueryFilter, QueryOrder, QuerySelect, Statement, Value,
};
use uuid::Uuid;

use std::collections::HashMap;

use super::{
    models::{error_event, error_issue, project, IssueStatus},
    operator_dto::{
        EventDto, EventListResponse, IssueCounts, IssueDetail, IssueListResponse, IssueSummary,
        SeriesPoint, SeriesResponse,
    },
};

/// Largest page an operator can ask for.
const MAX_PER_PAGE: u64 = 200;
/// Longest look-back window, in hours.
const MAX_HOURS: i64 = 24 * 365;
/// Events embedded in the issue detail response.
const DETAIL_EVENT_LIMIT: u64 = 50;

fn clamp_page(page: Option<u64>) -> u64 {
    page.unwrap_or(1).max(1)
}

fn clamp_per_page(per_page: Option<u64>) -> u64 {
    per_page.unwrap_or(50).clamp(1, MAX_PER_PAGE)
}

fn clamp_hours(hours: Option<i64>) -> i64 {
    hours.unwrap_or(24 * 7).clamp(1, MAX_HOURS)
}

/// Escape `%` and `_` so a search term is treated literally.
fn escape_like(term: &str) -> String {
    term.replace('\\', "\\\\")
        .replace('%', "\\%")
        .replace('_', "\\_")
}

/// Slug and display name per project id, for labelling issue rows.
///
/// The all-projects list has to say which application each row came from, and
/// a per-row lookup would be one query per issue. One query per page instead.
type ProjectLabels = HashMap<Uuid, (String, String)>;

async fn project_labels<I>(db: &DatabaseConnection, ids: I) -> Result<ProjectLabels, DbErr>
where
    I: IntoIterator<Item = Uuid>,
{
    let ids: Vec<Uuid> = {
        let mut ids: Vec<Uuid> = ids.into_iter().collect();
        ids.sort_unstable();
        ids.dedup();
        ids
    };
    if ids.is_empty() {
        return Ok(HashMap::new());
    }
    Ok(project::Entity::find()
        .filter(project::Column::Id.is_in(ids))
        .all(db)
        .await?
        .into_iter()
        .map(|p| (p.id, (p.slug, p.name)))
        .collect())
}

fn summary(model: error_issue::Model, labels: &ProjectLabels) -> IssueSummary {
    // A missing label means the project row vanished between the two queries.
    // Empty strings keep the row visible rather than dropping an issue from the
    // list over a label.
    let (slug, name) = labels
        .get(&model.project_id)
        .cloned()
        .unwrap_or_else(|| (String::new(), String::new()));
    IssueSummary {
        id: model.id,
        fingerprint: model.fingerprint,
        source: model.source,
        error_type: model.error_type,
        title: model.title,
        culprit: model.culprit,
        level: model.level,
        status: model.status,
        times_seen: model.times_seen,
        first_seen: model.first_seen,
        last_seen: model.last_seen,
        first_release: model.first_release,
        last_release: model.last_release,
        environment: model.environment,
        project_slug: slug,
        project_name: name,
    }
}

impl From<error_event::Model> for EventDto {
    fn from(model: error_event::Model) -> Self {
        Self {
            id: model.id,
            issue_id: model.issue_id,
            source: model.source,
            level: model.level,
            error_type: model.error_type,
            message: model.message,
            stack: model.stack,
            frames: model.frames,
            context: model.context,
            release: model.release,
            environment: model.environment,
            user_id: model.user_id,
            user_email: model.user_email,
            created_at: model.created_at,
        }
    }
}

/// Filters for [`list_issues`], already parsed from the query string.
#[derive(Debug, Clone, Default)]
pub struct IssueFilters {
    /// Restrict to one project. `None` is the all-projects view.
    pub project_id: Option<Uuid>,
    pub status: Option<String>,
    pub source: Option<String>,
    pub q: Option<String>,
    pub release: Option<String>,
    pub hours: Option<i64>,
    pub page: Option<u64>,
    pub per_page: Option<u64>,
}

fn issue_condition(filters: &IssueFilters, since: NaiveDateTime) -> Condition {
    let mut condition = Condition::all().add(error_issue::Column::LastSeen.gte(since));

    // Nested routes always set this; the all-projects list leaves it open. The
    // (project_id, status, last_seen) index serves both.
    if let Some(project_id) = filters.project_id {
        condition = condition.add(error_issue::Column::ProjectId.eq(project_id));
    }

    // `all` is an explicit opt-out; anything unrecognised falls back to the
    // default rather than silently returning everything.
    match filters.status.as_deref() {
        Some("all") => {}
        Some(other) => {
            let status = IssueStatus::from_str_opt(other).unwrap_or(IssueStatus::Unresolved);
            condition = condition.add(error_issue::Column::Status.eq(status.as_str()));
        }
        None => {
            condition =
                condition.add(error_issue::Column::Status.eq(IssueStatus::Unresolved.as_str()));
        }
    }

    if let Some(source) = filters
        .source
        .as_deref()
        .filter(|s| !s.is_empty() && *s != "all")
    {
        condition = condition.add(error_issue::Column::Source.eq(source));
    }
    if let Some(release) = filters.release.as_deref().filter(|s| !s.is_empty()) {
        condition = condition.add(error_issue::Column::LastRelease.eq(release));
    }
    if let Some(term) = filters
        .q
        .as_deref()
        .map(str::trim)
        .filter(|s| !s.is_empty())
    {
        let pattern = format!("%{}%", escape_like(term));
        condition = condition.add(
            Condition::any()
                .add(error_issue::Column::Title.like(&pattern))
                .add(error_issue::Column::ErrorType.like(&pattern))
                .add(error_issue::Column::Culprit.like(&pattern)),
        );
    }

    condition
}

/// List issues, newest activity first.
///
/// # Errors
///
/// Returns the underlying [`DbErr`] when the query fails.
pub async fn list_issues(
    db: &DatabaseConnection,
    filters: &IssueFilters,
) -> Result<IssueListResponse, DbErr> {
    let page = clamp_page(filters.page);
    let per_page = clamp_per_page(filters.per_page);
    let since = Utc::now().naive_utc() - Duration::hours(clamp_hours(filters.hours));

    let condition = issue_condition(filters, since);

    let total = error_issue::Entity::find()
        .filter(condition.clone())
        .count(db)
        .await?;

    let rows = error_issue::Entity::find()
        .filter(condition)
        .order_by_desc(error_issue::Column::LastSeen)
        .offset((page - 1) * per_page)
        .limit(per_page)
        .all(db)
        .await?;

    let labels = project_labels(db, rows.iter().map(|i| i.project_id)).await?;
    let issues = rows.into_iter().map(|i| summary(i, &labels)).collect();

    Ok(IssueListResponse {
        issues,
        page,
        per_page,
        total,
    })
}

/// Counts per status for the list header, over the same window.
///
/// # Errors
///
/// Returns the underlying [`DbErr`] when the query fails.
pub async fn issue_counts(
    db: &DatabaseConnection,
    project_id: Option<Uuid>,
    hours: Option<i64>,
) -> Result<IssueCounts, DbErr> {
    let since = Utc::now().naive_utc() - Duration::hours(clamp_hours(hours));
    let mut counts = IssueCounts {
        unresolved: 0,
        resolved: 0,
        ignored: 0,
    };

    for status in [
        IssueStatus::Unresolved,
        IssueStatus::Resolved,
        IssueStatus::Ignored,
    ] {
        let mut query = error_issue::Entity::find()
            .filter(error_issue::Column::LastSeen.gte(since))
            .filter(error_issue::Column::Status.eq(status.as_str()));
        if let Some(project_id) = project_id {
            query = query.filter(error_issue::Column::ProjectId.eq(project_id));
        }
        let count = query.count(db).await? as i64;
        match status {
            IssueStatus::Unresolved => counts.unresolved = count,
            IssueStatus::Resolved => counts.resolved = count,
            IssueStatus::Ignored => counts.ignored = count,
        }
    }

    Ok(counts)
}

/// Fetch one issue with its most recent occurrences.
///
/// # Errors
///
/// Returns the underlying [`DbErr`] when the query fails.
pub async fn get_issue(
    db: &DatabaseConnection,
    project_id: Uuid,
    id: Uuid,
) -> Result<Option<IssueDetail>, DbErr> {
    // Filtering rather than fetch-then-compare: an id from another project is
    // indistinguishable from one that does not exist, which is what the nested
    // route should say.
    let Some(issue) = error_issue::Entity::find_by_id(id)
        .filter(error_issue::Column::ProjectId.eq(project_id))
        .one(db)
        .await?
    else {
        return Ok(None);
    };

    let stored_events = error_event::Entity::find()
        .filter(error_event::Column::IssueId.eq(id))
        .count(db)
        .await? as i64;

    let events: Vec<EventDto> = error_event::Entity::find()
        .filter(error_event::Column::IssueId.eq(id))
        .order_by_desc(error_event::Column::CreatedAt)
        .limit(DETAIL_EVENT_LIMIT)
        .all(db)
        .await?
        .into_iter()
        .map(EventDto::from)
        .collect();

    let labels = project_labels(db, [issue.project_id]).await?;

    Ok(Some(IssueDetail {
        issue: summary(issue, &labels),
        stored_events,
        latest_event: events.first().cloned(),
        events,
    }))
}

/// Page through an issue's occurrences.
///
/// # Errors
///
/// Returns the underlying [`DbErr`] when the query fails.
pub async fn list_events(
    db: &DatabaseConnection,
    project_id: Uuid,
    issue_id: Uuid,
    page: Option<u64>,
    per_page: Option<u64>,
) -> Result<EventListResponse, DbErr> {
    let page = clamp_page(page);
    let per_page = clamp_per_page(per_page);

    // `error_event.project_id` is denormalised precisely so this does not have
    // to join `error_issue` to prove the issue belongs here.
    let total = error_event::Entity::find()
        .filter(error_event::Column::ProjectId.eq(project_id))
        .filter(error_event::Column::IssueId.eq(issue_id))
        .count(db)
        .await?;

    let events = error_event::Entity::find()
        .filter(error_event::Column::ProjectId.eq(project_id))
        .filter(error_event::Column::IssueId.eq(issue_id))
        .order_by_desc(error_event::Column::CreatedAt)
        .offset((page - 1) * per_page)
        .limit(per_page)
        .all(db)
        .await?
        .into_iter()
        .map(EventDto::from)
        .collect();

    Ok(EventListResponse {
        events,
        page,
        per_page,
        total,
    })
}

/// Bucket width for a window, chosen server-side so the client never has to.
const fn bucket_for(hours: i64) -> (&'static str, &'static str) {
    if hours <= 6 {
        ("minute", "1 minute")
    } else if hours <= 48 {
        ("hour", "1 hour")
    } else {
        ("day", "1 day")
    }
}

/// Occurrences over time, zero-filled.
///
/// Gaps are filled rather than omitted: a sparkline that simply skips empty
/// buckets draws a flat line through an outage and quietly lies about it.
///
/// # Errors
///
/// Returns the underlying [`DbErr`] when the query fails.
pub async fn series(
    db: &DatabaseConnection,
    project_id: Option<Uuid>,
    issue_id: Option<Uuid>,
    hours: Option<i64>,
    source: Option<&str>,
) -> Result<SeriesResponse, DbErr> {
    let hours = clamp_hours(hours);
    // Derived from a clamped integer, never from caller input.
    let (bucket, step) = bucket_for(hours);

    let mut values: Vec<Value> = vec![hours.into()];
    let mut filters = String::new();

    // Placeholder numbers come from `values.len()` rather than being written
    // out, so adding a filter cannot silently shift the ones after it.
    if let Some(id) = project_id {
        values.push(id.into());
        filters.push_str(&format!(" AND e.project_id = ${}", values.len()));
    }
    if let Some(id) = issue_id {
        values.push(id.into());
        filters.push_str(&format!(" AND e.issue_id = ${}", values.len()));
    }
    if let Some(source) = source.filter(|s| !s.is_empty() && *s != "all") {
        values.push(source.to_string().into());
        filters.push_str(&format!(" AND e.source = ${}", values.len()));
    }

    // The bounds are inlined rather than pulled from a CTE: a LEFT JOIN's ON
    // clause can only see the FROM item it attaches to, so a CTE reference
    // there is an "invalid reference to FROM-clause entry".
    //
    // Matching on the truncated bucket already confines rows to the window, so
    // no additional range predicate is needed.
    let sql = format!(
        "SELECT b.bucket AS t, count(e.id)::bigint AS count
         FROM generate_series(
                  date_trunc('{bucket}', (now() AT TIME ZONE 'utc') - ($1::bigint || ' hours')::interval),
                  date_trunc('{bucket}', (now() AT TIME ZONE 'utc')),
                  '{step}'::interval
              ) AS b(bucket)
         LEFT JOIN error_event e
           ON date_trunc('{bucket}', e.created_at) = b.bucket{filters}
         GROUP BY b.bucket
         ORDER BY b.bucket"
    );

    let rows = db
        .query_all(Statement::from_sql_and_values(
            DbBackend::Postgres,
            &sql,
            values,
        ))
        .await?;

    let points = rows
        .into_iter()
        .map(|row| {
            Ok(SeriesPoint {
                t: row.try_get("", "t")?,
                count: row.try_get("", "count")?,
            })
        })
        .collect::<Result<Vec<_>, DbErr>>()?;

    Ok(SeriesResponse {
        points,
        bucket: bucket.to_string(),
    })
}

/// Change an issue's triage state.
///
/// Resolving stamps `resolved_at` in UTC, which is what lets the ingest upsert
/// recognise a later recurrence as a regression. A local-time value here would
/// silently never reopen.
///
/// # Errors
///
/// Returns the underlying [`DbErr`] when the update fails.
pub async fn set_status(
    db: &DatabaseConnection,
    project_id: Uuid,
    id: Uuid,
    status: IssueStatus,
) -> Result<Option<IssueSummary>, DbErr> {
    use sea_orm::{ActiveModelTrait, ActiveValue::Set};

    let Some(issue) = error_issue::Entity::find_by_id(id)
        .filter(error_issue::Column::ProjectId.eq(project_id))
        .one(db)
        .await?
    else {
        return Ok(None);
    };

    let mut active: error_issue::ActiveModel = issue.into();
    active.status = Set(status.as_str().to_string());
    active.resolved_at = Set(match status {
        IssueStatus::Resolved => Some(Utc::now().naive_utc()),
        IssueStatus::Unresolved | IssueStatus::Ignored => None,
    });

    let updated = active.update(db).await?;
    let labels = project_labels(db, [updated.project_id]).await?;
    Ok(Some(summary(updated, &labels)))
}

/// Delete an issue and, by cascade, its events.
///
/// # Errors
///
/// Returns the underlying [`DbErr`] when the delete fails.
pub async fn delete_issue(
    db: &DatabaseConnection,
    project_id: Uuid,
    id: Uuid,
) -> Result<bool, DbErr> {
    let result = error_issue::Entity::delete_many()
        .filter(error_issue::Column::Id.eq(id))
        .filter(error_issue::Column::ProjectId.eq(project_id))
        .exec(db)
        .await?;
    Ok(result.rows_affected > 0)
}

/// Clear a deleted account's identity from stored events.
///
/// Anonymises rather than deletes: a stack, a release and a grouping are not
/// personal data, and removing the rows would corrupt `times_seen` and every
/// time series that has already been drawn.
///
/// # Errors
///
/// Returns the underlying [`DbErr`] when the update fails.
pub async fn anonymize_user(
    db: &DatabaseConnection,
    project_id: Uuid,
    user_id: Uuid,
) -> Result<u64, DbErr> {
    use sea_orm::sea_query::Expr;

    let result = error_event::Entity::update_many()
        .col_expr(
            error_event::Column::UserId,
            Expr::value(Option::<Uuid>::None),
        )
        .col_expr(
            error_event::Column::UserEmail,
            Expr::value(Option::<String>::None),
        )
        .filter(error_event::Column::ProjectId.eq(project_id))
        .filter(error_event::Column::UserId.eq(user_id))
        .exec(db)
        .await?;

    Ok(result.rows_affected)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pagination_is_clamped_into_range() {
        assert_eq!(clamp_page(None), 1);
        assert_eq!(clamp_page(Some(0)), 1, "page 0 is not a thing");
        assert_eq!(clamp_page(Some(7)), 7);

        assert_eq!(clamp_per_page(None), 50);
        assert_eq!(clamp_per_page(Some(0)), 1);
        assert_eq!(
            clamp_per_page(Some(10_000)),
            MAX_PER_PAGE,
            "an operator cannot ask for the whole table"
        );
    }

    #[test]
    fn the_window_is_clamped_to_a_year() {
        assert_eq!(clamp_hours(None), 24 * 7);
        assert_eq!(clamp_hours(Some(0)), 1);
        assert_eq!(clamp_hours(Some(-5)), 1);
        assert_eq!(clamp_hours(Some(999_999)), MAX_HOURS);
    }

    #[test]
    fn bucket_width_follows_the_window() {
        assert_eq!(bucket_for(1).0, "minute");
        assert_eq!(bucket_for(6).0, "minute");
        assert_eq!(bucket_for(24).0, "hour");
        assert_eq!(bucket_for(48).0, "hour");
        assert_eq!(bucket_for(72).0, "day");
        assert_eq!(bucket_for(24 * 90).0, "day");
    }

    #[test]
    fn like_wildcards_in_a_search_term_are_escaped() {
        // Without this, searching for "100%" would match everything.
        assert_eq!(escape_like("100%"), "100\\%");
        assert_eq!(escape_like("a_b"), "a\\_b");
        assert_eq!(escape_like("plain"), "plain");
    }
}
