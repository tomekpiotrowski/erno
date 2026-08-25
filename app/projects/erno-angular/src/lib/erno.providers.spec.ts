import { TestBed } from '@angular/core/testing';
import { HttpClient } from '@angular/common/http';
import { HttpTestingController, provideHttpClientTesting } from '@angular/common/http/testing';
import { firstValueFrom } from 'rxjs';
import { provideErno } from './erno.providers';
import { ErnoAuthService } from './auth/erno-auth.service';
import { ErnoDatabaseService } from './sync/erno-database.service';
import { ErrorHandler } from '@angular/core';
import { ErnoErrorHandler } from './errors/erno-error-handler';
import { ErnoErrorReporterService } from './errors/erno-error-reporter.service';

const BASE = 'http://api';

describe('provideErno', () => {
  let http: HttpClient;
  let httpMock: HttpTestingController;

  beforeEach(() => {
    sessionStorage.clear();
    localStorage.clear();
    sessionStorage.setItem('erno_access_token', 'old-access');
    localStorage.setItem('erno_refresh_token', 'refresh-1');
    localStorage.setItem('erno_user', JSON.stringify({ id: 'u1', email: 'a@b.c' }));

    TestBed.configureTestingModule({
      providers: [
        provideErno({ baseUrl: BASE, wsUrl: 'ws://api/ws' }),
        provideHttpClientTesting(),
        { provide: ErnoDatabaseService, useValue: { clear: () => Promise.resolve() } },
      ],
    });
    http = TestBed.inject(HttpClient);
    httpMock = TestBed.inject(HttpTestingController);
  });

  afterEach(() => {
    httpMock.verify();
    sessionStorage.clear();
    localStorage.clear();
  });

  it('registers the JWT interceptor and refreshes on 401', async () => {
    const pending = firstValueFrom(http.get(`${BASE}/api/todos`));

    const first = httpMock.expectOne(`${BASE}/api/todos`);
    expect(first.request.headers.get('Authorization')).toBe('Bearer old-access');
    first.flush({ message: 'expired' }, { status: 401, statusText: 'Unauthorized' });

    const refresh = httpMock.expectOne(`${BASE}/api/auth/refresh`);
    expect(refresh.request.body).toEqual({ refresh_token: 'refresh-1' });
    refresh.flush({
      access_token: 'new-access',
      refresh_token: 'refresh-2',
      user: { id: 'u1', email: 'a@b.c' },
    });

    const retry = httpMock.expectOne(`${BASE}/api/todos`);
    expect(retry.request.headers.get('Authorization')).toBe('Bearer new-access');
    retry.flush({ ok: true });

    expect(await pending).toEqual({ ok: true });
    expect(TestBed.inject(ErnoAuthService).accessToken).toBe('new-access');
  });

  it('registers the error handler and reporter', () => {
    expect(TestBed.inject(ErrorHandler)).toBeInstanceOf(ErnoErrorHandler);
    expect(TestBed.inject(ErnoErrorReporterService)).toBeTruthy();
  });

  it('leaves the reporter inert when no ingest key is configured', () => {
    // Overriding ErrorHandler is intrusive, so an application that has not
    // opted in must get a handler that reports nothing.
    expect(TestBed.inject(ErnoErrorReporterService).active).toBe(false);
  });
});

describe('provideErno session restore', () => {
  afterEach(() => {
    sessionStorage.clear();
    localStorage.clear();
  });

  it('does not cycle HTTP_INTERCEPTORS when restoreSession refreshes', () => {
    sessionStorage.clear();
    localStorage.setItem('erno_refresh_token', 'refresh-1');
    localStorage.setItem('erno_user', JSON.stringify({ id: 'u1', email: 'a@b.c' }));

    TestBed.configureTestingModule({
      providers: [
        provideErno({ baseUrl: BASE, wsUrl: 'ws://api/ws' }),
        provideHttpClientTesting(),
        { provide: ErnoDatabaseService, useValue: { clear: () => Promise.resolve() } },
      ],
    });

    expect(() => TestBed.inject(ErnoAuthService)).not.toThrow();

    const httpMock = TestBed.inject(HttpTestingController);
    httpMock.expectOne(`${BASE}/api/auth/refresh`).flush({
      access_token: 'new-access',
      refresh_token: 'refresh-2',
      user: { id: 'u1', email: 'a@b.c' },
    });
    httpMock.verify();
    expect(TestBed.inject(ErnoAuthService).accessToken).toBe('new-access');
  });
});
