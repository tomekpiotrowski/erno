---
title: Devtools
description: Dev overlay for session, sync, Dexie, mock email, jobs, and toast alerts
sidebar:
  order: 8
---

Local development helpers ship with `erno-angular`: a floating overlay for the running app (session, Dexie, sync, mail, jobs), services that hit the API’s `/dev/*` routes, and an optional toast queue.

## Devtools overlay

Add the component once in a root template (development builds only):

```html
<erno-devtools></erno-devtools>
```

The overlay is a Nocturne panel docked to the bottom-right. A health dot in the header (and on the collapsed pill) turns red when sync is in error or a job has failed, amber while work is in flight, and green otherwise. Collapse it with **—**; **tall** / **short** grows the body. The tab strip scrolls when it does not fit.

| Tab | What it shows |
|-----|----------------|
| **Status** | Network, WebSocket, sync (with the last pull error), API liveness, and queue counts. **Re-sync** pulls every entity. **simulate offline** drives `ErnoNetworkService` so sync and the socket take the offline path without Chrome’s network panel. |
| **Auth** | Current user, access-token expiry / `ver` claim, refresh-token presence (never the raw secret). Sign in as the `erno dev` seed user (`dev@example.com` / `password` — editable because projects override `[seed]`), logout, force refresh, **drop access** (deletes `erno_access_token` so the interceptor’s 401 path can be exercised), copy user id. |
| **Sync** | One row per registered entity (`lastSyncSeq`, last pull, last error). **reset cursor** writes seq 0 and pulls. Live log of the last 30 push events. |
| **Data** | Local Dexie databases. `provideErno` registers the `erno` DB (`syncMeta`, `pendingMutations`). Apps attach their own stores with `registerDevtoolsDatabase(db)`. Browse tables, filter JSON rows, copy a row, export a table as a JSON file, wipe a table or the whole database (local only). |
| **Emails** | Mock emails captured when the API uses `email.type = "mock"`; newest first, unread until opened. Verification mails get a **verify** button that calls `ErnoAuthService.verifyEmail`; password-reset mails open the reset URL. |
| **Jobs** | Recent jobs grouped by type, with run counts, filters (`all` / `attention` / `failed`), expand for individual runs, and **retry** on a failed job |

Tab badges show the inbox size, the number of job rows, `in` on Auth when signed in, and `!` on Status/Sync when sync is in error. **Clear all** empties the inbox or the job history depending on the open tab. The panel polls every few seconds so counts stay current while it is mounted.

Visibility is gated by Angular’s `isDevMode()` so production builds do not show the panel even if the tag remains in a template.

### Registering an app Dexie

`ErnoDatabaseService` only holds sync cursors and the (currently unused) pending-mutation queue. Domain rows live in the app’s own Dexie. Register that instance so it appears on the Data tab:

```typescript
import { registerDevtoolsDatabase } from 'erno-angular';

constructor(private todosDb: TodosDatabase) {
  registerDevtoolsDatabase(todosDb);
}
```

Do not open arbitrary names from `indexedDB.databases()` — opening a Dexie DB at the wrong version runs upgrades. Registration is explicit.

### Related services

| Service | Endpoints | Role |
|---------|-----------|------|
| `ErnoDevMailService` | `GET/DELETE /dev/emails` | List, delete one, or clear mock emails |
| `ErnoDevMailService.previewUrl(id)` | `GET /dev/emails/{id}/preview` | URL of the standalone preview page for one email |
| `ErnoDevJobsService` | `GET/DELETE /dev/jobs` | List/clear jobs for the Jobs tab |
| `ErnoDevJobsService.retry(id)` | `POST /dev/jobs/{id}/retry` | Re-queue a failed job |

### Opening an email

Clicking a row in the Emails tab opens `/dev/emails/{id}/preview` in a new tab: a page with the envelope
metadata (subject, from, to, sent) above an iframe holding the message body. The body is served untouched
from `/dev/emails/{id}/body`, so the email's own `<style>` blocks and layout render exactly as a mail client
would show them. Plain-text-only messages are wrapped in a `<pre>`. Both routes exist only where the mock
inbox does — `email.type = "mock"` outside production.

```typescript
import { ErnoDevMailService } from 'erno-angular';

constructor(private mail: ErnoDevMailService) {}

refresh() {
  this.mail.list().subscribe(emails => console.log(emails));
}
```

Configure the API with mock transport in development:

```toml
[email]
type = "mock"
```

See [Email (API)](/api/email/).

## Alerts (`ErnoAlertsService`)

`ErnoAlertsService` queues Ionic toasts so concurrent messages play one after another:

```typescript
import { ErnoAlertsService } from 'erno-angular';

constructor(private alerts: ErnoAlertsService) {}

onSaved() {
  this.alerts.success('Saved');
}

onFail(err: unknown) {
  this.alerts.error('Something went wrong');
}
```

| Method | Default duration | Ionic color |
|--------|------------------|-------------|
| `success(message, duration?)` | 3000 ms | `success` |
| `info(message, duration?)` | 3000 ms | `primary` |
| `warn(message, duration?)` | 4000 ms | `warning` |
| `error(message, duration?)` | 5000 ms | `danger` |

Requires Ionic’s `ToastController` (present in apps scaffolded with `erno new`). Toasts appear at the top of the viewport.
