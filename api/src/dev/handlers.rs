use axum::{
    extract::{Path, State},
    http::StatusCode,
    response::IntoResponse,
    Json,
};
use sea_orm::{EntityTrait, QueryOrder, QuerySelect};
use uuid::Uuid;

use crate::{app::App, database::models::job};

pub async fn list_emails<ExtraConfig: Clone + Send + Sync + 'static>(
    State(app): State<App<ExtraConfig>>,
) -> impl IntoResponse {
    let records = app.mailer.records().unwrap_or_default();
    Json(records)
}

pub async fn clear_emails<ExtraConfig: Clone + Send + Sync + 'static>(
    State(app): State<App<ExtraConfig>>,
) -> impl IntoResponse {
    app.mailer.clear_messages();
    StatusCode::NO_CONTENT
}

pub async fn list_jobs<ExtraConfig: Clone + Send + Sync + 'static>(
    State(app): State<App<ExtraConfig>>,
) -> impl IntoResponse {
    match job::Entity::find()
        .order_by_desc(job::Column::CreatedAt)
        .limit(100)
        .all(&app.db)
        .await
    {
        Ok(jobs) => Json(jobs).into_response(),
        Err(_) => StatusCode::INTERNAL_SERVER_ERROR.into_response(),
    }
}

pub async fn clear_jobs<ExtraConfig: Clone + Send + Sync + 'static>(
    State(app): State<App<ExtraConfig>>,
) -> impl IntoResponse {
    match job::Entity::delete_many().exec(&app.db).await {
        Ok(_) => StatusCode::NO_CONTENT.into_response(),
        Err(_) => StatusCode::INTERNAL_SERVER_ERROR.into_response(),
    }
}

pub async fn delete_email<ExtraConfig: Clone + Send + Sync + 'static>(
    State(app): State<App<ExtraConfig>>,
    Path(id): Path<Uuid>,
) -> impl IntoResponse {
    if app.mailer.remove_record(id) {
        StatusCode::NO_CONTENT
    } else {
        StatusCode::NOT_FOUND
    }
}

/// Standalone HTML page wrapping a mock email: a small header with the
/// envelope metadata plus an iframe rendering the message body untouched, so
/// the email's own CSS applies exactly as a mail client would show it.
pub async fn email_preview<ExtraConfig: Clone + Send + Sync + 'static>(
    State(app): State<App<ExtraConfig>>,
    Path(id): Path<Uuid>,
) -> impl IntoResponse {
    let Some(record) = app.mailer.record(id) else {
        return (StatusCode::NOT_FOUND, "Email not found").into_response();
    };

    let page = format!(
        r#"<!doctype html>
<html lang="en">
<head>
<meta charset="utf-8">
<meta name="viewport" content="width=device-width, initial-scale=1">
<title>{subject}</title>
<style>
  :root {{ color-scheme: light dark; }}
  * {{ box-sizing: border-box; }}
  body {{
    margin: 0; height: 100vh; display: flex; flex-direction: column;
    background: #f4f5f7; color: #1c1d21;
    font: 14px/1.5 -apple-system, BlinkMacSystemFont, "Segoe UI", Roboto, sans-serif;
  }}
  header {{
    padding: 16px 20px; background: #fff; border-bottom: 1px solid #dfe1e6;
    box-shadow: 0 1px 3px rgba(0,0,0,.06);
  }}
  h1 {{ margin: 0 0 8px; font-size: 18px; font-weight: 600; }}
  dl {{ margin: 0; display: grid; grid-template-columns: auto 1fr; gap: 2px 10px; font-size: 13px; }}
  dt {{ color: #6b6f76; }}
  dd {{ margin: 0; }}
  .badge {{
    display: inline-block; margin-left: 8px; padding: 1px 6px; border-radius: 4px;
    background: #eceef1; color: #6b6f76; font-size: 11px; font-weight: 500;
    text-transform: uppercase; letter-spacing: .04em; vertical-align: 2px;
  }}
  iframe {{ flex: 1; width: 100%; border: 0; background: #fff; }}
  @media (prefers-color-scheme: dark) {{
    body {{ background: #17181c; color: #e8e9ec; }}
    header {{ background: #1f2126; border-bottom-color: #2e3138; box-shadow: none; }}
    dt {{ color: #9a9ea6; }}
    .badge {{ background: #2e3138; color: #9a9ea6; }}
  }}
</style>
</head>
<body>
<header>
  <h1>{subject}<span class="badge">{kind}</span></h1>
  <dl>
    <dt>From</dt><dd>{from}</dd>
    <dt>To</dt><dd>{to}</dd>
    <dt>Sent</dt><dd>{created_at}</dd>
  </dl>
</header>
<iframe src="body" sandbox title="Email body"></iframe>
</body>
</html>
"#,
        subject = escape_html(&record.subject),
        from = escape_html(&record.from),
        to = escape_html(&record.to),
        created_at = record.created_at.format("%Y-%m-%d %H:%M:%S UTC"),
        kind = if record.body_html.is_some() {
            "html"
        } else {
            "text"
        },
    );

    html_response(page)
}

/// The raw message body, served on its own so the preview iframe renders it in
/// isolation. Plain-text-only messages are wrapped in a `<pre>`.
pub async fn email_body<ExtraConfig: Clone + Send + Sync + 'static>(
    State(app): State<App<ExtraConfig>>,
    Path(id): Path<Uuid>,
) -> impl IntoResponse {
    let Some(record) = app.mailer.record(id) else {
        return (StatusCode::NOT_FOUND, "Email not found").into_response();
    };

    let body = match (record.body_html, record.body_text) {
        (Some(html), _) => html,
        (None, Some(text)) => format!(
            "<!doctype html><meta charset=\"utf-8\">\
             <pre style=\"margin:0;padding:20px;white-space:pre-wrap;\
             font:13px/1.6 ui-monospace,SFMono-Regular,Menlo,monospace\">{}</pre>",
            escape_html(&text)
        ),
        (None, None) => "<!doctype html><meta charset=\"utf-8\"><p style=\"padding:20px;\
                         font-family:sans-serif;color:#6b6f76\">This message has no body.</p>"
            .to_string(),
    };

    html_response(body)
}

fn html_response(body: String) -> axum::response::Response {
    (
        [(axum::http::header::CONTENT_TYPE, "text/html; charset=utf-8")],
        body,
    )
        .into_response()
}

fn escape_html(input: &str) -> String {
    input
        .replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
        .replace('\'', "&#39;")
}
