---
title: Authorization
description: Policy-based authorization for SeaORM entities
sidebar:
  order: 12
---

> **Source**: `api/src/policy/`

Erno uses a policy pattern for authorization — one `Policy<E>` struct per entity, similar to [Pundit](https://github.com/varvet/pundit) in Rails. Policies control both individual record checks (`can_read`, `can_update`, `can_delete`) and query filtering (`readable`).

## Implementing a policy

```rust
use erno::policy::Policy;
use sea_orm::QueryFilter;

pub struct PostPolicy {
    pub user_id: Uuid,
}

impl Policy<post::Entity> for PostPolicy {
    fn can_read(&self, post: &post::Model) -> bool {
        post.user_id == self.user_id
    }

    fn readable(&self, query: Select<post::Entity>) -> Select<post::Entity> {
        query.filter(post::Column::UserId.eq(self.user_id))
    }
}
```

## Policy trait methods

| Method | Default | Description |
|--------|---------|-------------|
| `can_read(&self, entity) -> bool` | — | Must implement. Per-record read check. |
| `readable(&self, query) -> Select<E>` | — | Must implement. Filters a query to readable records (scope). |
| `can_create(&self) -> bool` | `false` | Check before inserting. |
| `can_update(&self, entity) -> bool` | delegates to `can_read` | Check before updating. |
| `can_delete(&self, entity) -> bool` | delegates to `can_update` | Check before deleting. |
| `can_view(&self, entity, view_name) -> bool` | delegates to `can_read` | Check for view-specific access (e.g. `"detailed"` view vs `"list"` view). |

Override only the methods where your access rules differ from the defaults.

## Using a policy in a handler

```rust
async fn get_post(
    State(app): State<App>,
    CurrentUser { user, .. }: CurrentUser,
    Path(post_id): Path<Uuid>,
) -> impl IntoResponse {
    let policy = PostPolicy { user_id: user.id };

    let post = post::Entity::find_by_id(post_id)
        .one(&app.db)
        .await?
        .ok_or(StatusCode::NOT_FOUND)?;

    if !policy.can_read(&post) {
        return Err(StatusCode::FORBIDDEN);
    }

    Ok(Json(post))
}

async fn list_posts(
    State(app): State<App>,
    CurrentUser { user, .. }: CurrentUser,
) -> impl IntoResponse {
    let policy = PostPolicy { user_id: user.id };

    let posts = policy
        .readable(post::Entity::find())
        .all(&app.db)
        .await?;

    Ok(Json(posts))
}
```

## Integration with sync

The [Sync](../sync) module requires a policy for each syncable entity. The policy's `readable` scope determines which connected users receive WebSocket push events for a given change — only users for whom the entity would appear in their `readable` query are notified.

For sync, the policy must also implement `FromUser` so the sync worker can instantiate it per connected user:

```rust
use erno::sync::from_user::FromUser;

impl FromUser for PostPolicy {
    fn from_user(user: &user::Model) -> Self {
        PostPolicy { user_id: user.id }
    }
}
```
