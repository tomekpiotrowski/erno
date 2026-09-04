/**
 * Wire types and normalisation for error reporting.
 *
 * Docs: docs/src/content/docs/app/error-reporting.md
 */
import { HttpErrorResponse } from '@angular/common/http';

/** Severity of a report. */
export type ErnoErrorLevel = 'warning' | 'error' | 'fatal';

/** One frame of a parsed stack trace. */
export interface ErnoStackFrame {
  function?: string;
  file?: string;
  line?: number;
  column?: number;
}

/** A report in the shape the collector accepts. */
export interface ErnoErrorReport {
  type: string;
  message: string;
  level: ErnoErrorLevel;
  timestamp?: string;
  stack?: string;
  frames?: ErnoStackFrame[];
  /** Explicit grouping override. The collector namespaces it by source. */
  fingerprint?: string[];
  context?: Record<string, unknown>;
}

/** A batch as posted to the collector. */
export interface ErnoErrorEnvelope {
  events: ErnoErrorReport[];
  release?: string;
  environment?: string;
  sdk: { name: string; version: string };
}


/** One OTLP attribute, string-valued. */
interface OtlpAttribute {
  key: string;
  value: { stringValue: string };
}

const OTLP_SEVERITY: Record<ErnoErrorLevel, number> = {
  warning: 13,
  error: 17,
  fatal: 21,
};

/**
 * The envelope as an OTLP/JSON logs request — the wire form the collector
 * accepts. Errors ride OTLP like every other signal: `exception.*` semconv
 * attributes plus the lossless `erno.frames`, release and environment on the
 * resource, and the browser token as a bearer.
 */
export function otlpLogsFromEnvelope(envelope: ErnoErrorEnvelope): unknown {
  const attr = (key: string, value: string): OtlpAttribute => ({
    key,
    value: { stringValue: value },
  });
  const records = envelope.events.map((event) => {
    const attributes: OtlpAttribute[] = [attr('exception.type', event.type)];
    if (event.stack) {
      attributes.push(attr('exception.stacktrace', event.stack));
    }
    if (event.frames?.length) {
      attributes.push(attr('erno.frames', JSON.stringify(event.frames)));
    }
    if (event.fingerprint?.length) {
      attributes.push(attr('erno.fingerprint', JSON.stringify(event.fingerprint)));
    }
    for (const [key, value] of Object.entries(event.context ?? {})) {
      const mapped = key === 'file' ? 'code.filepath' : key;
      attributes.push(attr(mapped, typeof value === 'string' ? value : JSON.stringify(value)));
    }
    const nanos = event.timestamp
      ? String(Date.parse(event.timestamp)) + '000000'
      : String(Date.now()) + '000000';
    return {
      timeUnixNano: nanos,
      severityNumber: OTLP_SEVERITY[event.level] ?? 17,
      severityText: event.level,
      body: { stringValue: event.message },
      attributes,
    };
  });

  const resourceAttributes: OtlpAttribute[] = [
    attr('telemetry.sdk.name', envelope.sdk.name),
    attr('telemetry.sdk.version', envelope.sdk.version),
  ];
  if (envelope.release) {
    resourceAttributes.push(attr('service.version', envelope.release));
  }
  if (envelope.environment) {
    resourceAttributes.push(attr('deployment.environment.name', envelope.environment));
  }
  return {
    resourceLogs: [
      {
        resource: { attributes: resourceAttributes },
        scopeLogs: [{ logRecords: records }],
      },
    ],
  };
}

/** Frames kept per report; the collector caps this too. */
const MAX_FRAMES = 50;

/**
 * Parse a stack string into frames.
 *
 * Handles both the V8 form (`at fn (file:line:col)`) and the
 * Safari/Firefox form (`fn@file:line:col`). Best-effort by design: an
 * unparseable stack still yields a report, grouped by message instead.
 */
export function parseStack(stack: string | undefined): ErnoStackFrame[] {
  if (!stack) {
    return [];
  }

  const frames: ErnoStackFrame[] = [];
  for (const raw of stack.split('\n')) {
    if (frames.length >= MAX_FRAMES) {
      break;
    }
    const line = raw.trim();

    // V8 with a function name: "at Foo.bar (https://host/main.js:12:5)"
    const named = /^at\s+(.+?)\s+\((.+?):(\d+):(\d+)\)$/.exec(line);
    if (named) {
      frames.push({
        function: named[1],
        file: named[2],
        line: Number(named[3]),
        column: Number(named[4]),
      });
      continue;
    }

    // V8 without one: "at https://host/main.js:12:5"
    const anonymous = /^at\s+(.+?):(\d+):(\d+)$/.exec(line);
    if (anonymous) {
      frames.push({
        file: anonymous[1],
        line: Number(anonymous[2]),
        column: Number(anonymous[3]),
      });
      continue;
    }

    // Safari / Firefox: "foo@https://host/main.js:12:5"
    const at = /^(.*?)@(.+?):(\d+):(\d+)$/.exec(line);
    if (at) {
      frames.push({
        function: at[1] || undefined,
        file: at[2],
        line: Number(at[3]),
        column: Number(at[4]),
      });
    }
  }
  return frames;
}

/**
 * Turn anything that can be thrown into a report.
 *
 * JavaScript lets you throw any value at all, so this has to cope with far
 * more than `Error`: rejected promises carrying strings, DOM exceptions,
 * Angular's `HttpErrorResponse`, and plain objects.
 */
export function normalizeError(error: unknown): ErnoErrorReport {
  if (error instanceof HttpErrorResponse) {
    return {
      type: 'HttpErrorResponse',
      message: `${error.status} ${error.statusText ?? ''} ${error.url ?? ''}`.trim(),
      level: 'error',
      context: { status: error.status, url: error.url ?? undefined },
    };
  }

  if (error instanceof Error) {
    return {
      type: error.name || 'Error',
      message: error.message || String(error),
      level: 'error',
      stack: error.stack,
      frames: parseStack(error.stack),
    };
  }

  // `unhandledrejection` and `error` events wrap the real value.
  if (typeof ErrorEvent !== 'undefined' && error instanceof ErrorEvent) {
    return normalizeError(error.error ?? error.message);
  }

  if (typeof error === 'string') {
    return { type: 'Error', message: error, level: 'error' };
  }

  if (error && typeof error === 'object') {
    // A thrown object with a message field is common in library code.
    const candidate = error as { name?: unknown; message?: unknown };
    if (typeof candidate.message === 'string') {
      return {
        type: typeof candidate.name === 'string' ? candidate.name : 'Error',
        message: candidate.message,
        level: 'error',
      };
    }
    try {
      return { type: 'Error', message: JSON.stringify(error), level: 'error' };
    } catch {
      // Circular structure.
      return { type: 'Error', message: '[unserialisable thrown value]', level: 'error' };
    }
  }

  return { type: 'Error', message: String(error), level: 'error' };
}
