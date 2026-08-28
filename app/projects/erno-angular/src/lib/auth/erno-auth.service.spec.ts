import { TestBed } from '@angular/core/testing';
import { HttpErrorResponse, provideHttpClient } from '@angular/common/http';
import { HttpTestingController, provideHttpClientTesting } from '@angular/common/http/testing';
import { ERNO_CONFIG } from '../erno.config';
import { ErnoDatabaseService } from '../sync/erno-database.service';
import {
  ErnoAuthService,
  LoginResponse,
  isFatalRefreshError,
  jwtAccessTokenExpired,
} from './erno-auth.service';

function jwtWithExp(exp: number): string {
  const payload = btoa(JSON.stringify({ exp }))
    .replace(/=+$/, '')
    .replace(/\+/g, '-')
    .replace(/\//g, '_');
  return `eyJhbGciOiJub25lIn0.${payload}.sig`;
}

const REFRESH_URL = 'http://api/api/auth/refresh';

const pair = (suffix: string): LoginResponse => ({
  access_token: `access_${suffix}`,
  refresh_token: `refresh_${suffix}`,
  user: { id: 'user-1', email: 'user@example.com' },
});

describe('ErnoAuthService', () => {
  let service: ErnoAuthService;
  let http: HttpTestingController;

  /** Builds the service by hand so each test controls storage before construction. */
  function build(): void {
    TestBed.configureTestingModule({
      providers: [
        provideHttpClient(),
        provideHttpClientTesting(),
        { provide: ERNO_CONFIG, useValue: { baseUrl: 'http://api', wsUrl: 'ws://api/ws' } },
        { provide: ErnoDatabaseService, useValue: { clear: vi.fn().mockResolvedValue(undefined) } },
        ErnoAuthService,
      ],
    });
    http = TestBed.inject(HttpTestingController);
    service = TestBed.inject(ErnoAuthService);
  }

  beforeEach(() => {
    localStorage.clear();
    sessionStorage.clear();
  });

  afterEach(() => http.verify());

  describe('refresh', () => {
    beforeEach(() => {
      // A session with a refresh token but no access token — the cold-load
      // shape that makes every caller reach for a refresh at once.
      localStorage.setItem('erno_refresh_token', 'stored_refresh');
      localStorage.setItem('erno_user', JSON.stringify({ id: 'user-1', email: 'user@example.com' }));
    });

    it('shares one request across concurrent callers', () => {
      build();
      // Construction already started one refresh via restoreSession().
      const first = vi.fn().mockName('first');
      const second = vi.fn().mockName('second');
      service.refresh().subscribe(first);
      service.refresh().subscribe(second);

      // The server deletes the refresh-token row as it consumes it, so a second
      // request carrying the same token would come back 401.
      const req = http.expectOne(REFRESH_URL);
      expect(req.request.body).toEqual({ refresh_token: 'stored_refresh' });
      req.flush(pair('one'));

      expect(first).toHaveBeenCalledWith(pair('one'));
      expect(second).toHaveBeenCalledWith(pair('one'));
      expect(service.accessToken).toBe('access_one');
      expect(service.refreshToken).toBe('refresh_one');
    });

    it('starts a new request once the shared one has settled', () => {
      build();
      http.expectOne(REFRESH_URL).flush(pair('one'));

      const later = vi.fn().mockName('later');
      service.refresh().subscribe(later);

      const second = http.expectOne(REFRESH_URL);
      expect(second.request.body).toEqual({ refresh_token: 'refresh_one' });
      second.flush(pair('two'));

      expect(later).toHaveBeenCalledWith(pair('two'));
      expect(service.accessToken).toBe('access_two');
    });

    it('releases the shared request after a failure so the next caller retries', () => {
      build();
      http.expectOne(REFRESH_URL).flush('nope', { status: 401, statusText: 'Unauthorized' });
      // restoreSession() clears the session only when refresh itself is 401.
      expect(service.refreshToken).toBeNull();

      localStorage.setItem('erno_refresh_token', 'another_refresh');
      const retry = vi.fn().mockName('retry');
      service.refresh().subscribe({ next: retry, error: () => undefined });

      const req = http.expectOne(REFRESH_URL);
      expect(req.request.body).toEqual({ refresh_token: 'another_refresh' });
      req.flush(pair('three'));
      expect(retry).toHaveBeenCalledWith(pair('three'));
    });

    it('keeps the restored session when the API is down', () => {
      build();
      http.expectOne(REFRESH_URL).flush('down', { status: 503, statusText: 'Service Unavailable' });
      expect(service.refreshToken).toBe('stored_refresh');
      expect(service.currentUser()).toEqual({ id: 'user-1', email: 'user@example.com' });
    });
  });

  describe('jwtAccessTokenExpired', () => {
    const now = 1_700_000_000_000;

    it('ignores missing and opaque tokens', () => {
      expect(jwtAccessTokenExpired(null, now)).toBe(false);
      expect(jwtAccessTokenExpired('tok', now)).toBe(false);
    });

    it('detects a live vs expired JWT', () => {
      expect(jwtAccessTokenExpired(jwtWithExp(now / 1000 + 120), now)).toBe(false);
      expect(jwtAccessTokenExpired(jwtWithExp(now / 1000 - 1), now)).toBe(true);
    });
  });

  describe('isFatalRefreshError', () => {
    it('is true only for HTTP 401', () => {
      expect(isFatalRefreshError(new HttpErrorResponse({ status: 401 }))).toBe(true);
      expect(isFatalRefreshError(new HttpErrorResponse({ status: 0 }))).toBe(false);
      expect(isFatalRefreshError(new HttpErrorResponse({ status: 404 }))).toBe(false);
      expect(isFatalRefreshError(new HttpErrorResponse({ status: 503 }))).toBe(false);
      expect(isFatalRefreshError(new Error('boom'))).toBe(false);
    });
  });

  it('does not refresh when there is no stored refresh token', () => {
    build();
    http.expectNone(REFRESH_URL);
    expect(service.currentUser()).toBeNull();
  });

  it('refreshes on construct when the stored access token is an expired JWT', () => {
    sessionStorage.setItem('erno_access_token', jwtWithExp(Date.now() / 1000 - 60));
    localStorage.setItem('erno_refresh_token', 'stored_refresh');
    localStorage.setItem('erno_user', JSON.stringify({ id: 'user-1', email: 'user@example.com' }));
    build();
    http.expectOne(REFRESH_URL).flush(pair('fresh'));
    expect(service.accessToken).toBe('access_fresh');
  });
});
