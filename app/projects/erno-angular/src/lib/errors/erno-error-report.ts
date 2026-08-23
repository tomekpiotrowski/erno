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
