import { ErrorHandler, Injectable, Injector, inject } from '@angular/core';
import { CollectorApi } from './api';

// Docs: docs/src/content/docs/monitoring/error-reporting.md
//
// Deliberately small. This console does not depend on `erno-angular`, and it
// should not grow that dependency just to report its own crashes: it is always
// online with the collector (they are the same deployment), so there is no
// offline buffer, no backoff, and no circuit breaker to justify.
//
// It posts with the operator's Basic credentials via the existing interceptor,
// so no public ingest key is shipped in this bundle at all.

/** Reports suppressed within this window if they look identical. */
const DEDUPE_WINDOW_MS = 5_000;
/** Ceiling on reports per rolling minute, so a render loop cannot spam. */
const MAX_PER_MINUTE = 20;

interface StackFrame {
  function?: string;
  file?: string;
  line?: number;
  column?: number;
}

/** Best-effort parse of V8 and Safari/Firefox stack formats. */
export function parseStack(stack: string | undefined): StackFrame[] {
  if (!stack) {
    return [];
  }
  const frames: StackFrame[] = [];
  for (const raw of stack.split('\n').slice(1, 51)) {
    const line = raw.trim();
    // V8: "at Foo.bar (https://host/main.js:12:5)"
    const v8 = /^at\s+(.+?)\s+\((.+?):(\d+):(\d+)\)$/.exec(line);
    if (v8) {
      frames.push({ function: v8[1], file: v8[2], line: +v8[3], column: +v8[4] });
      continue;
    }
    // V8 without a function name: "at https://host/main.js:12:5"
    const bare = /^at\s+(.+?):(\d+):(\d+)$/.exec(line);
    if (bare) {
      frames.push({ file: bare[1], line: +bare[2], column: +bare[3] });
      continue;
    }
    // Safari/Firefox: "foo@https://host/main.js:12:5"
    const at = /^(.*?)@(.+?):(\d+):(\d+)$/.exec(line);
    if (at) {
      frames.push({ function: at[1] || undefined, file: at[2], line: +at[3], column: +at[4] });
    }
  }
  return frames;
}

export function normalizeError(error: unknown): {
  type: string;
  message: string;
  stack?: string;
} {
  if (error instanceof Error) {
    return { type: error.name || 'Error', message: error.message, stack: error.stack };
  }
  if (typeof error === 'string') {
    return { type: 'Error', message: error };
  }
  try {
    return { type: 'Error', message: JSON.stringify(error) ?? String(error) };
  } catch {
    return { type: 'Error', message: String(error) };
  }
}

@Injectable({ providedIn: 'root' })
export class MonitoringErrorHandler implements ErrorHandler {
  // Injected lazily: resolving the API eagerly here would create a DI cycle
  // with HttpClient's own error paths.
  private readonly injector = inject(Injector);

  private readonly recent = new Map<string, number>();
  private sentTimestamps: number[] = [];
  private sending = false;

  handleError(error: unknown): void {
    try {
      this.report(error);
    } catch {
      // A broken reporter must never hide the application's own bug.
    }
    // Always, so developer ergonomics do not regress.
    console.error(error);
  }

  private report(error: unknown): void {
    if (this.sending) {
      return;
    }

    const normalized = normalizeError(error);
    const frames = parseStack(normalized.stack);

    // Never report a failure of reporting itself.
    const url = typeof location === 'undefined' ? '' : location.href;
    if (normalized.message.includes('/api/errors')) {
      return;
    }

    const key = `${normalized.type}|${normalized.message}|${frames[0]?.file ?? ''}`;
    const now = Date.now();

    const last = this.recent.get(key);
    if (last !== undefined && now - last < DEDUPE_WINDOW_MS) {
      return;
    }
    this.recent.set(key, now);
    if (this.recent.size > 100) {
      // Cheap eviction: the oldest inserted key.
      const oldest = this.recent.keys().next().value;
      if (oldest !== undefined) {
        this.recent.delete(oldest);
      }
    }

    this.sentTimestamps = this.sentTimestamps.filter((t) => now - t < 60_000);
    if (this.sentTimestamps.length >= MAX_PER_MINUTE) {
      return;
    }
    this.sentTimestamps.push(now);

    this.sending = true;
    this.injector
      .get(CollectorApi)
      .report({
        events: [
          {
            type: normalized.type,
            message: normalized.message,
            level: 'error',
            stack: normalized.stack,
            frames,
            context: { url: url.split('#')[0], user_agent: navigator.userAgent },
          },
        ],
        environment: location.hostname === 'localhost' ? 'development' : 'production',
        sdk: { name: 'erno-monitoring-ui', version: '0.1.0' },
      })
      .subscribe({
        next: () => {
          this.sending = false;
        },
        // Swallowed on purpose: rethrowing would re-enter handleError.
        error: () => {
          this.sending = false;
        },
      });
  }
}
