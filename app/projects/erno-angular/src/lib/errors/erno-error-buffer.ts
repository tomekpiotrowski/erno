/**
 * Buffering for reports that could not be delivered yet.
 *
 * Docs: docs/src/content/docs/app/error-reporting.md
 */
import { ErnoErrorReport } from './erno-error-report';

/** Somewhere to hold reports between attempts. */
export interface ErnoErrorBuffer {
  push(report: ErnoErrorReport): Promise<void>;
  take(max: number): Promise<ErnoErrorReport[]>;
  /** Put reports back at the front after a failed send. */
  requeue(reports: ErnoErrorReport[]): Promise<void>;
  size(): Promise<number>;
}

/**
 * The default buffer: bounded, in memory, drop-oldest on overflow.
 *
 * Oldest-first is the right end to drop here — unlike the server queue, where
 * a saturated queue holds duplicates of one runaway error, a full client buffer
 * usually means the app has been offline for a while and the newest errors are
 * the ones describing what the user is hitting right now.
 */
export class MemoryErrorBuffer implements ErnoErrorBuffer {
  private reports: ErnoErrorReport[] = [];
  private droppedCount = 0;

  constructor(private readonly maxSize: number) {}

  async push(report: ErnoErrorReport): Promise<void> {
    this.reports.push(report);
    while (this.reports.length > this.maxSize) {
      this.reports.shift();
      this.droppedCount++;
    }
  }

  async take(max: number): Promise<ErnoErrorReport[]> {
    return this.reports.splice(0, max);
  }

  async requeue(reports: ErnoErrorReport[]): Promise<void> {
    this.reports = [...reports, ...this.reports].slice(0, this.maxSize);
  }

  async size(): Promise<number> {
    return this.reports.length;
  }

  /** Reports lost to overflow, for diagnostics. */
  dropped(): number {
    return this.droppedCount;
  }
}
