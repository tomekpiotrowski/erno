import { Inject, Injectable } from '@angular/core';
import { HttpClient } from '@angular/common/http';
import { BehaviorSubject, Observable } from 'rxjs';
import { tap } from 'rxjs/operators';
import { ERNO_CONFIG, ErnoConfig } from '../erno.config';

export interface AuthUser {
  id: string;
  email: string;
}

export interface LoginResponse {
  access_token: string;
  refresh_token: string;
  user: AuthUser;
}

@Injectable()
export class ErnoAuthService {
  constructor(
    @Inject(ERNO_CONFIG) private config: ErnoConfig,
    private http: HttpClient,
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

  refresh(): Observable<LoginResponse> {
    return this.http.post<LoginResponse>(`${this.config.baseUrl}/api/auth/refresh`, { refresh_token: this.refreshToken }).pipe(
      tap(res => this.storeSession(res)),
    );
  }

  verifyEmail(token: string): Observable<void> {
    return this.http.post<void>(`${this.config.baseUrl}/api/auth/email/verify`, { token });
  }

  requestPasswordReset(email: string): Observable<void> {
    return this.http.post<void>(`${this.config.baseUrl}/api/auth/password-reset/request`, { email });
  }

  confirmPasswordReset(token: string, password: string): Observable<void> {
    return this.http.post<void>(`${this.config.baseUrl}/api/auth/password-reset/confirm`, { token, password });
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
