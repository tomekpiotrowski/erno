import { inject, Injectable } from '@angular/core';
import { HttpClient } from '@angular/common/http';
import { Observable } from 'rxjs';
import { ERNO_CONFIG } from '../erno.config';

export interface MockEmail {
  id: string;
  to: string;
  from: string;
  subject: string;
  body_html: string | null;
  body_text: string | null;
  created_at: string;
}

@Injectable()
export class ErnoDevMailService {
  private http = inject(HttpClient);
  private config = inject(ERNO_CONFIG);

  list(): Observable<MockEmail[]> {
    return this.http.get<MockEmail[]>(`${this.config.baseUrl}/dev/emails`);
  }

  delete(id: string): Observable<void> {
    return this.http.delete<void>(`${this.config.baseUrl}/dev/emails/${id}`);
  }

  clear(): Observable<void> {
    return this.http.delete<void>(`${this.config.baseUrl}/dev/emails`);
  }
}
