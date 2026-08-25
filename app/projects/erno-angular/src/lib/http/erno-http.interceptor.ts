import { inject, Injectable, Injector, runInInjectionContext } from '@angular/core';
import {
  HttpErrorResponse,
  HttpEvent,
  HttpHandler,
  HttpHandlerFn,
  HttpInterceptor,
  HttpInterceptorFn,
  HttpRequest,
} from '@angular/common/http';
import { Observable, throwError } from 'rxjs';
import { catchError, switchMap } from 'rxjs/operators';
import { ErnoAuthService, ernoAccessToken } from '../auth/erno-auth.service';
import { ERNO_CONFIG } from '../erno.config';

/**
 * Endpoints that the 401 recovery path calls itself. A 401 from one of these is
 * the recovery failing, not a request needing recovery — feeding it back into
 * `handle401` would refresh, log out, and refresh again without end.
 */
function isAuthRecovery(url: string): boolean {
  return url.includes('/api/auth/refresh') || url.includes('/api/auth/logout');
}

/**
 * The error-reporting ingest endpoint.
 *
 * A 401 from here must never start the refresh/logout dance: a failing refresh
 * throws, the global ErrorHandler reports it, and that report POSTs here again.
 * The collector is normally a different origin, so the guard above would not
 * even run — this is belt and braces for same-origin dev setups.
 */
function isErrorIngest(url: string): boolean {
  return url.includes('/api/errors');
}

function addToken(req: HttpRequest<unknown>): HttpRequest<unknown> {
  const token = ernoAccessToken();
  return token ? req.clone({ setHeaders: { Authorization: `Bearer ${token}` } }) : req;
}

function handle401(
  req: HttpRequest<unknown>,
  next: HttpHandlerFn,
  auth: ErnoAuthService,
): Observable<HttpEvent<unknown>> {
  if (!auth.refreshToken) {
    auth.logout().subscribe({ error: () => undefined });
    return throwError(() => new Error('No refresh token'));
  }

  // `ErnoAuthService.refresh()` already coalesces concurrent callers onto one
  // in-flight POST, so concurrent 401s share that request and then retry.
  return auth.refresh().pipe(
    switchMap(() => next(addToken(req))),
    catchError(err => {
      auth.logout().subscribe({ error: () => undefined });
      return throwError(() => err);
    }),
  );
}

/**
 * Attaches the JWT access token to requests against `baseUrl` and retries once
 * after a silent refresh on 401.
 *
 * Functional (and token-read without constructing `ErnoAuthService`) so the
 * first HTTP call cannot hit
 * `HTTP_INTERCEPTORS → interceptor → ErnoAuthService → HttpClient → HTTP_INTERCEPTORS`.
 * That cycle shows up as NG0200 when `restoreSession()` refreshes from the
 * auth constructor, or on the first `login`/`register` that builds the chain.
 */
export const ernoHttpInterceptor: HttpInterceptorFn = (req, next) => {
  const config = inject(ERNO_CONFIG);
  const injector = inject(Injector);

  if (!req.url.startsWith(config.baseUrl)) {
    return next(req);
  }

  return next(addToken(req)).pipe(
    catchError(err => {
      if (
        err instanceof HttpErrorResponse &&
        err.status === 401 &&
        !isAuthRecovery(req.url) &&
        !isErrorIngest(req.url)
      ) {
        // Look up auth only on the 401 path. `restoreSession()` posts /refresh
        // from the `ErnoAuthService` constructor, and that request is what
        // first builds this chain — injecting auth at the top of the function
        // would re-enter the still-hydrating service.
        return handle401(req, next, injector.get(ErnoAuthService));
      }
      return throwError(() => err);
    }),
  );
};

/** Class adapter for apps that still register `HTTP_INTERCEPTORS` by `useClass`. */
@Injectable()
export class ErnoHttpInterceptor implements HttpInterceptor {
  private readonly injector = inject(Injector);

  intercept(req: HttpRequest<unknown>, next: HttpHandler): Observable<HttpEvent<unknown>> {
    return runInInjectionContext(this.injector, () =>
      ernoHttpInterceptor(req, downstream => next.handle(downstream)),
    );
  }
}
