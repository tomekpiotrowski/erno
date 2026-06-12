use axum::{routing::get, Router};
use chrono::Utc;
use sea_orm::{
    ActiveModelTrait, ColumnTrait, Condition, ConnectionTrait, EntityTrait, QueryFilter, Set,
};
use serde_json::json;
use uuid::Uuid;

use crate::{
    app::App,
    auth::jwt::generate_token,
    database::{migrations::Migrator, models::user},
    password::hash_password,
    policy::Policy,
    share::extractor::SHARE_TOKEN_HEADER,
    share::models::{share, share_grant},
    share::principal::{resolve_principal, resolve_share_token, ActiveShare, FromPrincipal, Principal},
    share::router::share_router,
    sync::delta::sync_delta_shared,
    sync::registry::SyncRegistry,
    sync::syncable::Syncable,
    tests::setup_test::{setup_test_with_registry, TestUtils},
    token::hash_token,
};

// ---------------------------------------------------------------------------
// Test entities: a shareable "note" and its implied "note comments".
// Tables are created inside each test's transaction, so they roll back.
// ---------------------------------------------------------------------------

mod note {
    use chrono::NaiveDateTime;
    use sea_orm::entity::prelude::*;
    use serde::{Deserialize, Serialize};

    #[derive(Clone, Debug, PartialEq, DeriveEntityModel, Serialize, Deserialize)]
    #[sea_orm(table_name = "share_test_notes")]
    pub struct Model {
        #[sea_orm(primary_key, auto_increment = false)]
        pub id: Uuid,
        pub user_id: Uuid,
        pub body: String,
        pub sync_seq: i64,
        pub deleted_at: Option<NaiveDateTime>,
    }

    #[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
    pub enum Relation {}

    impl ActiveModelBehavior for ActiveModel {}
}

mod note_comment {
    use chrono::NaiveDateTime;
    use sea_orm::entity::prelude::*;
    use serde::{Deserialize, Serialize};

    #[derive(Clone, Debug, PartialEq, DeriveEntityModel, Serialize, Deserialize)]
    #[sea_orm(table_name = "share_test_comments")]
    pub struct Model {
        #[sea_orm(primary_key, auto_increment = false)]
        pub id: Uuid,
        pub note_id: Uuid,
        pub user_id: Uuid,
        pub body: String,
        pub sync_seq: i64,
        pub deleted_at: Option<NaiveDateTime>,
    }

    #[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
    pub enum Relation {}

    impl ActiveModelBehavior for ActiveModel {}
}

/// Owner sees own notes; an active share on a note widens read access to it.
/// Updating (and therefore sharing) stays owner-only: share-derived access is
/// read-only in v1, so `can_update` must not consult `shared_note_ids`.
pub struct NotePolicy {
    pub user_id: Option<Uuid>,
    pub shared_note_ids: Vec<Uuid>,
}

impl FromPrincipal for NotePolicy {
    fn from_principal(principal: &Principal) -> Self {
        Self {
            user_id: principal.user.as_ref().map(|u| u.id),
            shared_note_ids: principal.shared_ids("share_test_notes"),
        }
    }
}

impl Policy<note::Entity> for NotePolicy {
    fn can_read(&self, entity: &note::Model) -> bool {
        self.user_id == Some(entity.user_id) || self.shared_note_ids.contains(&entity.id)
    }

    fn can_update(&self, entity: &note::Model) -> bool {
        self.user_id == Some(entity.user_id)
    }

    fn readable(&self, query: sea_orm::Select<note::Entity>) -> sea_orm::Select<note::Entity> {
        let mut condition =
            Condition::any().add(note::Column::Id.is_in(self.shared_note_ids.clone()));
        if let Some(user_id) = self.user_id {
            condition = condition.add(note::Column::UserId.eq(user_id));
        }
        query.filter(condition)
    }
}

impl Syncable for note::Entity {
    type Policy = NotePolicy;
    fn entity_type() -> &'static str {
        "share_test_notes"
    }
    fn sync_seq_column() -> note::Column {
        note::Column::SyncSeq
    }
    fn sync_seq(model: &note::Model) -> i64 {
        model.sync_seq
    }
}

/// Access to a note implies access to its comments: the comment policy keys
/// on the principal's shared *note* ids — the transitive-implication pattern.
pub struct CommentPolicy {
    pub user_id: Option<Uuid>,
    pub shared_note_ids: Vec<Uuid>,
}

