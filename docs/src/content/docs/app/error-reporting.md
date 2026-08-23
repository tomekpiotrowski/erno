---
title: Error reporting
description: Report client-side errors from an Ionic or Angular app to an Erno monitoring collector.
sidebar:
  order: 9
---

> **Source**: `app/projects/erno-angular/src/lib/errors/`

`provideErno` can install a global `ErrorHandler` that reports uncaught
exceptions, unhandled promise rejections and `window` errors to a
[monitoring collector](/monitoring/error-reporting/).

## Enabling it

```ts
import { provideErno } from 'erno-angular';

bootstrapApplication(AppComponent, {
  providers: [
    provideErno({
      baseUrl: 'https://api.example.com',
      wsUrl: 'wss://api.example.com/ws',
      errorReporting: {
        // The collector is a different deployment on a different host.
        endpoint: 'https://monitoring.example.com/api/errors',
        key: 'the-public-browser-token',
        release: '1.4.2',
        environment: 'production',
      },
    }),
  ],
});
```

**Without `key`, reporting stays off.** An application never sends diagnostics
anywhere by accident, and the `ErrorHandler` is a pass-through that only logs.

The key ships inside your JavaScript bundle and is therefore **public**. That
is unavoidable for browser reporting and is accounted for in the collector's
design — see [the auth section](/monitoring/error-reporting/#authentication).

## Options

| Option | Default | Purpose |
|---|---|---|
| `enabled` | `true` | Master switch |
| `key` | — | Public ingest token; without it reporting is off |
| `endpoint` | `${baseUrl}/api/errors` | Absolute collector URL |
| `release` | — | Build version, so an issue ties to a deploy |
| `environment` | — | Deployment environment |
| `sampleRate` | `1` | Fraction of errors reported |
| `maxQueueSize` | `50` | Reports held while offline |
| `maxReportsPerMinute` | `20` | Rolling cap |
| `dedupeWindowMs` | `5000` | Identical errors counted, not resent |
| `sendUser` | `true` | Attach the signed-in user's id and email |
| `ignoreMessages` | `[]` | Strings or regexes never reported |
| `beforeSend` | — | Last chance to redact or veto; return `null` to drop |

## Reporting manually

```ts
import { ErnoErrorReporterService } from 'erno-angular';

const reporter = inject(ErnoErrorReporterService);

reporter.report(error, { deckId: deck.id });
reporter.captureMessage('cache rebuild took unusually long', 'warning');
await reporter.flush();
```

## What is sent

The error's type, message and stack; parsed frames; the page URL with its
fragment removed and secrets stripped from the query; the user agent; your
`release` and `environment`; and the signed-in user's id and email when
`sendUser` is on.

Never sent: cookies, `localStorage` or `sessionStorage`, request bodies,
response bodies, or form values.

Scrubbing runs before anything leaves the device — JWTs, `Bearer`/`Basic`
headers, card numbers and long opaque strings become `[redacted]`, and keys
like `password` or `api_key` are redacted wholesale. UUIDs are deliberately
kept: they are identifiers rather than credentials, and they are usually what
makes an error traceable to a record. The collector repeats the same pass
before storing anything.

`beforeSend` is the hook for anything domain-specific:

```ts
beforeSend: (report) => {
  if (report.message.includes('internal-only')) {
    return null; // drop it
  }
  delete report.context?.['patientName'];
  return report;
};
```

## Noise control

Three mechanisms stop a render loop from flooding the collector:

- **Dedupe** — an identical error inside `dedupeWindowMs` is counted rather
  than sent, and the count rides along on the next batch as
  `context.duplicates`.
- **Rate cap** — at most `maxReportsPerMinute` distinct reports; the overflow
  is recorded as `context.dropped_locally`.
- **Sampling** — `sampleRate` below 1 reports a fraction.

## Offline behaviour

Reports are buffered in memory (bounded by `maxQueueSize`, dropping oldest
first) and flushed when `ErnoNetworkService` reports the connection back, when
the app resumes, or on `pagehide`. Sends use exponential backoff with jitter.

A `4xx` other than `429` drops the batch rather than retrying it — the
collector will never accept that payload, and retrying would be a permanent
loop.

## Not reporting its own failures

Three independent guards, because this is the one genuine feedback loop on the
client:

1. The reporter posts with raw `HttpClient`, never `ErnoHttpService` — which
   would raise a toast and swallow the failure into `EMPTY`.
2. A re-entrancy flag, plus a check that drops any report mentioning the ingest
   endpoint.
3. `ErnoHttpInterceptor` skips its 401 recovery path for the ingest URL.
   Without that, an ingest 401 would trigger a token refresh, a failing refresh
   would throw, the `ErrorHandler` would report it, and that report would POST
   to ingest again.

## Using your own ErrorHandler

`provideErno` provides `ErrorHandler`, so supply yours afterwards to win:

```ts
providers: [
  provideErno({ ... }),
  { provide: ErrorHandler, useClass: MyErrorHandler },
];
```

`ErnoErrorHandler` always calls `console.error` after reporting, so local
development is unaffected by turning reporting on.
