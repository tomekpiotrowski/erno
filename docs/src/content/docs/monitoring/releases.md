---
title: Releases
description: Record deploys so every other signal can be read against them.
sidebar:
  order: 2
---

> **Source**: `api/src/error_reporting/collector/releases.rs`

A deploy is the most common explanation for a change in behaviour, so recording
deploys is what turns "issues appeared at 14:02" into "that deploy introduced
them".

## Recording a deploy

```sh
curl -X POST https://monitoring.example.com/api/collector/releases \
  -H 'X-Erno-Ingest-Key: $SERVER_TOKEN' \
  -H 'content-type: application/json' \
  -d '{"version":"2.4.1","environment":"production","commit_sha":"'"$GITHUB_SHA"'","source":"github-actions"}'
```

Uses the **trusted server token**, not operator credentials — this is
machine-to-machine, and the public browser token must not be able to forge
deploys.

Re-posting the same `version` and `environment` updates the existing row rather
than creating a duplicate, so a re-run pipeline is harmless.

| Field | Required | Notes |
|---|---|---|
| `version` | yes | Matches the `release` your app reports with |
| `environment` | yes | `production`, `staging`, … |
| `commit_sha` | no | |
| `source` | no | Who recorded it |
| `deployed_at` | no | Defaults to now |

## What it gives you

The releases list shows, per deploy, how many error types were **first seen**
carrying that version. `first_release` is written only when an issue is
created, so this counts issues *born* in a release rather than merely seen
during it — which is the difference between "this deploy broke something" and
"this deploy was running when something broke".

Clicking the count opens the issues list filtered to that release, with the
status filter widened, because after a deploy you want to see everything it did
rather than only what is still open.

## Version matching

For any of this to line up, the `release` an application reports must equal the
`version` recorded here. The API reports its own `AppInfo::version`
automatically; browser apps take it from `errorReporting.release`, which should
be set from the same build metadata your pipeline posts.
