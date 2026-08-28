/**
 * Client-side error reporting.
 *
 * Docs: docs/src/content/docs/app/error-reporting.md
 *
 * The collector lives on separate infrastructure, so every failure mode here
 * is a network failure mode. The governing rule is the same as on the server:
 * **reporting must never be able to hurt the application**. Hence a bounded
 * buffer, a dedupe window, a rate cap, backoff, and three independent guards
 * against reporting a failure of reporting.
 */
import { HttpClient } from '@angular/common/http';
import { Inject, Injectable, OnDestroy, Optional } from '@angular/core';
import { Subscription } from 'rxjs';

import { ErnoAuthService } from '../auth/erno-auth.service';
import { ErnoAppStateService } from '../app-state/erno-app-state.service';
import { ErnoNetworkService } from '../network/erno-network.service';
import { ERNO_CONFIG, ErnoConfig, ErnoErrorReportingConfig } from '../erno.config';
import { nextDelayMs } from './backoff';
import { ErnoErrorBuffer, MemoryErrorBuffer } from './erno-error-buffer';
import {
  ErnoErrorEnvelope,
  ErnoErrorLevel,
  ErnoErrorReport,
  normalizeError,
} from './erno-error-report';
import { scrubContext, scrubText, scrubUrl } from './scrub';

const SDK_NAME = 'erno-angular';
const SDK_VERSION = '0.1.0';
/** Debounce between an error arriving and a flush being attempted. */
const FLUSH_DEBOUNCE_MS = 2_000;
/** Attempts for one batch before it is abandoned. */
const MAX_ATTEMPTS = 4;
/** Entries kept in the dedupe map before the oldest is evicted. */
const DEDUPE_KEYS = 100;

interface Defaults {
  enabled: boolean;
  sampleRate: number;
  maxQueueSize: number;
  maxReportsPerMinute: number;
  dedupeWindowMs: number;
  sendUser: boolean;
}

const DEFAULTS: Defaults = {
  enabled: true,
  sampleRate: 1,
  maxQueueSize: 50,
  maxReportsPerMinute: 20,
  dedupeWindowMs: 5_000,
  sendUser: true,
};

@Injectable()
export class ErnoErrorReporterService implements OnDestroy {
  private readonly options: ErnoErrorReportingConfig;
  private readonly settings: Defaults;
  private readonly endpoint: string;
  private readonly buffer: ErnoErrorBuffer;

  private installed = false;
  private sending = false;
  private attempt = 0;
  private flushTimer: ReturnType<typeof setTimeout> | null = null;

  /** Suppressed duplicates and rate-limited drops, reported with the next send. */
  private duplicates = 0;
  private droppedLocally = 0;

  private readonly recent = new Map<string, number>();
  private sentTimestamps: number[] = [];
  private readonly subscriptions = new Subscription();
  private listeners: Array<() => void> = [];

  constructor(
    @Inject(ERNO_CONFIG) private readonly config: ErnoConfig,
    private readonly http: HttpClient,
    @Optional() private readonly auth: ErnoAuthService | null,
    @Optional() private readonly network: ErnoNetworkService | null,
    @Optional() private readonly appState: ErnoAppStateService | null,
  ) {
    this.options = config.errorReporting ?? {};
    this.settings = {
      enabled: this.options.enabled ?? DEFAULTS.enabled,
      sampleRate: this.options.sampleRate ?? DEFAULTS.sampleRate,
      maxQueueSize: this.options.maxQueueSize ?? DEFAULTS.maxQueueSize,
      maxReportsPerMinute: this.options.maxReportsPerMinute ?? DEFAULTS.maxReportsPerMinute,
      dedupeWindowMs: this.options.dedupeWindowMs ?? DEFAULTS.dedupeWindowMs,
      sendUser: this.options.sendUser ?? DEFAULTS.sendUser,
    };
    // An absolute URL, not `baseUrl + path`: the collector is a different
    // deployment on a different host.
    this.endpoint = this.options.endpoint ?? `${config.baseUrl}/api/errors`;
    this.buffer = new MemoryErrorBuffer(this.settings.maxQueueSize);
  }

  /** Whether reports will actually be sent. */
  get active(): boolean {
    return this.settings.enabled && Boolean(this.options.key);
  }

