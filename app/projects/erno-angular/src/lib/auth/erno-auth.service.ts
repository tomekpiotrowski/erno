import { Inject, Injectable } from '@angular/core';
import { HttpClient } from '@angular/common/http';
import { BehaviorSubject, Observable, from } from 'rxjs';
import { switchMap, tap } from 'rxjs/operators';
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

@Injectable()
export class ErnoAuthService {
  constructor(
    @Inject(ERNO_CONFIG) private config: ErnoConfig,
    private http: HttpClient,
    private db: ErnoDatabaseService,
  ) {}

  private _currentUser = new BehaviorSubject<AuthUser | null>(null);
  readonly currentUser$ = this._currentUser.asObservable();
  get currentUser(): AuthUser | null { return this._currentUser.value; }

  get accessToken(): string | null { return sessionStorage.getItem('erno_access_token'); }
  get refreshToken(): string | null { return localStorage.getItem('erno_refresh_token'); }

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
      tap(() => this.clearSession()),
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

  refresh(): Observable<LoginResponse> {
    return this.http.post<LoginResponse>(`${this.config.baseUrl}/api/auth/refresh`, { refresh_token: this.refreshToken }).pipe(
      tap(res => this.storeSession(res)),
    );
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
    sessionStorage.setItem('erno_access_token', res.access_token);
    localStorage.setItem('erno_refresh_token', res.refresh_token);
    this._currentUser.next(res.user);
  }

  private clearSession(): void {
    sessionStorage.removeItem('erno_access_token');
    localStorage.removeItem('erno_refresh_token');
    this._currentUser.next(null);
  }
}