impl FromPrincipal for CommentPolicy {
    fn from_principal(principal: &Principal) -> Self {
        Self {
            user_id: principal.user.as_ref().map(|u| u.id),
            shared_note_ids: principal.shared_ids("share_test_notes"),
        }
    }
}

impl Policy<note_comment::Entity> for CommentPolicy {
    fn can_read(&self, entity: &note_comment::Model) -> bool {
        self.user_id == Some(entity.user_id) || self.shared_note_ids.contains(&entity.note_id)
    }

    fn can_update(&self, entity: &note_comment::Model) -> bool {
        self.user_id == Some(entity.user_id)
    }

    fn readable(
        &self,
        query: sea_orm::Select<note_comment::Entity>,
    ) -> sea_orm::Select<note_comment::Entity> {
        let mut condition =
            Condition::any().add(note_comment::Column::NoteId.is_in(self.shared_note_ids.clone()));
        if let Some(user_id) = self.user_id {
            condition = condition.add(note_comment::Column::UserId.eq(user_id));
        }
        query.filter(condition)
    }
}

impl Syncable for note_comment::Entity {
    type Policy = CommentPolicy;
    fn entity_type() -> &'static str {
        "share_test_comments"
    }
    fn sync_seq_column() -> note_comment::Column {
        note_comment::Column::SyncSeq
    }
    fn sync_seq(model: &note_comment::Model) -> i64 {
        model.sync_seq
    }
}

// ---------------------------------------------------------------------------
// Test setup helpers
// ---------------------------------------------------------------------------

fn registry() -> SyncRegistry {
    SyncRegistry::new()
        .register_shareable::<note::Entity>()
        .register_shareable::<note_comment::Entity>()
}

fn test_router(app: App) -> Router {
    Router::new()
        .route("/notes/sync", get(sync_delta_shared::<note::Entity, ()>))
        .route(
            "/comments/sync",
            get(sync_delta_shared::<note_comment::Entity, ()>),
        )
        .with_state(app.clone())
        .nest("/shares", share_router(app))
}

fn no_fixtures(
    db: &sea_orm::DatabaseConnection,
) -> std::pin::Pin<Box<dyn std::future::Future<Output = ()> + Send + '_>> {
    Box::pin(async move {
        let _ = db;
    })
}

async fn setup() -> TestUtils {
    let t = setup_test_with_registry::<Migrator>(test_router, no_fixtures, registry()).await;
    t.db.execute_unprepared(
        "CREATE TABLE share_test_notes (
            id UUID PRIMARY KEY,
            user_id UUID NOT NULL,
            body TEXT NOT NULL,
            sync_seq BIGINT NOT NULL DEFAULT 0,
            deleted_at TIMESTAMP NULL
        );
        CREATE TABLE share_test_comments (
            id UUID PRIMARY KEY,
            note_id UUID NOT NULL,
            user_id UUID NOT NULL,
            body TEXT NOT NULL,
            sync_seq BIGINT NOT NULL DEFAULT 0,
            deleted_at TIMESTAMP NULL
        );",
    )
    .await
    .expect("create test tables");
    t
}

async fn create_user(db: &sea_orm::DatabaseConnection, label: &str) -> user::Model {
    user::ActiveModel {
        email: Set(format!("{label}-{}@example.com", Uuid::new_v4())),
        password_hash: Set(hash_password("password123").unwrap()),
        email_verified_at: Set(Some(Utc::now().naive_utc())),
        ..Default::default()
    }
    .insert(db)
    .await
    .unwrap()
}

async fn create_note(
    db: &sea_orm::DatabaseConnection,
    owner: &user::Model,
    body: &str,
    sync_seq: i64,
) -> note::Model {
    note::ActiveModel {
        id: Set(Uuid::new_v4()),
        user_id: Set(owner.id),
        body: Set(body.to_string()),
        sync_seq: Set(sync_seq),
        deleted_at: Set(None),
    }
    .insert(db)
    .await
    .unwrap()
}

async fn create_comment(
    db: &sea_orm::DatabaseConnection,
    note: &note::Model,
    author: &user::Model,
    body: &str,
    sync_seq: i64,
) -> note_comment::Model {
    note_comment::ActiveModel {
        id: Set(Uuid::new_v4()),
        note_id: Set(note.id),
        user_id: Set(author.id),
        body: Set(body.to_string()),
        sync_seq: Set(sync_seq),
        deleted_at: Set(None),
    }
    .insert(db)
    .await
    .unwrap()
}

