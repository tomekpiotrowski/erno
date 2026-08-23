import { TestBed } from '@angular/core/testing';
import { provideHttpClient } from '@angular/common/http';
import { HttpTestingController, provideHttpClientTesting } from '@angular/common/http/testing';

import { ERNO_CONFIG, ErnoConfig, ErnoErrorReportingConfig } from '../erno.config';
import { ErnoErrorReporterService } from './erno-error-reporter.service';
import { REDACTED } from './scrub';

const ENDPOINT = 'https://monitoring.test/api/errors';

function configure(errorReporting: ErnoErrorReportingConfig | undefined) {
  const config: ErnoConfig = {
    baseUrl: 'https://api.test',
    wsUrl: 'wss://api.test',
    errorReporting,
  };
  TestBed.configureTestingModule({
    providers: [
      provideHttpClient(),
      provideHttpClientTesting(),
      { provide: ERNO_CONFIG, useValue: config },
      ErnoErrorReporterService,
    ],
  });
  return {
    service: TestBed.inject(ErnoErrorReporterService),
    http: TestBed.inject(HttpTestingController),
  };
}


/**
 * Let queued microtasks settle.
 *
 * The reporter buffers through promises, so a report is not visible to `flush`
 * — and `flush`'s request is not visible to the testing controller — until the
 * microtask queue drains.
 */
async function settle(times = 5): Promise<void> {
  for (let i = 0; i < times; i++) {
    await Promise.resolve();
  }
}

/**
 * Flush and hand back the pending promise.
 *
 * `flush()` resolves only once the HTTP call completes, and the testing
 * controller does not complete it until the test asks — so awaiting the flush
 * before expecting the request would deadlock.
 */
async function startFlush(
  service: ErnoErrorReporterService,
): Promise<{ pending: Promise<void> }> {
  await settle();
  const pending = service.flush();
  await settle();
  // Wrapped, because `await` recursively unwraps a returned promise — handing
  // the flush back bare would make the caller wait for the very response it has
  // not delivered yet.
  return { pending };
}

const enabled: ErnoErrorReportingConfig = {
  key: 'public-browser-token',
  endpoint: ENDPOINT,
  release: '1.2.3',
  environment: 'production',
};

