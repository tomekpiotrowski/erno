import { Injectable, inject } from '@angular/core';
import { HttpClient } from '@angular/common/http';
import { CanActivateFn } from '@angular/router';
import { BehaviorSubject, Observable, of } from 'rxjs';
import { catchError, map, tap } from 'rxjs/operators';

/** A project as the operator API lists it. Secrets are never included. */
export interface Project {
  id: string;
  slug: string;
  name: string;
  cors_origins: string[];
  scrape_target: string;
  scrape_scheme: string;
  scrape_metrics_token_set: boolean;
  event_retention_days: number | null;
  issue_retention_days: number | null;
  max_events_per_issue: number | null;
  status_enabled: boolean;
  status_name: string;
  created_at: string;
}

const KEY = 'erno-monitoring-project';

/**
 * Which application the console is currently looking at.
 *
 * The operator API is nested under `/api/collector/projects/{slug}`, so every
 * request needs a slug before it can be built. This holds one, remembered per
 * browser, and is the single place that answers "which project".
 */
@Injectable({ providedIn: 'root' })
export class ProjectContext {
  private readonly http = inject(HttpClient);

  private readonly _projects = new BehaviorSubject<Project[]>([]);
  readonly projects$ = this._projects.asObservable();

  private readonly _slug = new BehaviorSubject<string>('');
  readonly slug$ = this._slug.asObservable();

  /**
   * Synchronous read, for building a URL.
   *
   * Empty until {@link load} resolves. Callers that build a request before then
   * would address `/projects//issues`, which is why the shell waits.
   */
  get slug(): string {
    return this._slug.value;
  }

  get projects(): Project[] {
    return this._projects.value;
  }

  /** Base path of every project-scoped operator route. */
  get base(): string {
    return `/api/collector/projects/${encodeURIComponent(this.slug)}`;
  }

  select(slug: string): void {
    if (!slug || slug === this._slug.value) {
      return;
    }
    localStorage.setItem(KEY, slug);
    this._slug.next(slug);
  }

  /**
   * Fetch the project list and settle on one.
   *
   * Prefers the remembered slug, falling back to the first project — which on a
   * fresh collector is the boot-seeded `monitoring`. A remembered project that
   * has since been deleted must not strand the console on 404s, so it is only
   * honoured when it is still in the list.
   */
  load(): Observable<Project[]> {
    return this.http.get<{ projects: Project[] }>('/api/collector/projects').pipe(
      map((body) => body.projects ?? []),
      tap((projects) => {
        this._projects.next(projects);
        const remembered = localStorage.getItem(KEY);
        const chosen =
          projects.find((p) => p.slug === remembered)?.slug ?? projects[0]?.slug ?? '';
        this._slug.next(chosen);
      }),
      catchError(() => {
        // A failure here is a login problem or a collector that is down; the
        // pages report that themselves. Do not take the shell down with it.
        this._projects.next([]);
        return of([]);
      }),
    );
  }
}

/**
 * Settle on a project before the shell renders.
 *
 * Every data page builds its URL from the current slug, so a page that ran
 * first would request `/projects//issues` and 404. Loading once here rather
 * than in each page also keeps it to one request per session.
 */
export const projectGuard: CanActivateFn = () => inject(ProjectContext).load().pipe(map(() => true));
