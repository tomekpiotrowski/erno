import { Inject, Injectable } from '@angular/core';
import { HttpClient } from '@angular/common/http';
import { Observable } from 'rxjs';
import { ERNO_CONFIG, ErnoConfig } from '../erno.config';

export interface ActiveSubscription {
  plan: string;
  status: string;
  current_period_end: string;
}

@Injectable()
export class ErnoBillingService {
  constructor(
    @Inject(ERNO_CONFIG) private config: ErnoConfig,
    private http: HttpClient,
  ) {}

  getSubscription(): Observable<ActiveSubscription | null> {
    return this.http.get<ActiveSubscription | null>(`${this.config.baseUrl}/api/billing/subscription`);
  }

  getCheckoutUrl(plan: string): Observable<{ url: string }> {
    return this.http.post<{ url: string }>(`${this.config.baseUrl}/api/billing/checkout`, { plan });
  }

  getPortalUrl(): Observable<{ url: string }> {
    return this.http.post<{ url: string }>(`${this.config.baseUrl}/api/billing/portal`, {});
  }
}
