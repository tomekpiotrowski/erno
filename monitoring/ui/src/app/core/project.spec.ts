import { TestBed } from '@angular/core/testing';
import { provideHttpClient } from '@angular/common/http';
import { HttpTestingController, provideHttpClientTesting } from '@angular/common/http/testing';
import { ProjectContext, Project } from './project';
import { CollectorApi } from './api';

const KEY = 'erno-monitoring-project';

function project(slug: string): Project {
  return {
    id: `id-${slug}`,
    slug,
    name: slug,
    cors_origins: [],
    scrape_target: '',
    scrape_scheme: 'https',
    scrape_metrics_token_set: false,
    event_retention_days: null,
    issue_retention_days: null,
    max_events_per_issue: null,
    status_enabled: false,
    status_name: '',
    created_at: '2026-08-28T00:00:00',
  };
}

describe('ProjectContext', () => {
  let context: ProjectContext;
  let http: HttpTestingController;

  beforeEach(() => {
    localStorage.removeItem(KEY);
    TestBed.configureTestingModule({
      providers: [provideHttpClient(), provideHttpClientTesting()],
    });
    context = TestBed.inject(ProjectContext);
    http = TestBed.inject(HttpTestingController);
  });

  afterEach(() => {
    http.verify();
    localStorage.removeItem(KEY);
  });

  function load(projects: Project[]) {
    context.load().subscribe();
    http.expectOne('/api/collector/projects').flush({ projects });
  }

  it('settles on the first project when nothing is remembered', () => {
    load([project('monitoring'), project('teryon')]);
    expect(context.slug).toBe('monitoring');
  });

  it('prefers the remembered project', () => {
    localStorage.setItem(KEY, 'teryon');
    load([project('monitoring'), project('teryon')]);
    expect(context.slug).toBe('teryon');
  });

  // A project can be deleted from another browser. Honouring a stale slug would
  // strand this console on 404s with no way back.
  it('falls back to the first project when the remembered one is gone', () => {
    localStorage.setItem(KEY, 'deleted');
    load([project('monitoring')]);
    expect(context.slug).toBe('monitoring');
  });

  it('leaves the slug empty when there are no projects', () => {
    load([]);
    expect(context.slug).toBe('');
  });

  it('survives a collector that cannot answer', () => {
    let completed = false;
    context.load().subscribe({ complete: () => (completed = true) });
    http.expectOne('/api/collector/projects').flush('nope', { status: 500, statusText: 'Error' });
    expect(completed).toBe(true);
    expect(context.projects).toEqual([]);
  });

  it('remembers a selection for the next visit', () => {
    load([project('monitoring'), project('teryon')]);
    context.select('teryon');
    expect(context.slug).toBe('teryon');
    expect(localStorage.getItem(KEY)).toBe('teryon');
  });
});

describe('CollectorApi', () => {
  let api: CollectorApi;
  let context: ProjectContext;
  let http: HttpTestingController;

  beforeEach(() => {
    localStorage.removeItem(KEY);
    TestBed.configureTestingModule({
      providers: [provideHttpClient(), provideHttpClientTesting()],
    });
    api = TestBed.inject(CollectorApi);
    context = TestBed.inject(ProjectContext);
    http = TestBed.inject(HttpTestingController);

    context.load().subscribe();
    http.expectOne('/api/collector/projects').flush({ projects: [project('teryon')] });
  });

  afterEach(() => {
    http.verify();
    localStorage.removeItem(KEY);
  });

  // The operator API is nested under the project; a request built without the
  // slug would read another application's data or 404.
  it('addresses the current project', () => {
    api.issues().subscribe();
    http.expectOne((r) => r.url === '/api/collector/projects/teryon/issues').flush({
      issues: [],
      page: 1,
      per_page: 50,
      total: 0,
    });

    api.health().subscribe();
    http.expectOne('/api/collector/projects/teryon/health').flush({ instances: [], state: 'ok' });

    api.deleteAlertRule('r1').subscribe();
    http.expectOne('/api/collector/projects/teryon/alerts/r1').flush(null);
  });

  it('follows a change of project', () => {
    context.select('other');
    api.uptime().subscribe();
    http
      .expectOne((r) => r.url === '/api/collector/projects/other/uptime')
      .flush({ checks: [], window_hours: 24 });
  });
});