describe('ErnoErrorReporterService', () => {
  afterEach(() => {
    vi.useRealTimers();
  });

  it('is inert without a key, so an app never reports by accident', () => {
    const { service, http } = configure({ endpoint: ENDPOINT });
    expect(service.active).toBe(false);
    service.report(new Error('boom'));
    http.verify();
  });

  it('is inert when explicitly disabled', () => {
    const { service } = configure({ ...enabled, enabled: false });
    expect(service.active).toBe(false);
  });

  it('sends a scrubbed report with release and environment', async () => {
    const { service, http } = configure(enabled);
    service.report(new Error('failed with Bearer abcdef123456'));

    const { pending } = await startFlush(service);

    const request = http.expectOne(ENDPOINT);
    expect(request.request.headers.get('X-Erno-Ingest-Key')).toBe('public-browser-token');
    const body = request.request.body;
    expect(body.release).toBe('1.2.3');
    expect(body.environment).toBe('production');
    expect(body.sdk.name).toBe('erno-angular');
    expect(body.events[0].message).toContain(REDACTED);
    expect(body.events[0].message).not.toContain('abcdef123456');
    request.flush({ accepted: 1, dropped: 0 });
    await pending;
    http.verify();
  });

  it('suppresses a burst of identical errors and reports the count', async () => {
    // What stops a render loop from spamming the collector.
    const { service, http } = configure(enabled);
    for (let i = 0; i < 50; i++) {
      service.report(new Error('same every time'));
    }

    const { pending } = await startFlush(service);

    const request = http.expectOne(ENDPOINT);
    expect(request.request.body.events).toHaveLength(1);
    expect(request.request.body.events[0].context.duplicates).toBe(49);
    request.flush({});
    await pending;
    http.verify();
  });

  it('caps the number of distinct reports per minute', async () => {
    const { service, http } = configure({ ...enabled, maxReportsPerMinute: 5 });
    for (let i = 0; i < 20; i++) {
      service.report(new Error(`distinct ${i}`));
    }

    const { pending } = await startFlush(service);

    const request = http.expectOne(ENDPOINT);
    expect(request.request.body.events.length).toBe(5);
    // The loss is recorded on the batch rather than being silent.
    expect(request.request.body.events[0].context.dropped_locally).toBe(15);
    request.flush({});
    await pending;
    http.verify();
  });

  it('sends nothing at a zero sample rate', async () => {
    const { service, http } = configure({ ...enabled, sampleRate: 0 });
    service.report(new Error('boom'));
    await settle();
    await service.flush();
    http.verify();
  });

  it('lets beforeSend veto a report', async () => {
    const { service, http } = configure({ ...enabled, beforeSend: () => null });
    service.report(new Error('boom'));
    await settle();
    await service.flush();
    http.verify();
  });

  it('lets beforeSend rewrite a report', async () => {
    const { service, http } = configure({
      ...enabled,
      beforeSend: (report) => ({ ...report, message: 'rewritten' }),
    });
    service.report(new Error('original'));
    const { pending } = await startFlush(service);

    const request = http.expectOne(ENDPOINT);
    expect(request.request.body.events[0].message).toBe('rewritten');
    request.flush({});
    await pending;
  });

  it('honours ignoreMessages', async () => {
    const { service, http } = configure({
      ...enabled,
      ignoreMessages: ['ResizeObserver', /chunk load/i],
    });
    service.report(new Error('ResizeObserver loop limit exceeded'));
    service.report(new Error('Chunk Load Error'));
    await settle();
    await service.flush();
    http.verify();
  });

  it('never reports a failure of reporting itself', async () => {
    // The one genuine feedback loop on the client.
    const { service, http } = configure(enabled);
    service.report(new Error(`POST ${ENDPOINT} failed`));
    await settle();
    await service.flush();
    http.verify();
  });

  it('retries a 5xx but not a 400', async () => {
    const { service, http } = configure(enabled);

    service.report(new Error('transient'));
    const { pending: first } = await startFlush(service);
    http.expectOne(ENDPOINT).flush(null, { status: 503, statusText: 'Unavailable' });
    await first;

    // The batch was requeued rather than lost.
    const { pending: second } = await startFlush(service);
    const retried = http.expectOne(ENDPOINT);
    expect(retried.request.body.events[0].message).toBe('transient');
    // A rejected payload is dropped, so nothing is requeued this time.
    retried.flush(null, { status: 400, statusText: 'Bad Request' });
    await second;

    await settle();
    await service.flush();
    http.verify();
  });

  it('strips the URL fragment from context', async () => {
    const { service, http } = configure(enabled);
    service.report(new Error('boom'), { url: 'https://app.test/x#tok=supersecretvalue' });
    const { pending } = await startFlush(service);

    const request = http.expectOne(ENDPOINT);
    expect(JSON.stringify(request.request.body)).not.toContain('supersecretvalue');
    request.flush({});
    await pending;
  });

  it('captureMessage reports without an exception', async () => {
    const { service, http } = configure(enabled);
    service.captureMessage('something looked odd', 'warning');
    const { pending } = await startFlush(service);

    const request = http.expectOne(ENDPOINT);
    expect(request.request.body.events[0].level).toBe('warning');
    expect(request.request.body.events[0].message).toBe('something looked odd');
    request.flush({});
    await pending;
  });

  it('report() never throws, whatever it is handed', () => {
    const { service } = configure(enabled);
    const circular: Record<string, unknown> = {};
    circular['self'] = circular;
    for (const value of [null, undefined, 0, '', circular, Symbol('x')]) {
      expect(() => service.report(value)).not.toThrow();
    }
  });
});