  /**
   * Attach global listeners. Idempotent, and safe to call outside a browser.
   *
   * Registered from an app initializer so the listeners exist from startup
   * rather than whenever something first injects this service.
   */
  install(): void {
    if (this.installed || !this.active || typeof window === 'undefined') {
      return;
    }
    this.installed = true;

    // `addEventListener`, never `window.onerror = ...`, which would silently
    // replace any handler the host application had already set.
    const onError = (event: ErrorEvent) => this.report(event.error ?? event.message);
    const onRejection = (event: PromiseRejectionEvent) =>
      this.report(event.reason, { unhandledRejection: true });
    const onHide = () => void this.flush();

    window.addEventListener('error', onError);
    window.addEventListener('unhandledrejection', onRejection);
    window.addEventListener('pagehide', onHide);

    this.listeners = [
      () => window.removeEventListener('error', onError),
      () => window.removeEventListener('unhandledrejection', onRejection),
      () => window.removeEventListener('pagehide', onHide),
    ];

    if (this.network) {
      this.subscriptions.add(this.network.online$.subscribe(() => void this.flush()));
    }
    if (this.appState) {
      this.subscriptions.add(this.appState.resumed$.subscribe(() => void this.flush()));
    }
  }

  /** Report an error. Never throws. */
  report(error: unknown, context?: Record<string, unknown>): void {
    try {
      this.enqueue(error, context, 'error');
    } catch {
      // Reporting must not become the failure it is reporting on.
    }
  }

  /** Report a message with no exception behind it. */
  captureMessage(message: string, level: ErnoErrorLevel = 'warning'): void {
    try {
      this.enqueue(message, undefined, level);
    } catch {
      /* see above */
    }
  }

  private enqueue(error: unknown, context: Record<string, unknown> | undefined, level: ErnoErrorLevel): void {
    if (!this.active) {
      return;
    }
    // Guard 1: never report a failure that happened while reporting.
    if (this.sending) {
      return;
    }

    const report = normalizeError(error);
    report.level = level;

    // Guard 2: never report anything about the ingest endpoint itself.
    if (this.mentionsIngest(report)) {
      return;
    }

    if (this.options.ignoreMessages?.some((m) => matches(m, report.message))) {
      return;
    }
    if (Math.random() >= this.settings.sampleRate) {
      return;
    }

    const now = Date.now();
    if (this.isDuplicate(report, now)) {
      this.duplicates++;
      return;
    }
    if (this.isRateLimited(now)) {
      this.droppedLocally++;
      return;
    }

    report.timestamp = new Date(now).toISOString();
    report.message = scrubText(report.message);
    if (report.stack) {
      report.stack = scrubText(report.stack);
    }
    report.frames = report.frames?.map((frame) => ({
      ...frame,
      file: frame.file ? scrubUrl(frame.file) : frame.file,
    }));
    report.context = this.buildContext(report.context, context);

    const finished = this.options.beforeSend ? this.options.beforeSend(report) : report;
    if (!finished) {
      return;
    }

    void this.buffer.push(finished).then(() => this.scheduleFlush());
  }

  private mentionsIngest(report: ErnoErrorReport): boolean {
    const needle = this.endpoint;
    if (report.message.includes(needle)) {
      return true;
    }
    const url = report.context?.['url'];
    return typeof url === 'string' && url.includes(needle);
  }

  private isDuplicate(report: ErnoErrorReport, now: number): boolean {
    const key = `${report.type}|${report.message}|${report.frames?.[0]?.file ?? ''}`;
    const last = this.recent.get(key);
    if (last !== undefined && now - last < this.settings.dedupeWindowMs) {
      return true;
    }
    this.recent.set(key, now);
    if (this.recent.size > DEDUPE_KEYS) {
      const oldest = this.recent.keys().next().value;
      if (oldest !== undefined) {
        this.recent.delete(oldest);
      }
    }
    return false;
  }

  private isRateLimited(now: number): boolean {
    this.sentTimestamps = this.sentTimestamps.filter((t) => now - t < 60_000);
    if (this.sentTimestamps.length >= this.settings.maxReportsPerMinute) {
      return true;
    }
    this.sentTimestamps.push(now);
    return false;
  }

