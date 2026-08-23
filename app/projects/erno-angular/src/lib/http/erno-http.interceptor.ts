import { Inject, Injectable } from '@angular/core';
import { HttpErrorResponse, HttpEvent, HttpHandler, HttpInterceptor, HttpRequest } from '@angular/common/http';
import { Observable, throwError, BehaviorSubject } from 'rxjs';
import { catchError, filter, switchMap, take } from 'rxjs/operators';
import { ErnoAuthService, LoginResponse } from '../auth/erno-auth.service';
import { ERNO_CONFIG, ErnoConfig } from '../erno.config';

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

@Injectable()
export class ErnoHttpInterceptor implements HttpInterceptor {
  private refreshing = false;
  private refreshSubject = new BehaviorSubject<LoginResponse | null>(null);

  constructor(
    private auth: ErnoAuthService,
    @Inject(ERNO_CONFIG) private config: ErnoConfig,
  ) {}

  intercept(req: HttpRequest<unknown>, next: HttpHandler): Observable<HttpEvent<unknown>> {
    if (!req.url.startsWith(this.config.baseUrl)) {
      return next.handle(req);
    }

    return next.handle(this.addToken(req)).pipe(
      catchError(err => {
        if (
          err instanceof HttpErrorResponse &&
          err.status === 401 &&
          !isAuthRecovery(req.url) &&
          !isErrorIngest(req.url)
        ) {
          return this.handle401(req, next);
        }
        return throwError(() => err);
      }),
    );
  }

  private addToken(req: HttpRequest<unknown>): HttpRequest<unknown> {
    const token = this.auth.accessToken;
    return token ? req.clone({ setHeaders: { Authorization: `Bearer ${token}` } }) : req;
  }

  private handle401(req: HttpRequest<unknown>, next: HttpHandler): Observable<HttpEvent<unknown>> {
    if (!this.auth.refreshToken) {
      this.auth.logout().subscribe({ error: () => undefined });
      return throwError(() => new Error('No refresh token'));
    }

    if (this.refreshing) {
      return this.refreshSubject.pipe(
        filter(r => r !== null),
        take(1),
        switchMap(() => next.handle(this.addToken(req))),
      );
    }

    this.refreshing = true;
    this.refreshSubject.next(null);

    return this.auth.refresh().pipe(
      switchMap(res => {
        this.refreshing = false;
        this.refreshSubject.next(res);
        return next.handle(this.addToken(req));
      }),
      catchError(err => {
        this.refreshing = false;
        this.auth.logout().subscribe({ error: () => undefined });
        return throwError(() => err);
      }),
    );
  }
}
