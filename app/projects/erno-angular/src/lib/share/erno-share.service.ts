import { Inject, Injectable } from '@angular/core';
import { HttpClient } from '@angular/common/http';
import { Observable } from 'rxjs';
import { ERNO_CONFIG, ErnoConfig } from '../erno.config';

/** Header carrying raw share link tokens — never sent as a query parameter. */
export const SHARE_TOKEN_HEADER = 'X-Erno-Share';

export interface CreateShareRequest {
  entity_type: string;
  entity_id: string;
  /** Mint a secret link token (returned raw exactly once). */
  link?: boolean;
  /** Grant directly to these users — active immediately, recipients are notified. */
  recipient_user_ids?: string[];
  expires_at?: string;
}

export interface CreateShareResponse {
  id: string;
  /** Raw link token, present only when `link` was requested. Shown once. */
  token: string | null;
  entity_type: string;
  entity_id: string;
  permission: 'read' | 'write';
  expires_at: string | null;
  granted_user_ids: string[];
}

export interface ShareGrant {
  id: string;
  share_id: string;
  user_id: string;
  notified_at: string | null;
  revoked_at: string | null;
  created_at: string;
}

export interface ShareWithGrants {
  id: string;
  entity_type: string;
  entity_id: string;
  owner_id: string;
  permission: 'read' | 'write';
  expires_at: string | null;
  revoked_at: string | null;
  created_at: string;
  has_link: boolean;
  grants: ShareGrant[];
}

/**
 * Owner-side share management: create link/grant shares, list them, add
 * recipients, revoke. Share *consumption* is handled by
 * `ErnoSharedViewService` (online-only view) and the `X-Erno-Share` header.
 */
@Injectable()
export class ErnoShareService {
  constructor(
    @Inject(ERNO_CONFIG) private config: ErnoConfig,
    private http: HttpClient,
  ) {}

  create(request: CreateShareRequest): Observable<CreateShareResponse> {
    return this.http.post<CreateShareResponse>(`${this.config.baseUrl}/api/shares`, request);
  }

  list(filter?: { entity_type?: string; entity_id?: string }): Observable<ShareWithGrants[]> {
    return this.http.get<ShareWithGrants[]>(`${this.config.baseUrl}/api/shares`, {
      params: { ...filter },
    });
  }

  addGrant(shareId: string, userId: string): Observable<void> {
    return this.http.post<void>(`${this.config.baseUrl}/api/shares/${shareId}/grants`, {
      user_id: userId,
    });
  }

  revoke(shareId: string): Observable<void> {
    return this.http.delete<void>(`${this.config.baseUrl}/api/shares/${shareId}`);
  }

  revokeGrant(shareId: string, userId: string): Observable<void> {
    return this.http.delete<void>(
      `${this.config.baseUrl}/api/shares/${shareId}/grants/${userId}`,
    );
  }

  /**
   * Build a shareable URL: the token rides the fragment (`#s=...`), which is
   * never sent to the server — it stays out of access logs and Referer
   * headers. Read it back with `tokenFromLocation`.
   */
  buildShareUrl(viewUrl: string, token: string): string {
    return `${viewUrl}#s=${encodeURIComponent(token)}`;
  }

  /** Extract a share token from the current location's fragment, if any. */
  tokenFromLocation(hash: string = window.location.hash): string | null {
    const match = /[#&]s=([^&]+)/.exec(hash);
    return match ? decodeURIComponent(match[1]) : null;
  }
}