async fn create_share_row(
    db: &sea_orm::DatabaseConnection,
    owner: &user::Model,
    entity_type: &str,
    entity_id: Uuid,
    raw_token: Option<&str>,
) -> share::Model {
    share::ActiveModel {
        token_hash: Set(raw_token.map(hash_token)),
        entity_type: Set(entity_type.to_string()),
        entity_id: Set(entity_id),
        owner_id: Set(owner.id),
        permission: Set(share::SharePermission::Read),
        ..Default::default()
    }
    .insert(db)
    .await
    .unwrap()
}

async fn create_grant_row(
    db: &sea_orm::DatabaseConnection,
    share: &share::Model,
    user: &user::Model,
) -> share_grant::Model {
    share_grant::ActiveModel {
        share_id: Set(share.id),
        user_id: Set(user.id),
        ..Default::default()
    }
    .insert(db)
    .await
    .unwrap()
}

fn bearer(t: &TestUtils, user: &user::Model) -> String {
    format!(
        "Bearer {}",
        generate_token(&t.config, user.id, user.token_version).unwrap()
    )
}

fn delta_ids(body: &serde_json::Value) -> Vec<String> {
    body["items"]
        .as_array()
        .unwrap()
        .iter()
        .map(|i| i["id"].as_str().unwrap().to_string())
        .collect()
}

// ---------------------------------------------------------------------------
// Principal resolution
// ---------------------------------------------------------------------------

#[tokio::test]
async fn resolve_share_token_returns_active_share() {
    let t = setup().await;
    let owner = create_user(&t.db, "owner").await;
    let note = create_note(&t.db, &owner, "hello", 1).await;
    let share = create_share_row(&t.db, &owner, "share_test_notes", note.id, Some("tok-1")).await;

    let resolved = resolve_share_token(&t.db, "tok-1").await.unwrap();
    let resolved = resolved.expect("share resolves");
    assert_eq!(resolved.id, share.id);
    assert_eq!(resolved.entity_type, "share_test_notes");
    assert_eq!(resolved.entity_id, note.id);

    assert!(resolve_share_token(&t.db, "wrong-token")
        .await
        .unwrap()
        .is_none());
}

#[tokio::test]
async fn resolve_share_token_rejects_expired_and_revoked() {
    let t = setup().await;
    let owner = create_user(&t.db, "owner").await;
    let note = create_note(&t.db, &owner, "hello", 1).await;

    let expired = create_share_row(&t.db, &owner, "share_test_notes", note.id, Some("tok-exp")).await;
    let mut active: share::ActiveModel = expired.into();
    active.expires_at = Set(Some((Utc::now() - chrono::Duration::hours(1)).naive_utc()));
    active.update(&t.db).await.unwrap();

    let revoked = create_share_row(&t.db, &owner, "share_test_notes", note.id, Some("tok-rev")).await;
    let mut active: share::ActiveModel = revoked.into();
    active.revoked_at = Set(Some(Utc::now().naive_utc()));
    active.update(&t.db).await.unwrap();

    assert!(resolve_share_token(&t.db, "tok-exp").await.unwrap().is_none());
    assert!(resolve_share_token(&t.db, "tok-rev").await.unwrap().is_none());
}

#[tokio::test]
async fn resolve_principal_loads_account_grants() {
    let t = setup().await;
    let owner = create_user(&t.db, "owner").await;
    let recipient = create_user(&t.db, "recipient").await;
    let note = create_note(&t.db, &owner, "hello", 1).await;
    let share = create_share_row(&t.db, &owner, "share_test_notes", note.id, None).await;
    create_grant_row(&t.db, &share, &recipient).await;

    let principal = resolve_principal(&t.db, Some(recipient.clone()), &[])
        .await
        .unwrap();
    assert_eq!(principal.shares.len(), 1);
    assert_eq!(principal.shares[0].id, share.id);
    assert_eq!(principal.shared_ids("share_test_notes"), vec![note.id]);

    // A revoked grant no longer contributes.
    let grant = share_grant::Entity::find()
        .filter(share_grant::Column::ShareId.eq(share.id))
        .one(&t.db)
        .await
        .unwrap()
        .unwrap();
    let mut active: share_grant::ActiveModel = grant.into();
    active.revoked_at = Set(Some(Utc::now().naive_utc()));
    active.update(&t.db).await.unwrap();

    let principal = resolve_principal(&t.db, Some(recipient), &[]).await.unwrap();
    assert!(principal.shares.is_empty());
}

