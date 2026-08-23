import { InjectionToken } from '@angular/core';
import { ErnoErrorReport } from './errors/erno-error-report';

/**
 * Client-side error reporting.
 *
 * Docs: docs/src/content/docs/app/error-reporting.md
 *
 * Absent, or without a `key`, reporting stays off — an application never sends
 * diagnostics anywhere by accident.
 */
export interface ErnoErrorReportingConfig {
  /** Master switch. Default true when the block is present. */
  enabled?: boolean;
  /**
   * Public ingest token. It ships inside the JS bundle and is therefore **not
   * a secret** — a speed bump against drive-by scanners, nothing more. The
   * real protections are the collector's rate limits and bounded queue.
   */
  key?: string;
  /**
   * Absolute URL of the collector's ingest endpoint. Defaults to
   * `${baseUrl}/api/errors`, but in a real deployment the collector is a
   * different host entirely.
   */
  endpoint?: string;
  /** Build version, so an issue can be tied to a deploy. */
  release?: string;
  /** Deployment environment. */
  environment?: string;
  /** Fraction of errors reported, 0..1. Default 1. */
  sampleRate?: number;
  /** Reports held while offline. Default 50. */
  maxQueueSize?: number;
  /** Rolling per-minute cap. Default 20. */
  maxReportsPerMinute?: number;
  /** Window in which an identical error is counted rather than sent. Default 5000. */
  dedupeWindowMs?: number;
  /** Attach the signed-in user's id and email. Default true. */
  sendUser?: boolean;
  /** Messages never reported. */
  ignoreMessages?: (string | RegExp)[];
  /** Last chance to redact or veto a report; return null to drop it. */
  beforeSend?: (report: ErnoErrorReport) => ErnoErrorReport | null;
}

export interface ErnoConfig {
  baseUrl: string;
  wsUrl: string;
  /** Error reporting. Omit to leave it off. */
  errorReporting?: ErnoErrorReportingConfig;
}

export const ERNO_CONFIG = new InjectionToken<ErnoConfig>('ErnoConfig');