  private buildContext(
    fromError: Record<string, unknown> | undefined,
    extra: Record<string, unknown> | undefined,
  ): Record<string, unknown> {
    const context: Record<string, unknown> = {
      ...scrubContext(fromError),
      ...scrubContext(extra),
    };

    if (typeof location !== 'undefined') {
      context['url'] = scrubUrl(location.href);
    }
    if (typeof navigator !== 'undefined') {
      context['user_agent'] = navigator.userAgent;
    }
    return context;
  }

  private scheduleFlush(): void {
    if (this.flushTimer !== null) {
      return;
    }
    this.flushTimer = setTimeout(() => {
      this.flushTimer = null;
      void this.flush();
    }, FLUSH_DEBOUNCE_MS);
  }

  /** Send everything buffered. Resolves when the attempt finishes. */
  async flush(): Promise<void> {
    if (!this.active || this.sending) {
      return;
    }
    // Nothing will get through while offline; keep it buffered instead of
    // burning an attempt and a backoff step.
    if (this.network && !this.network.connected) {
      return;
    }

    const reports = await this.buffer.take(this.settings.maxQueueSize);
    if (!reports.length) {
      return;
    }

    this.sending = true;
    try {
      await this.send(reports);
      this.attempt = 0;
    } catch (retryable) {
      if (retryable === RETRYABLE) {
        this.attempt++;
        if (this.attempt < MAX_ATTEMPTS) {
          await this.buffer.requeue(reports);
          const delay = nextDelayMs(this.attempt);
          this.flushTimer = setTimeout(() => {
            this.flushTimer = null;
            void this.flush();
          }, delay);
        } else {
          // Give up on this batch rather than retrying for ever; the counters
          // in `context` record that reports were lost.
          this.attempt = 0;
        }
      }
      // A non-retryable rejection means the collector will never accept this
      // payload, so the batch is dropped deliberately.
    } finally {
      this.sending = false;
    }
  }

  private send(reports: ErnoErrorReport[]): Promise<void> {
    const events = reports.map((report) => this.withUser(report));
    // Suppressed duplicates and rate-limited drops are attached here rather
    // than when they happen: at enqueue time the batch they belong to has not
    // been assembled yet, and the counts would ride on some later, unrelated
    // report instead.
    if (events.length && (this.duplicates > 0 || this.droppedLocally > 0)) {
      events[0] = {
        ...events[0],
        context: {
          ...events[0].context,
          ...(this.duplicates > 0 ? { duplicates: this.duplicates } : {}),
          ...(this.droppedLocally > 0 ? { dropped_locally: this.droppedLocally } : {}),
        },
      };
      this.duplicates = 0;
      this.droppedLocally = 0;
    }

    const envelope: ErnoErrorEnvelope = {
      events,
      release: this.options.release,
      environment: this.options.environment,
      sdk: { name: SDK_NAME, version: SDK_VERSION },
    };

    return new Promise((resolve, reject) => {
      // Raw HttpClient, deliberately not ErnoHttpService: that funnels failures
      // into a user-facing toast and swallows them into EMPTY.
      this.http
        .post(this.endpoint, envelope, {
          headers: { 'X-Erno-Ingest-Key': this.options.key ?? '' },
        })
        .subscribe({
          next: () => resolve(),
          error: (error: { status?: number }) => {
            const status = error?.status ?? 0;
            // A 4xx other than 429 means the payload itself is unacceptable;
            // retrying it would be a permanent hot loop.
            const retryable = status === 0 || status === 429 || status >= 500;
            reject(retryable ? RETRYABLE : PERMANENT);
          },
        });
    });
  }

  private withUser(report: ErnoErrorReport): ErnoErrorReport {
    const user = this.auth?.currentUser();
    if (!this.settings.sendUser || !user) {
      return report;
    }
    return {
      ...report,
      context: { ...report.context, user_id: user.id, user_email: user.email },
    };
  }

  ngOnDestroy(): void {
    if (this.flushTimer !== null) {
      clearTimeout(this.flushTimer);
    }
    this.subscriptions.unsubscribe();
    for (const remove of this.listeners) {
      remove();
    }
    this.listeners = [];
  }
}

const RETRYABLE = Symbol('retryable');
const PERMANENT = Symbol('permanent');

function matches(pattern: string | RegExp, message: string): boolean {
  return typeof pattern === 'string' ? message.includes(pattern) : pattern.test(message);
}