#[tokio::test]
async fn resolve_principal_dedupes_token_and_grant() {
    let t = setup().await;
    let owner = create_user(&t.db, "owner").await;
    let recipient = create_user(&t.db, "recipient").await;
    let note = create_note(&t.db, &owner, "hello", 1).await;
    let share = create_share_row(&t.db, &owner, "share_test_notes", note.id, Some("tok-dup")).await;
    create_grant_row(&t.db, &share, &recipient).await;

    let principal = resolve_principal(&t.db, Some(recipient), &["tok-dup".to_string()])
        .await
        .unwrap();
    assert_eq!(principal.shares.len(), 1);
}

// ---------------------------------------------------------------------------
// Registry: principal-based read checks + share authorization
// ---------------------------------------------------------------------------

#[tokio::test]
async fn registry_checks_principals_and_share_authority() {
    let t = setup().await;
    let owner = create_user(&t.db, "owner").await;
    let stranger = create_user(&t.db, "stranger").await;
    let note = create_note(&t.db, &owner, "hello", 1).await;
    let snapshot = serde_json::to_value(&note).unwrap();
    let registry = registry();

    // Owner reads own note; stranger does not.
    let owner_principal = Principal::from_user_model(owner.clone());
    let stranger_principal = Principal::from_user_model(stranger.clone());
    assert!(registry.can_principal_read("share_test_notes", &snapshot, &owner_principal));
    assert!(!registry.can_principal_read("share_test_notes", &snapshot, &stranger_principal));

    // An anonymous principal with an active share reads it.
    let anonymous_with_share = Principal {
        user: None,
        shares: vec![ActiveShare {
            id: Uuid::new_v4(),
            entity_type: "share_test_notes".to_string(),
            entity_id: note.id,
            permission: share::SharePermission::Read,
        }],
    };
    assert!(registry.can_principal_read("share_test_notes", &snapshot, &anonymous_with_share));

    // The same share implies the note's comments.
    let comment = create_comment(&t.db, &note, &owner, "a comment", 2).await;
    let comment_snapshot = serde_json::to_value(&comment).unwrap();
    assert!(registry.can_principal_read(
        "share_test_comments",
        &comment_snapshot,
        &anonymous_with_share
    ));

    // Unregistered entity types always deny.
    assert!(!registry.can_principal_read("unknown_entities", &snapshot, &owner_principal));

    // Sharing requires update authority: owner yes, stranger no, missing entity no.
    assert!(
        registry
            .can_user_share(&t.db, "share_test_notes", note.id, &owner)
            .await
    );
    assert!(
        !registry
            .can_user_share(&t.db, "share_test_notes", note.id, &stranger)
            .await
    );
    assert!(
        !registry
            .can_user_share(&t.db, "share_test_notes", Uuid::new_v4(), &owner)
            .await
    );
    assert!(registry.is_shareable("share_test_notes"));
    assert!(!registry.is_shareable("users"));
}

// ---------------------------------------------------------------------------
// Share-aware delta sync
// ---------------------------------------------------------------------------

#[tokio::test]
async fn delta_anonymous_share_header_scopes_results() {
    let t = setup().await;
    let owner = create_user(&t.db, "owner").await;
    let shared_note = create_note(&t.db, &owner, "shared", 1).await;
    create_note(&t.db, &owner, "private", 2).await;
    create_share_row(&t.db, &owner, "share_test_notes", shared_note.id, Some("tok-d1")).await;

    // Anonymous without the header: nothing.
    let response = t.server.get("/api/notes/sync").await;
    assert_eq!(response.status_code(), 200);
    assert!(delta_ids(&response.json::<serde_json::Value>()).is_empty());

    // Anonymous with the header: exactly the shared note.
    let response = t
        .server
        .get("/api/notes/sync")
        .add_header(SHARE_TOKEN_HEADER, "tok-d1")
        .await;
    assert_eq!(response.status_code(), 200);
    let body = response.json::<serde_json::Value>();
    assert_eq!(delta_ids(&body), vec![shared_note.id.to_string()]);
    assert_eq!(body["next_since"], 1);
}

