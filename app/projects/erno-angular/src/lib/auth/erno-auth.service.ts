import { Inject, Injectable } from '@angular/core';
import { HttpClient, HttpErrorResponse } from '@angular/common/http';
import { BehaviorSubject, Observable, from } from 'rxjs';
import { finalize, shareReplay, switchMap, tap } from 'rxjs/operators';
import { ERNO_CONFIG, ErnoConfig } from '../erno.config';
import { ErnoDatabaseService } from '../sync/erno-database.service';

export interface AuthUser {
  id: string;
  email: string;
}

export interface LoginResponse {
  access_token: string;
  refresh_token: string;
  user: AuthUser;
}

export type OauthProviderName = 'google' | 'discord' | 'apple';

export interface OauthProvidersResponse {
  providers: OauthProviderName[];
}

const ACCESS_KEY = 'erno_access_token';
const REFRESH_KEY = 'erno_refresh_token';
/** Persists `{ id, email }` so a page reload does not look signed-out. */
const USER_KEY = 'erno_user';

/** Read the access token without constructing `ErnoAuthService`. */
export function ernoAccessToken(): string | null {
  return sessionStorage.getItem(ACCESS_KEY);
}

/** Access tokens last 15 minutes; refresh this far before `exp`. */
const EXPIRY_SKEW_SECONDS = 30;

/**
 * True when `token` is a JWT whose `exp` is at/past now (with skew).
 *
 * Missing tokens and opaque non-JWTs are not expired — anonymous sockets and
 * tests that stub `accessToken: 'tok'` must not trigger a refresh.
 */
export function jwtAccessTokenExpired(
  token: string | null,
  nowMs = Date.now(),
  skewSeconds = EXPIRY_SKEW_SECONDS,
): boolean {
  if (!token) return false;
  const part = token.split('.')[1];
  if (!part) return false;
  const padded = part.replace(/-/g, '+').replace(/_/g, '/') + '='.repeat((4 - (part.length % 4)) % 4);
  try {
    const payload = JSON.parse(atob(padded)) as { exp?: unknown };
    return typeof payload.exp === 'number' && payload.exp * 1000 <= nowMs + skewSeconds * 1000;
  } catch {
    return false;
  }
}

/**
 * A failed `/api/auth/refresh` only means the session is dead when the server
 * rejected the token (`401`). Network errors, `404` from the boot liveness
 * server, `5xx`, `429` — those are the API being down, not a revoked session.
 */
export function isFatalRefreshError(err: unknown): boolean {
  return err instanceof HttpErrorResponse && err.status === 401;
}

@Injectable()
export class ErnoAuthService {
  constructor(
    @Inject(ERNO_CONFIG) private config: ErnoConfig,
    private http: HttpClient,
    private db: ErnoDatabaseService,
  ) {
    this.restoreSession();
  }

  private _currentUser = new BehaviorSubject<AuthUser | null>(null);
  readonly currentUser$ = this._currentUser.asObservable();
  get currentUser(): AuthUser | null { return this._currentUser.value; }

  get accessToken(): string | null { return ernoAccessToken(); }
  get refreshToken(): string | null { return localStorage.getItem(REFRESH_KEY); }

  /** The refresh currently in flight, if any. See `refresh()`. */
  private inFlightRefresh: Observable<LoginResponse> | null = null;

  login(email: string, password: string): Observable<LoginResponse> {
    return this.http.post<LoginResponse>(`${this.config.baseUrl}/api/auth/login`, { email, password }).pipe(
      tap(res => this.storeSession(res)),
    );
  }

  register(email: string, password: string): Observable<void> {
    return this.http.post<void>(`${this.config.baseUrl}/api/auth/register`, { email, password });
  }

  logout(): Observable<void> {
    return this.http.post<void>(`${this.config.baseUrl}/api/auth/logout`, { refresh_token: this.refreshToken }).pipe(
      // The local session goes either way. `/logout` needs a live access token,
      // so the logout that follows a failed refresh is answered with a 401 —
      // and leaving the dead tokens in storage would have every later request
      // reach for a refresh that cannot succeed.
      tap({ next: () => this.clearSession(), error: () => this.clearSession() }),
    );
  }

  /** Permanently delete the current account and all its data.
   * Password accounts: pass the current password (`X-Confirm-Password`).
   * OAuth-only accounts: omit the password. */
  deleteAccount(password?: string): Observable<void> {
    const headers: Record<string, string> = {};
    if (password) {
      headers['X-Confirm-Password'] = password;
    }
    return this.http.delete<void>(`${this.config.baseUrl}/api/account`, { headers }).pipe(
      tap(() => this.clearSession()),
      switchMap(() => from(this.wipeLocalData())),
    );
  }

