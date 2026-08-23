/**
 * Angular's global error hook, wired to the reporter.
 *
 * Docs: docs/src/content/docs/app/error-reporting.md
 */
import { ErrorHandler, Injectable, Injector, inject } from '@angular/core';
import { ErnoErrorReporterService } from './erno-error-reporter.service';

@Injectable()
export class ErnoErrorHandler implements ErrorHandler {
  // Resolved lazily: taking the reporter (and through it HttpClient) as a
  // constructor dependency creates a DI cycle, because the handler is itself
  // constructed while the injector is still being set up.
  private readonly injector = inject(Injector);

  handleError(error: unknown): void {
    try {
      this.injector.get(ErnoErrorReporterService).report(error);
    } catch {
      // A broken reporter must never hide the application's own bug.
    }
    // Always, and after reporting: swallowing the console output is the
    // classic way an error SDK makes local development worse.
    console.error(error);
  }
}