#[tokio::test]
async fn delta_share_implies_comments() {
    let t = setup().await;
    let owner = create_user(&t.db, "owner").await;
    let shared_note = create_note(&t.db, &owner, "shared", 1).await;
    let other_note = create_note(&t.db, &owner, "private", 2).await;
    let on_shared = create_comment(&t.db, &shared_note, &owner, "visible", 3).await;
    create_comment(&t.db, &other_note, &owner, "hidden", 4).await;
    create_share_row(&t.db, &owner, "share_test_notes", shared_note.id, Some("tok-d2")).await;

    let response = t
        .server
        .get("/api/comments/sync")
        .add_header(SHARE_TOKEN_HEADER, "tok-d2")
        .await;
    assert_eq!(response.status_code(), 200);
    assert_eq!(
        delta_ids(&response.json::<serde_json::Value>()),
        vec![on_shared.id.to_string()]
    );
}

#[tokio::test]
async fn delta_grant_recipient_needs_no_header() {
    let t = setup().await;
    let owner = create_user(&t.db, "owner").await;
    let recipient = create_user(&t.db, "recipient").await;
    let shared_note = create_note(&t.db, &owner, "shared", 1).await;
    create_note(&t.db, &owner, "private", 2).await;
    let own_note = create_note(&t.db, &recipient, "mine", 3).await;
    let share = create_share_row(&t.db, &owner, "share_test_notes", shared_note.id, None).await;
    create_grant_row(&t.db, &share, &recipient).await;

    let response = t
        .server
        .get("/api/notes/sync")
        .add_header("Authorization", bearer(&t, &recipient))
        .await;
    assert_eq!(response.status_code(), 200);
    let mut ids = delta_ids(&response.json::<serde_json::Value>());
    ids.sort();
    let mut expected = vec![shared_note.id.to_string(), own_note.id.to_string()];
    expected.sort();
    assert_eq!(ids, expected);
}

#[tokio::test]
async fn delta_revoked_share_returns_nothing() {
    let t = setup().await;
    let owner = create_user(&t.db, "owner").await;
    let shared_note = create_note(&t.db, &owner, "shared", 1).await;
    let share =
        create_share_row(&t.db, &owner, "share_test_notes", shared_note.id, Some("tok-d3")).await;

    let mut active: share::ActiveModel = share.into();
    active.revoked_at = Set(Some(Utc::now().naive_utc()));
    active.update(&t.db).await.unwrap();

    let response = t
        .server
        .get("/api/notes/sync")
        .add_header(SHARE_TOKEN_HEADER, "tok-d3")
        .await;
    assert_eq!(response.status_code(), 200);
    assert!(delta_ids(&response.json::<serde_json::Value>()).is_empty());
}

#[tokio::test]
async fn delta_invalid_jwt_still_rejected() {
    let t = setup().await;
    let response = t
        .server
        .get("/api/notes/sync")
        .add_header("Authorization", "Bearer garbage")
        .await;
    assert_eq!(response.status_code(), 401);
}

// ---------------------------------------------------------------------------
// Share management endpoints
// ---------------------------------------------------------------------------

#[tokio::test]
async fn create_share_with_link_returns_raw_token_once() {
    let t = setup().await;
    let owner = create_user(&t.db, "owner").await;
    let note = create_note(&t.db, &owner, "to share", 1).await;

    let response = t
        .server
        .post("/api/shares")
        .add_header("Authorization", bearer(&t, &owner))
        .json(&json!({
            "entity_type": "share_test_notes",
            "entity_id": note.id,
            "link": true,
        }))
        .await;
    assert_eq!(response.status_code(), 201);
    let body = response.json::<serde_json::Value>();
    let raw_token = body["token"].as_str().expect("raw token returned").to_string();
    assert_eq!(body["permission"], "read");

    // Only the hash is stored, and the raw token resolves.
    let share_id = Uuid::parse_str(body["id"].as_str().unwrap()).unwrap();
    let stored = share::Entity::find_by_id(share_id)
        .one(&t.db)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(stored.token_hash, Some(hash_token(&raw_token)));

    let resolved = resolve_share_token(&t.db, &raw_token).await.unwrap();
    assert_eq!(resolved.unwrap().entity_id, note.id);
}

