import { inject, Injectable } from '@angular/core';
import { HttpErrorResponse, HttpEvent, HttpHandler, HttpInterceptor, HttpRequest } from '@angular/common/http';
import { Observable, throwError, BehaviorSubject } from 'rxjs';
import { catchError, filter, switchMap, take } from 'rxjs/operators';
import { ErnoAuthService, LoginResponse } from '../auth/erno-auth.service';
import { ERNO_CONFIG } from '../erno.config';

@Injectable()
export class ErnoHttpInterceptor implements HttpInterceptor {
  private auth = inject(ErnoAuthService);
  private config = inject(ERNO_CONFIG);

  private refreshing = false;
  private refreshSubject = new BehaviorSubject<LoginResponse | null>(null);

  intercept(req: HttpRequest<unknown>, next: HttpHandler): Observable<HttpEvent<unknown>> {
    if (!req.url.startsWith(this.config.baseUrl)) {
      return next.handle(req);
    }

    return next.handle(this.addToken(req)).pipe(
      catchError(err => {
        if (err instanceof HttpErrorResponse && err.status === 401 && !req.url.includes('/auth/refresh')) {
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
      this.auth.logout().subscribe();
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
        this.auth.logout().subscribe();
        return throwError(() => err);
      }),
    );
  }
}