  private async wipeLocalData(): Promise<void> {
    try {
      await this.db.clear();
    } catch {
      // ignore — server-side deletion already succeeded
    }
  }

  /**
   * Exchange the refresh token for a fresh pair, sharing one in-flight request.
   *
   * The server rotates refresh tokens: it deletes the row as it consumes it and
   * rejects a replay, so two concurrent calls carrying the same token leave one
   * holding a 401. `restoreSession()`, a route guard and the HTTP interceptor
   * all reach for a refresh the moment an access token is missing, and on a cold
   * load they reach at once — so they share a single call rather than race.
   */
  refresh(): Observable<LoginResponse> {
    this.inFlightRefresh ??= this.http
      .post<LoginResponse>(`${this.config.baseUrl}/api/auth/refresh`, {
        refresh_token: this.refreshToken,
      })
      .pipe(
        tap(res => this.storeSession(res)),
        finalize(() => (this.inFlightRefresh = null)),
        shareReplay({ bufferSize: 1, refCount: false }),
      );
    return this.inFlightRefresh;
  }

  verifyEmail(token: string): Observable<LoginResponse> {
    return this.http.post<LoginResponse>(`${this.config.baseUrl}/api/auth/email/verify`, { token }).pipe(
      tap(res => this.storeSession(res)),
    );
  }

  resendVerification(email: string): Observable<void> {
    return this.http.post<void>(`${this.config.baseUrl}/api/auth/email/resend-verification`, { email });
  }

  requestPasswordReset(email: string): Observable<void> {
    return this.http.post<void>(`${this.config.baseUrl}/api/auth/password-reset/request`, { email });
  }

  confirmPasswordReset(token: string, password: string): Observable<LoginResponse> {
    return this.http.post<LoginResponse>(`${this.config.baseUrl}/api/auth/password-reset/confirm`, { token, password }).pipe(
      tap(res => this.storeSession(res)),
    );
  }

  /** Configured OAuth providers (for showing social buttons). */
  listOauthProviders(): Observable<OauthProvidersResponse> {
    return this.http.get<OauthProvidersResponse>(`${this.config.baseUrl}/api/auth/oauth/providers`);
  }

  /** Absolute URL that starts the OAuth redirect for `provider`. */
  oauthStartUrl(provider: OauthProviderName): string {
    return `${this.config.baseUrl}/api/auth/oauth/${provider}/start`;
  }

  /** Navigate the browser to the OAuth provider start endpoint. */
  beginOauth(provider: OauthProviderName): void {
    window.location.assign(this.oauthStartUrl(provider));
  }

  /** Exchange the one-time code from `/oauth/callback?code=` for a session. */
  exchangeOauthCode(code: string): Observable<LoginResponse> {
    return this.http.post<LoginResponse>(`${this.config.baseUrl}/api/auth/oauth/exchange`, { code }).pipe(
      tap(res => this.storeSession(res)),
    );
  }

  private storeSession(res: LoginResponse): void {
    sessionStorage.setItem(ACCESS_KEY, res.access_token);
    localStorage.setItem(REFRESH_KEY, res.refresh_token);
    localStorage.setItem(USER_KEY, JSON.stringify(res.user));
    this._currentUser.next(res.user);
  }

  private clearSession(): void {
    sessionStorage.removeItem(ACCESS_KEY);
    localStorage.removeItem(REFRESH_KEY);
    localStorage.removeItem(USER_KEY);
    this._currentUser.next(null);
  }

  /**
   * After a full page load, tokens may still be in storage but `currentUser`
   * starts null. Rehydrate the user from localStorage; if we still lack a
   * user or access token, exchange the refresh token for a full session.
   */
  private restoreSession(): void {
    if (!this.refreshToken) {
      // Leave guest state clean (no partial token crumbs).
      sessionStorage.removeItem(ACCESS_KEY);
      localStorage.removeItem(USER_KEY);
      return;
    }

    const raw = localStorage.getItem(USER_KEY);
    if (raw) {
      try {
        const user = JSON.parse(raw) as AuthUser;
        if (user?.id && user?.email) {
          this._currentUser.next(user);
        }
      } catch {
        localStorage.removeItem(USER_KEY);
      }
    }

    // Missing access token (new tab), expired JWT, or user profile (upgrade /
    // cleared key): refresh re-issues both and repopulates storage.
    if (!this.accessToken || !this.currentUser || jwtAccessTokenExpired(this.accessToken)) {
      this.refresh().subscribe({
        error: err => {
          if (isFatalRefreshError(err)) {
            this.clearSession();
          }
        },
      });
    }
  }
}