#[tokio::test]
async fn create_share_validations() {
    let t = setup().await;
    let owner = create_user(&t.db, "owner").await;
    let stranger = create_user(&t.db, "stranger").await;
    let note = create_note(&t.db, &owner, "mine", 1).await;

    // Not the owner → 403.
    let response = t
        .server
        .post("/api/shares")
        .add_header("Authorization", bearer(&t, &stranger))
        .json(&json!({ "entity_type": "share_test_notes", "entity_id": note.id, "link": true }))
        .await;
    assert_eq!(response.status_code(), 403);

    // Unshareable entity type → 422.
    let response = t
        .server
        .post("/api/shares")
        .add_header("Authorization", bearer(&t, &owner))
        .json(&json!({ "entity_type": "users", "entity_id": owner.id, "link": true }))
        .await;
    assert_eq!(response.status_code(), 422);

    // Neither link nor recipients → 422.
    let response = t
        .server
        .post("/api/shares")
        .add_header("Authorization", bearer(&t, &owner))
        .json(&json!({ "entity_type": "share_test_notes", "entity_id": note.id }))
        .await;
    assert_eq!(response.status_code(), 422);

    // Write permission reserved → 422.
    let response = t
        .server
        .post("/api/shares")
        .add_header("Authorization", bearer(&t, &owner))
        .json(&json!({
            "entity_type": "share_test_notes",
            "entity_id": note.id,
            "link": true,
            "permission": "write",
        }))
        .await;
    assert_eq!(response.status_code(), 422);

    // Unknown recipient → 422.
    let response = t
        .server
        .post("/api/shares")
        .add_header("Authorization", bearer(&t, &owner))
        .json(&json!({
            "entity_type": "share_test_notes",
            "entity_id": note.id,
            "recipient_user_ids": [Uuid::new_v4()],
        }))
        .await;
    assert_eq!(response.status_code(), 422);

    // No auth → 401.
    let response = t
        .server
        .post("/api/shares")
        .json(&json!({ "entity_type": "share_test_notes", "entity_id": note.id, "link": true }))
        .await;
    assert_eq!(response.status_code(), 401);
}

#[tokio::test]
async fn create_share_with_recipients_grants_immediately_and_fans_in() {
    let t = setup().await;
    let owner = create_user(&t.db, "owner").await;
    let recipient = create_user(&t.db, "recipient").await;
    let note = create_note(&t.db, &owner, "to grant", 1).await;

    // Recipient has a live connection before the grant is issued.
    let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel();
    let connection_id = t
        .websocket_connections
        .register_connection(Principal::from_user_model(recipient.clone()), tx)
        .await;

    let response = t
        .server
        .post("/api/shares")
        .add_header("Authorization", bearer(&t, &owner))
        .json(&json!({
            "entity_type": "share_test_notes",
            "entity_id": note.id,
            "recipient_user_ids": [recipient.id],
        }))
        .await;
    assert_eq!(response.status_code(), 201);
    let body = response.json::<serde_json::Value>();
    assert!(body["token"].is_null());
    let share_id = Uuid::parse_str(body["id"].as_str().unwrap()).unwrap();

    // Grant row exists, active immediately, notified.
    let grant = share_grant::Entity::find()
        .filter(share_grant::Column::ShareId.eq(share_id))
        .one(&t.db)
        .await
        .unwrap()
        .expect("grant created");
    assert_eq!(grant.user_id, recipient.id);
    assert!(grant.revoked_at.is_none());
    assert!(grant.notified_at.is_some());

    // Live fan-in: connection principal now holds the share...
    let snapshot = t.websocket_connections.snapshot().await;
    let (_, principal, _) = snapshot
        .iter()
        .find(|(id, _, _)| *id == connection_id)
        .unwrap();
    assert!(principal.has_share(share_id));

    // ...and a share-granted broadcast was delivered.
    let message = rx.try_recv().expect("share-granted notification");
    let parsed: serde_json::Value = serde_json::from_str(&message).unwrap();
    assert_eq!(parsed["broadcast"]["type"], "share-granted");
    assert_eq!(parsed["broadcast"]["share_id"], share_id.to_string());

    // Recipient's delta now includes the granted note without any header.
    let response = t
        .server
        .get("/api/notes/sync")
        .add_header("Authorization", bearer(&t, &recipient))
        .await;
    assert_eq!(
        delta_ids(&response.json::<serde_json::Value>()),
        vec![note.id.to_string()]
    );
}

