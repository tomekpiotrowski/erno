/**
 * Retry pacing for the reporter.
 *
 * Docs: docs/src/content/docs/app/error-reporting.md
 *
 * Kept local to the reporter rather than shared: nothing else in the library
 * has exponential backoff today (`ErnoRealtimeService` uses a flat delay), and
 * inventing a shared utility for a single caller would be premature.
 */

/** First retry delay, in milliseconds. */
export const BASE_DELAY_MS = 1_000;
/** Ceiling on the delay however many failures have accumulated. */
export const MAX_DELAY_MS = 30_000;
/** Fraction of the delay randomised, so tabs do not retry in lockstep. */
export const JITTER_FRACTION = 0.2;

/** The un-jittered delay for a given attempt (1 = the first retry). */
export function baseDelayMs(attempt: number): number {
  if (attempt <= 0) {
    return 0;
  }
  // `Math.min` before the shift keeps a huge attempt count from overflowing.
  const doublings = Math.min(attempt - 1, 30);
  return Math.min(BASE_DELAY_MS * 2 ** doublings, MAX_DELAY_MS);
}

/** Delay before a retry, with ±20% jitter. */
export function nextDelayMs(attempt: number, random: () => number = Math.random): number {
  const base = baseDelayMs(attempt);
  if (base === 0) {
    return 0;
  }
  const spread = base * JITTER_FRACTION;
  return Math.round(base - spread + random() * spread * 2);
}
