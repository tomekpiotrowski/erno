import { Component, inject, isDevMode, OnInit } from '@angular/core';
import { CommonModule } from '@angular/common';
import { ErnoRealtimeService } from '../realtime/erno-realtime.service';
import { ErnoSyncService } from '../sync/erno-sync.service';
import { ErnoDevMailService, MockEmail } from './erno-dev-mail.service';

type Tab = 'status' | 'emails';

@Component({
  selector: 'erno-devtools',
  standalone: false,
  template: `
    <div *ngIf="visible" class="erno-devtools" [class.wide]="tab === 'emails'">
      <div class="header">
        <strong>Erno Devtools</strong>
        <div class="tabs">
          <button [class.active]="tab === 'status'" (click)="tab = 'status'">Status</button>
          <button [class.active]="tab === 'emails'" (click)="switchToEmails()">
            Emails<span *ngIf="emails.length"> ({{ emails.length }})</span>
          </button>
        </div>
      </div>

      <ng-container *ngIf="tab === 'status'">
        <div>WS: {{ wsStatus }}</div>
        <div>Sync: {{ syncStatus$ | async }}</div>
        <button (click)="forceSync()">Force re-sync</button>
      </ng-container>

      <ng-container *ngIf="tab === 'emails'">
        <div class="email-toolbar">
          <button (click)="clearAll()" [disabled]="emails.length === 0">Clear all</button>
        </div>
        <div *ngIf="emails.length === 0" class="empty">No emails sent.</div>
        <div *ngFor="let email of emails" class="email-row">
          <div class="email-summary" (click)="toggle(email.id)">
            <span class="arrow">{{ expanded === email.id ? '▾' : '▸' }}</span>
            <span class="subject">{{ email.subject }}</span>
            <span class="to">→ {{ email.to }}</span>
          </div>
          <ng-container *ngIf="expanded === email.id">
            <iframe *ngIf="email.body_html" [srcdoc]="email.body_html" sandbox class="body-frame"></iframe>
            <pre *ngIf="!email.body_html && email.body_text" class="body-text">{{ email.body_text }}</pre>
            <button class="delete-btn" (click)="deleteEmail(email.id)">× delete</button>
          </ng-container>
        </div>
      </ng-container>
    </div>
  `,
  styles: [`
    .erno-devtools {
      position: fixed;
      bottom: 16px;
      right: 16px;
      background: rgba(0,0,0,0.88);
      color: #fff;
      padding: 12px 16px;
      border-radius: 8px;
      font-size: 12px;
      font-family: monospace;
      z-index: 9999;
      display: flex;
      flex-direction: column;
      gap: 6px;
      width: 220px;
      max-height: 480px;
      overflow-y: auto;
    }
    .erno-devtools.wide { width: 380px; }
    .header { display: flex; flex-direction: column; gap: 4px; }
    .tabs { display: flex; gap: 4px; margin-top: 4px; }
    .tabs button {
      background: #333; color: #ccc; border: none; border-radius: 4px;
      padding: 2px 8px; cursor: pointer; font-size: 11px; font-family: monospace;
    }
    .tabs button.active { background: #555; color: #fff; }
    .email-toolbar { display: flex; justify-content: flex-end; }
    .empty { color: #888; font-style: italic; }
    .email-row { border-top: 1px solid #333; padding-top: 4px; }
    .email-summary { cursor: pointer; display: flex; gap: 4px; align-items: baseline; }
    .arrow { flex-shrink: 0; }
    .subject { font-weight: bold; flex-shrink: 0; max-width: 140px; overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
    .to { color: #aaa; overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
    .body-frame { width: 100%; height: 200px; border: 1px solid #555; border-radius: 4px; margin-top: 4px; background: #fff; }
    .body-text { white-space: pre-wrap; font-size: 11px; color: #ccc; max-height: 200px; overflow-y: auto; margin: 4px 0; }
    .delete-btn { background: #600; color: #fff; border: none; border-radius: 4px; padding: 2px 6px; cursor: pointer; font-size: 11px; font-family: monospace; margin-top: 4px; }
    button { cursor: pointer; }
  `],
})
export class ErnoDevtoolsComponent implements OnInit {
  private realtime = inject(ErnoRealtimeService);
  private sync = inject(ErnoSyncService);
  private mailService = inject(ErnoDevMailService);

  readonly visible = isDevMode();
  readonly syncStatus$ = this.sync.status$;
  wsStatus = 'disconnected';

  tab: Tab = 'status';
  emails: MockEmail[] = [];
  expanded: string | null = null;

  ngOnInit(): void {
    // WS connection state will be surfaced via ErnoRealtimeService in a later iteration
  }

  forceSync(): void {
    this.sync.pullDelta();
  }

  switchToEmails(): void {
    this.tab = 'emails';
    this.loadEmails();
  }

  toggle(id: string): void {
    this.expanded = this.expanded === id ? null : id;
  }

  loadEmails(): void {
    this.mailService.list().subscribe(emails => {
      this.emails = emails;
    });
  }

  deleteEmail(id: string): void {
    this.mailService.delete(id).subscribe(() => {
      this.emails = this.emails.filter(e => e.id !== id);
      if (this.expanded === id) this.expanded = null;
    });
  }

  clearAll(): void {
    this.mailService.clear().subscribe(() => {
      this.emails = [];
      this.expanded = null;
    });
  }
}