#[tokio::test]
async fn grant_endpoint_owner_only() {
    let t = setup().await;
    let owner = create_user(&t.db, "owner").await;
    let recipient = create_user(&t.db, "recipient").await;
    let stranger = create_user(&t.db, "stranger").await;
    let note = create_note(&t.db, &owner, "note", 1).await;
    let share = create_share_row(&t.db, &owner, "share_test_notes", note.id, Some("tok-g1")).await;

    let response = t
        .server
        .post(&format!("/api/shares/{}/grants", share.id))
        .add_header("Authorization", bearer(&t, &stranger))
        .json(&json!({ "user_id": recipient.id }))
        .await;
    assert_eq!(response.status_code(), 403);

    let response = t
        .server
        .post(&format!("/api/shares/{}/grants", share.id))
        .add_header("Authorization", bearer(&t, &owner))
        .json(&json!({ "user_id": recipient.id }))
        .await;
    assert_eq!(response.status_code(), 201);

    // Unknown recipient → 422.
    let response = t
        .server
        .post(&format!("/api/shares/{}/grants", share.id))
        .add_header("Authorization", bearer(&t, &owner))
        .json(&json!({ "user_id": Uuid::new_v4() }))
        .await;
    assert_eq!(response.status_code(), 422);
}

#[tokio::test]
async fn revoke_share_fans_out_to_all_holders() {
    let t = setup().await;
    let owner = create_user(&t.db, "owner").await;
    let recipient = create_user(&t.db, "recipient").await;
    let note = create_note(&t.db, &owner, "note", 1).await;
    let share = create_share_row(&t.db, &owner, "share_test_notes", note.id, Some("tok-r1")).await;
    create_grant_row(&t.db, &share, &recipient).await;
    let active_share = ActiveShare::from(&share);

    // One authenticated connection (grant) and one anonymous (link).
    let (user_tx, mut user_rx) = tokio::sync::mpsc::unbounded_channel();
    t.websocket_connections
        .register_connection(
            Principal {
                user: Some(recipient.clone()),
                shares: vec![active_share.clone()],
            },
            user_tx,
        )
        .await;
    let (anon_tx, mut anon_rx) = tokio::sync::mpsc::unbounded_channel();
    t.websocket_connections
        .register_connection(
            Principal {
                user: None,
                shares: vec![active_share.clone()],
            },
            anon_tx,
        )
        .await;

    let response = t
        .server
        .delete(&format!("/api/shares/{}", share.id))
        .add_header("Authorization", bearer(&t, &owner))
        .await;
    assert_eq!(response.status_code(), 204);

    let stored = share::Entity::find_by_id(share.id)
        .one(&t.db)
        .await
        .unwrap()
        .unwrap();
    assert!(stored.revoked_at.is_some());

    // Both connections lost the share and were notified.
    for (_, principal, _) in t.websocket_connections.snapshot().await {
        assert!(!principal.has_share(share.id));
    }
    for rx in [&mut user_rx, &mut anon_rx] {
        let message = rx.try_recv().expect("share-revoked notification");
        let parsed: serde_json::Value = serde_json::from_str(&message).unwrap();
        assert_eq!(parsed["broadcast"]["type"], "share-revoked");
    }

    // The link token no longer grants delta access.
    let response = t
        .server
        .get("/api/notes/sync")
        .add_header(SHARE_TOKEN_HEADER, "tok-r1")
        .await;
    assert!(delta_ids(&response.json::<serde_json::Value>()).is_empty());
}

