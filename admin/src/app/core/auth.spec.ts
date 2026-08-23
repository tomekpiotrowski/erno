import { HttpRequest, HttpHandlerFn, HttpResponse } from '@angular/common/http';
import { of } from 'rxjs';
import {
  authInterceptor,
  clearBasicAuth,
  getBasicAuth,
  setBasicAuth,
} from './auth';

/** Capture the request the interceptor actually forwarded. */
function run(url: string): HttpRequest<unknown> {
  const req = new HttpRequest('GET', url);
  let seen!: HttpRequest<unknown>;
  const next: HttpHandlerFn = (r) => {
    seen = r;
    return of(new HttpResponse());
  };
  (authInterceptor as unknown as (r: HttpRequest<unknown>, n: HttpHandlerFn) => unknown)(
    req,
    next,
  );
  return seen;
}

describe('authInterceptor', () => {
  beforeEach(() => clearBasicAuth());

  it('attaches Basic credentials to admin API calls', () => {
    setBasicAuth('operator', 'hunter2');
    expect(run('/admin/api/dashboard').headers.get('Authorization')).toBe(
      `Basic ${btoa('operator:hunter2')}`,
    );
  });

  it('leaves other requests untouched', () => {
    setBasicAuth('operator', 'hunter2');
    // Assets and any third-party host must never receive operator credentials.
    expect(run('/assets/logo.svg').headers.has('Authorization')).toBe(false);
    expect(run('https://example.com/x').headers.has('Authorization')).toBe(false);
  });

  it('does not authenticate /prometheus — the admin console no longer proxies it', () => {
    // Prometheus moved to the monitoring deployment. This asserts the console
    // does not hand its operator credentials to a path it no longer serves.
    setBasicAuth('operator', 'hunter2');
    expect(run('/prometheus/api/v1/query').headers.has('Authorization')).toBe(false);
  });

  it('sends nothing when the operator is not logged in', () => {
    expect(run('/admin/api/dashboard').headers.has('Authorization')).toBe(false);
  });
});

describe('credential storage', () => {
  it('round-trips and clears', () => {
    clearBasicAuth();
    expect(getBasicAuth()).toBeNull();
    setBasicAuth('a', 'b');
    expect(getBasicAuth()).toBe(btoa('a:b'));
    clearBasicAuth();
    expect(getBasicAuth()).toBeNull();
  });
});
