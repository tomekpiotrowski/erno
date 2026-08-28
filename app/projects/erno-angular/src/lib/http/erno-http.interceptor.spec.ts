import { TestBed } from '@angular/core/testing';
import { HttpClient, provideHttpClient, withInterceptors } from '@angular/common/http';
import { HttpTestingController, provideHttpClientTesting } from '@angular/common/http/testing';
import { ERNO_CONFIG } from '../erno.config';
import { ErnoDatabaseService } from '../sync/erno-database.service';
import { ErnoAuthService } from '../auth/erno-auth.service';
import { ernoHttpInterceptor } from './erno-http.interceptor';

const PROJECTS_URL = 'http://api/api/projects';
const REFRESH_URL = 'http://api/api/auth/refresh';
const LOGOUT_URL = 'http://api/api/auth/logout';
const INGEST_URL = 'http://api/api/errors';

describe('ErnoHttpInterceptor', () => {
  let http: HttpTestingController;
  let client: HttpClient;
  let auth: ErnoAuthService;

  beforeEach(() => {
    localStorage.clear();
    sessionStorage.clear();
    // A session whose tokens are both present but no longer accepted.
    localStorage.setItem('erno_refresh_token', 'dead_refresh');
    localStorage.setItem('erno_user', JSON.stringify({ id: 'u', email: 'u@example.com' }));
    sessionStorage.setItem('erno_access_token', 'stale_access');

    TestBed.configureTestingModule({
      providers: [
        provideHttpClient(withInterceptors([ernoHttpInterceptor])),
        provideHttpClientTesting(),
        { provide: ERNO_CONFIG, useValue: { baseUrl: 'http://api', wsUrl: 'ws://api/ws' } },
        { provide: ErnoDatabaseService, useValue: { clear: vi.fn().mockResolvedValue(undefined) } },
        ErnoAuthService,
      ],
    });
    http = TestBed.inject(HttpTestingController);
    client = TestBed.inject(HttpClient);
    auth = TestBed.inject(ErnoAuthService);
  });

  afterEach(() => http.verify());

  it('attaches the access token to calls at the configured base URL', () => {
    client.get(PROJECTS_URL).subscribe();
    expect(http.expectOne(PROJECTS_URL).request.headers.get('Authorization')).toBe(
      'Bearer stale_access',
    );
  });

  it('leaves requests to other hosts alone', () => {
    client.get('http://elsewhere/thing').subscribe();
    expect(http.expectOne('http://elsewhere/thing').request.headers.has('Authorization')).toBe(
      false,
    );
  });

  it('settles into signed-out instead of looping when the refresh token is dead', () => {
    const failed = vi.fn().mockName('failed');
    client.get(PROJECTS_URL).subscribe({ next: () => undefined, error: failed });

    // Answer every request with 401, the way a server with a dead session does.
    // A round that leaves more work behind is the interceptor driving another
    // recovery attempt; the sequence has to reach a fixed point.
    const rounds: string[][] = [];
    for (let i = 0; i < 10; i++) {
      const pending = http.match(() => true);
      if (pending.length === 0) break;
      rounds.push(pending.map(r => r.request.url));
      for (const r of pending) {
        r.flush('nope', { status: 401, statusText: 'Unauthorized' });
      }
    }

    // The original call, then one refresh, then one logout — and quiet.
    expect(rounds).toEqual([[PROJECTS_URL], [REFRESH_URL], [LOGOUT_URL]]);
    expect(failed).toHaveBeenCalled();
    // The dead tokens are gone, so nothing reaches for a refresh again.
    expect(auth.refreshToken).toBeNull();
    expect(auth.accessToken).toBeNull();
    expect(auth.currentUser()).toBeNull();
  });

  it('retries the original request once the refresh succeeds', () => {
    const ok = vi.fn().mockName('ok');
    client.get(PROJECTS_URL).subscribe(ok);

    http.expectOne(PROJECTS_URL).flush('nope', { status: 401, statusText: 'Unauthorized' });
    http.expectOne(REFRESH_URL).flush({
      access_token: 'fresh_access',
      refresh_token: 'fresh_refresh',
      user: { id: 'u', email: 'u@example.com' },
    });

    const retried = http.expectOne(PROJECTS_URL);
    expect(retried.request.headers.get('Authorization')).toBe('Bearer fresh_access');
    retried.flush([{ id: 'p1' }]);
    expect(ok).toHaveBeenCalledWith([{ id: 'p1' }]);
  });

  it.each([
    { label: 'the boot liveness server (404)', status: 404, statusText: 'Not Found' },
    { label: 'a restarting process (503)', status: 503, statusText: 'Service Unavailable' },
  ])('keeps the session when refresh fails because $label', ({ status, statusText }) => {
    const failed = vi.fn().mockName('failed');
    client.get(PROJECTS_URL).subscribe({ next: () => undefined, error: failed });

    http.expectOne(PROJECTS_URL).flush('expired', { status: 401, statusText: 'Unauthorized' });
    http.expectOne(REFRESH_URL).flush('down', { status, statusText });

    expect(failed).toHaveBeenCalled();
    http.expectNone(LOGOUT_URL);
    expect(auth.refreshToken).toBe('dead_refresh');
    expect(auth.accessToken).toBe('stale_access');
    expect(auth.currentUser()).toEqual({ id: 'u', email: 'u@example.com' });
  });

  it('keeps the session when refresh fails with a network error', () => {
    const failed = vi.fn().mockName('failed');
    client.get(PROJECTS_URL).subscribe({ next: () => undefined, error: failed });

    http.expectOne(PROJECTS_URL).flush('expired', { status: 401, statusText: 'Unauthorized' });
    http.expectOne(REFRESH_URL).error(new ProgressEvent('error'));

    expect(failed).toHaveBeenCalled();
    http.expectNone(LOGOUT_URL);
    expect(auth.refreshToken).toBe('dead_refresh');
    expect(auth.accessToken).toBe('stale_access');
  });

  it('does not start token recovery for a 401 from the ingest endpoint', () => {
    // Otherwise: ingest 401 -> refresh -> refresh fails -> ErrorHandler reports
    // it -> POST to ingest -> 401, for ever.
    let status = 0;
    client.post(INGEST_URL, { events: [] }).subscribe({
      next: () => undefined,
      error: (e: { status: number }) => (status = e.status),
    });

    http.expectOne(INGEST_URL).flush(null, { status: 401, statusText: 'Unauthorized' });

    expect(status).toBe(401);
    // No refresh was attempted.
    http.expectNone(REFRESH_URL);
  });
});

describe('ernoHttpInterceptor guest session', () => {
  afterEach(() => {
    sessionStorage.clear();
    localStorage.clear();
  });

  it('does not throw NG0200 on the first register POST', () => {
    sessionStorage.clear();
    localStorage.clear();

    TestBed.configureTestingModule({
      providers: [
        provideHttpClient(withInterceptors([ernoHttpInterceptor])),
        provideHttpClientTesting(),
        { provide: ERNO_CONFIG, useValue: { baseUrl: 'http://api', wsUrl: 'ws://api/ws' } },
        { provide: ErnoDatabaseService, useValue: { clear: vi.fn().mockResolvedValue(undefined) } },
        ErnoAuthService,
      ],
    });

    const auth = TestBed.inject(ErnoAuthService);
    expect(() => auth.register('a@b.c', 'password1').subscribe()).not.toThrow();

    const http = TestBed.inject(HttpTestingController);
    const req = http.expectOne('http://api/api/auth/register');
    expect(req.request.headers.has('Authorization')).toBe(false);
    req.flush(null);
    http.verify();
  });
});