#[tokio::test]
async fn revoke_single_grant_leaves_link_alive() {
    let t = setup().await;
    let owner = create_user(&t.db, "owner").await;
    let recipient = create_user(&t.db, "recipient").await;
    let note = create_note(&t.db, &owner, "note", 1).await;
    let share = create_share_row(&t.db, &owner, "share_test_notes", note.id, Some("tok-r2")).await;
    create_grant_row(&t.db, &share, &recipient).await;
    let active_share = ActiveShare::from(&share);

    let (user_tx, _user_rx) = tokio::sync::mpsc::unbounded_channel();
    let user_conn = t
        .websocket_connections
        .register_connection(
            Principal {
                user: Some(recipient.clone()),
                shares: vec![active_share.clone()],
            },
            user_tx,
        )
        .await;
    let (anon_tx, _anon_rx) = tokio::sync::mpsc::unbounded_channel();
    let anon_conn = t
        .websocket_connections
        .register_connection(
            Principal {
                user: None,
                shares: vec![active_share.clone()],
            },
            anon_tx,
        )
        .await;

    let response = t
        .server
        .delete(&format!("/api/shares/{}/grants/{}", share.id, recipient.id))
        .add_header("Authorization", bearer(&t, &owner))
        .await;
    assert_eq!(response.status_code(), 204);

    // Recipient's connection lost the share; the anonymous link viewer kept it.
    for (id, principal, _) in t.websocket_connections.snapshot().await {
        if id == user_conn {
            assert!(!principal.has_share(share.id));
        } else if id == anon_conn {
            assert!(principal.has_share(share.id));
        }
    }

    // Recipient's delta is empty; the link still works.
    let response = t
        .server
        .get("/api/notes/sync")
        .add_header("Authorization", bearer(&t, &recipient))
        .await;
    assert!(delta_ids(&response.json::<serde_json::Value>()).is_empty());

    let response = t
        .server
        .get("/api/notes/sync")
        .add_header(SHARE_TOKEN_HEADER, "tok-r2")
        .await;
    assert_eq!(
        delta_ids(&response.json::<serde_json::Value>()),
        vec![note.id.to_string()]
    );
}

#[tokio::test]
async fn list_shares_never_exposes_token_hash() {
    let t = setup().await;
    let owner = create_user(&t.db, "owner").await;
    let other = create_user(&t.db, "other").await;
    let note = create_note(&t.db, &owner, "note", 1).await;
    let share = create_share_row(&t.db, &owner, "share_test_notes", note.id, Some("tok-l1")).await;
    create_grant_row(&t.db, &share, &other).await;
    // A share owned by someone else must not appear.
    let other_note = create_note(&t.db, &other, "other note", 2).await;
    create_share_row(&t.db, &other, "share_test_notes", other_note.id, None).await;

    let response = t
        .server
        .get("/api/shares")
        .add_header("Authorization", bearer(&t, &owner))
        .await;
    assert_eq!(response.status_code(), 200);
    let body = response.json::<serde_json::Value>();
    let items = body.as_array().unwrap();
    assert_eq!(items.len(), 1);
    assert_eq!(items[0]["id"], share.id.to_string());
    assert_eq!(items[0]["has_link"], true);
    assert_eq!(items[0]["grants"].as_array().unwrap().len(), 1);

    // Neither the hash nor the raw token appears anywhere in the response.
    let raw_body = response.text();
    assert!(!raw_body.contains(&hash_token("tok-l1")));
    assert!(!raw_body.contains("token_hash"));
}

// ---------------------------------------------------------------------------
// Connections: live principal mutation
// ---------------------------------------------------------------------------

#[tokio::test]
async fn add_share_to_user_targets_only_that_users_connections() {
    let t = setup().await;
    let recipient = create_user(&t.db, "recipient").await;
    let bystander = create_user(&t.db, "bystander").await;

    let (recipient_tx, mut recipient_rx) = tokio::sync::mpsc::unbounded_channel();
    let recipient_conn = t
        .websocket_connections
        .register_connection(Principal::from_user_model(recipient.clone()), recipient_tx)
        .await;
    let (bystander_tx, mut bystander_rx) = tokio::sync::mpsc::unbounded_channel();
    t.websocket_connections
        .register_connection(Principal::from_user_model(bystander.clone()), bystander_tx)
        .await;

    let share = ActiveShare {
        id: Uuid::new_v4(),
        entity_type: "share_test_notes".to_string(),
        entity_id: Uuid::new_v4(),
        permission: share::SharePermission::Read,
    };
    let updated = t
        .websocket_connections
        .add_share_to_user(recipient.id, share.clone())
        .await;
    assert_eq!(updated, 1);

    assert!(recipient_rx.try_recv().is_ok());
    assert!(bystander_rx.try_recv().is_err());

    for (id, principal, _) in t.websocket_connections.snapshot().await {
        assert_eq!(principal.has_share(share.id), id == recipient_conn);
    }

    // subscribe/unsubscribe mutate a single connection.
    t.websocket_connections
        .unsubscribe_share(recipient_conn, share.id)
        .await;
    let snapshot = t.websocket_connections.snapshot().await;
    assert!(snapshot.iter().all(|(_, p, _)| !p.has_share(share.id)));
}
