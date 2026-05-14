import { inject, Injectable } from '@angular/core';
import { HttpClient } from '@angular/common/http';
import { Observable } from 'rxjs';
import { ERNO_CONFIG } from '../erno.config';

export interface ActiveSubscription {
  plan: string;
  status: string;
  current_period_end: string;
}

@Injectable()
export class ErnoBillingService {
  private config = inject(ERNO_CONFIG);
  private http = inject(HttpClient);

  getSubscription(): Observable<ActiveSubscription | null> {
    return this.http.get<ActiveSubscription | null>(`${this.config.baseUrl}/billing/subscription`);
  }

  getCheckoutUrl(plan: string): Observable<{ url: string }> {
    return this.http.post<{ url: string }>(`${this.config.baseUrl}/billing/checkout`, { plan });
  }

  getPortalUrl(): Observable<{ url: string }> {
    return this.http.post<{ url: string }>(`${this.config.baseUrl}/billing/portal`, {});
  }
}
