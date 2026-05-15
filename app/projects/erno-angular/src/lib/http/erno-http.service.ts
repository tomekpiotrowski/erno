import { Inject, Injectable } from '@angular/core';
import { HttpClient, HttpErrorResponse, HttpParams } from '@angular/common/http';
import { EMPTY, Observable } from 'rxjs';
import { catchError, map, shareReplay } from 'rxjs/operators';
import { ERNO_CONFIG, ErnoConfig } from '../erno.config';
import { ErnoAlertsService } from '../alerts/erno-alerts.service';

export type HttpStatusCodeHandlers<ResponseType> = {
  [status: number]: (error: HttpErrorResponse) => Observable<ResponseType>;
};

@Injectable()
export class ErnoHttpService {
  constructor(
    @Inject(ERNO_CONFIG) private config: ErnoConfig,
    private http: HttpClient,
    private alerts: ErnoAlertsService,
  ) {}

  get<T>(
    path: string,
    params?: Record<string, string | string[]>,
    handlers?: HttpStatusCodeHandlers<T>,
  ): Observable<T> {
    return this.http
      .get<T>(`${this.config.baseUrl}${path}`, { params: this.toHttpParams(params) })
      .pipe(
        catchError(err => this.handleErrorResponses<T>(err, handlers)),
        shareReplay({ bufferSize: 1, refCount: true }),
      );
  }

  post<T>(
    path: string,
    body: unknown,
    params?: Record<string, string | string[]>,
    handlers?: HttpStatusCodeHandlers<T>,
  ): Observable<T> {
    return this.http
      .post<T>(`${this.config.baseUrl}${path}`, body, { params: this.toHttpParams(params) })
      .pipe(
        catchError(err => this.handleErrorResponses<T>(err, handlers)),
        shareReplay({ bufferSize: 1, refCount: true }),
      );
  }

  put<T>(
    path: string,
    body: unknown,
    params?: Record<string, string | string[]>,
    handlers?: HttpStatusCodeHandlers<T>,
  ): Observable<T> {
    return this.http
      .put<T>(`${this.config.baseUrl}${path}`, body, { params: this.toHttpParams(params) })
      .pipe(
        catchError(err => this.handleErrorResponses<T>(err, handlers)),
        shareReplay({ bufferSize: 1, refCount: true }),
      );
  }

  delete(
    path: string,
    params?: Record<string, string | string[]>,
    handlers?: HttpStatusCodeHandlers<void>,
  ): Observable<void> {
    return this.http
      .delete(`${this.config.baseUrl}${path}`, { params: this.toHttpParams(params) })
      .pipe(
        map(() => undefined),
        catchError(err => this.handleErrorResponses<void>(err, handlers)),
        shareReplay({ bufferSize: 1, refCount: true }),
      );
  }

  private handleErrorResponses<T>(
    error: unknown,
    handlers?: HttpStatusCodeHandlers<T>,
  ): Observable<T> {
    if (error instanceof HttpErrorResponse && handlers?.[error.status]) {
      return handlers[error.status](error);
    }
    this.defaultErrorHandler(error);
    return EMPTY as Observable<T>;
  }

  public defaultErrorHandler(err: unknown): void {
    if (!(err instanceof HttpErrorResponse)) {
      return;
    }
    if (err.status === 0) {
      this.alerts.error('Could not connect to the server. Please check your connection.');
    } else if (err.status >= 500) {
      this.alerts.error('A server error occurred. Please try again.');
    } else if (err.status >= 400) {
      this.alerts.warn('Invalid request. Please check your input and try again.');
    }
  }

  private toHttpParams(params?: Record<string, string | string[]>): HttpParams | undefined {
    if (!params) return undefined;
    let p = new HttpParams();
    for (const [key, value] of Object.entries(params)) {
      if (Array.isArray(value)) {
        value.forEach(v => (p = p.append(key, v)));
      } else {
        p = p.set(key, value);
      }
    }
    return p;
  }
}
