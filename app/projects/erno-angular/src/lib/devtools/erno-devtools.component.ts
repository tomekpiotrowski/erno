import {
  ChangeDetectionStrategy,
  Component,
  DestroyRef,
  Inject,
  isDevMode,
  OnInit,
  computed,
  inject,
  signal,
} from '@angular/core';
import { HttpClient } from '@angular/common/http';
import { takeUntilDestroyed } from '@angular/core/rxjs-interop';
import { catchError, of } from 'rxjs';
import { ERNO_CONFIG, ErnoConfig } from '../erno.config';
import { ErnoRealtimeService } from '../realtime/erno-realtime.service';
import { ErnoSyncService, SyncStatus } from '../sync/erno-sync.service';
import { ErnoDevMailService, MockEmail } from './erno-dev-mail.service';
import { DevJob, ErnoDevJobsService } from './erno-dev-jobs.service';
import {
  JobGroup,
  JobKindFilter,
  Tone,
  apiHost,
  filterJobGroups,
  formatClock,
  formatMs,
  formatUptime,
  groupJobs,
  groupRuns,
  statusLabel,
  statusTone,
  syncLabel,
  syncTone,
} from './erno-devtools.util';

type Tab = 'status' | 'emails' | 'jobs';

const OK = 'oklch(0.755 0.085 168)';
const WARN = 'oklch(0.80 0.095 82)';
const ERR = 'oklch(0.695 0.125 22)';
const TONE_COLOR: Record<Tone, string> = { ok: OK, warn: WARN, err: ERR, muted: 'var(--dt-n600)' };
const VERSION = '0.0.1';
const POLL_MS = 4000;
const NOTE_MS = 3000;

@Component({
  selector: 'erno-devtools',
  imports: [],
  changeDetection: ChangeDetectionStrategy.OnPush,
  template: `
    @if (visible) {
      @if (open()) {
        <div
          class="panel"
          [class.tall]="tall()"
          [style.--health]="healthColor()"
          role="dialog"
          aria-label="Erno Devtools"
        >
          <div class="head">
            <span class="health"></span>
            <span class="title">Erno Devtools</span>
            <span class="ver">{{ version }}</span>
            <span class="head-acts">
              <button type="button" class="ghost" (click)="toggleDock()">
                {{ tall() ? 'short' : 'tall' }}
              </button>
              <button type="button" class="ghost icon" (click)="open.set(false)" title="collapse">
                —
              </button>
            </span>
          </div>

          <div class="tabs" role="tablist">
            @for (t of tabDefs(); track t.key) {
              <button
                type="button"
                role="tab"
                class="tab"
                [class.on]="tab() === t.key"
                [attr.aria-selected]="tab() === t.key"
                (click)="selectTab(t.key)"
              >
                <span>{{ t.label }}</span>
                @if (t.count) {
                  <span class="count" [class.err]="t.tone === 'err'" [class.accent]="t.tone === 'accent'">
                    {{ t.count }}
                  </span>
                }
              </button>
            }
          </div>
          <div class="rule"></div>

          <div class="body">
            @if (tab() === 'status') {
              @for (row of statusRows(); track row.key) {
                <div class="srow">
                  <span class="skey">{{ row.key }}</span>
                  <span class="sval">
                    <span class="smain" [style.color]="toneColor(row.tone)">{{ row.val }}</span>
                    @if (row.detail) {
                      <span class="sdetail">{{ row.detail }}</span>
                    }
                  </span>
                  <span class="smeta">{{ row.meta }}</span>
                </div>
              }
              <div class="sync-row">
                <button
                  type="button"
                  class="primary"
                  (click)="forceSync()"
                  [disabled]="syncBusy()"
                >
                  @if (syncBusy()) {
                    <span class="spin" aria-hidden="true"></span>
                  }
                  {{ syncBusy() ? 'Re-syncing' : (syncStatus() === 'error' ? 'Force re-sync' : 'Re-sync') }}
                </button>
                <span class="sync-note">{{ syncHint() }}</span>
              </div>
            }

            @if (tab() === 'emails') {
              @if (emails().length === 0) {
                <div class="empty">
                  <span class="empty-title">Outbox empty</span>
                  <span class="empty-sub">Mail the app sends in dev lands here instead of going out.</span>
                </div>
              }
              @for (email of emails(); track email.id) {
                <div
                  class="erow"
                  (click)="openEmail(email)"
                  (keydown.enter)="openEmail(email)"
                  tabindex="0"
                  role="button"
                >
                  <span class="eline">
                    <span class="esubj" [class.read]="!isUnread(email.id)">{{ email.subject }}</span>
                    @if (isUnread(email.id)) {
                      <span class="udot" aria-label="unread"></span>
                    }
                    <span class="etime">{{ clock(email.created_at) }}</span>
                  </span>
                  <span class="eline">
                    <span class="eto">{{ email.to }}</span>
                    <span class="eact">
                      <button type="button" class="ghost sm" (click)="openEmail(email); $event.stopPropagation()">
                        open ↗
                      </button>
                      <button
                        type="button"
                        class="ghost sm mute"
                        (click)="deleteEmail(email.id); $event.stopPropagation()"
                      >
                        ✕
                      </button>
                    </span>
                  </span>
                </div>
              }
            }

            @if (tab() === 'jobs') {
              <div class="jbar">
                <input
                  class="filter"
                  placeholder="filter jobs"
                  [value]="jobQuery()"
                  (input)="jobQuery.set($any($event.target).value)"
                />
                @for (f of jobFilterDefs; track f.key) {
                  <button
                    type="button"
                    class="chip"
                    [class.on]="jobFilter() === f.key"
                    (click)="jobFilter.set(f.key)"
                  >
                    {{ f.label }}
                  </button>
                }
              </div>
              @if (visibleGroups().length === 0) {
                <div class="empty">
                  <span class="empty-title">
                    @if (jobQuery().trim()) {
                      Nothing matches “{{ jobQuery().trim() }}”
                    } @else {
                      No jobs.
                    }
                  </span>
                  <span class="empty-sub">{{ jobsEmptyHint() }}</span>
                </div>
              }
              @for (g of visibleGroups(); track g.type) {
                <div class="jkind">
                  <div class="jrow" (click)="toggleGroup(g.type)" role="button" tabindex="0"
                    (keydown.enter)="toggleGroup(g.type)">
                    <span class="caret" [class.open]="expanded().has(g.type)">▸</span>
                    <span class="jname">
                      <span>{{ g.type }}</span>
                      @if (g.runCount > 1) {
                        <span class="xcount">×{{ g.runCount }}</span>
                      }
                    </span>
                    <span class="jstat" [class.pulse]="g.status === 'running'" [style.color]="toneColor(statusTone(g.status))">
                      {{ statusLabel(g.status) }}
                    </span>
                    <span class="jtime">{{ groupTiming(g) }}</span>
                  </div>
                  @if (expanded().has(g.type)) {
                    <div class="jexp">
                      @for (run of groupRuns(g); track run.id) {
                        <div class="run">
                          <span class="run-id">{{ run.id }}</span>
                          <span class="run-ms" [class.warn]="run.ms != null && run.ms > 500">{{ formatMs(run.ms) }}</span>
                          <span class="run-st" [style.color]="toneColor(statusTone(run.state))">{{ run.state }}</span>
                        </div>
                      }
                      @if (g.error) {
                        <div class="errbox">
                          <span class="err-msg">{{ g.error }}</span>
                          @if (g.failedJobId; as failedId) {
                            <span class="err-acts">
                              <button type="button" class="ghost sm" (click)="retryJob(failedId); $event.stopPropagation()">
                                retry
                              </button>
                            </span>
                          }
                        </div>
                      }
                    </div>
                  }
                </div>
              }
            }
          </div>

          <div class="foot">
            <span class="fnote">{{ footNote() }}</span>
            <button type="button" class="secondary" (click)="clearAll()">Clear all</button>
          </div>
        </div>
      } @else {
        <button type="button" class="pill" [style.--health]="healthColor()" (click)="open.set(true)">
          <span class="health"></span>
          <span>Erno Devtools</span>
          <span class="pcnt">{{ pillCounts() }}</span>
        </button>
      }
    }
  `,
  styles: `
    :host {
      --dt-bg: #161826;
      --dt-surface: #232532;
      --dt-text: #e9e9ed;
      --dt-accent: #9184d9;
      --dt-accent-200: #e7e5fe;
      --dt-accent-800: #423a6a;
      --dt-accent-100: #f5f4ff;
      --dt-n400: #b2b6ca;
      --dt-n500: #9397ab;
      --dt-n600: #75798c;
      --dt-n700: #595d6c;
      --dt-n800: #3f424d;
      --dt-n900: #292b31;
      --dt-div: color-mix(in srgb, #e9e9ed 16%, transparent);
      --dt-font: Inter, system-ui, sans-serif;
      --dt-mono: "JetBrains Mono", ui-monospace, SFMono-Regular, Menlo, monospace;
      --dt-ok: oklch(0.755 0.085 168);
      --dt-warn: oklch(0.80 0.095 82);
      --dt-err: oklch(0.695 0.125 22);
      --dt-radius: 14px;
    }

    .panel, .pill {
      position: fixed;
      right: 28px;
      bottom: 28px;
      z-index: 9999;
      color: var(--dt-text);
      font-family: var(--dt-font);
      animation: dt-rise 0.22s ease-out;
    }

    .panel {
      width: min(400px, calc(100vw - 32px));
      display: flex;
      flex-direction: column;
      border-radius: var(--dt-radius);
      background: var(--dt-bg);
      box-shadow: 0 0 0 1px var(--dt-n800), 0 16px 40px rgba(0, 0, 0, 0.65);
      overflow: hidden;
    }

    .head, .foot {
      display: flex;
      align-items: center;
      gap: 6px;
      padding: 11px 12px 11px 14px;
      background: var(--dt-surface);
    }
    .head { border-bottom: 1px solid var(--dt-div); }
    .foot { border-top: 1px solid var(--dt-div); padding: 8px 12px 8px 14px; }

    .health {
      width: 7px;
      height: 7px;
      border-radius: 50%;
      flex: none;
      background: var(--health, var(--dt-ok));
      box-shadow: 0 0 0 3px color-mix(in srgb, var(--health, var(--dt-ok)) 20%, transparent);
    }

    .title {
      font-size: 13px;
      font-weight: 500;
      letter-spacing: 0.01em;
    }
    .ver {
      font-family: var(--dt-mono);
      font-size: 10px;
      color: var(--dt-n600);
    }
    .head-acts { margin-left: auto; display: flex; align-items: center; gap: 2px; }

    .tabs {
      display: flex;
      gap: 2px;
      padding: 9px 12px 0;
    }
    .tab {
      display: flex;
      align-items: center;
      gap: 6px;
      padding: 6px 11px;
      cursor: pointer;
      font: 12px/1.2 var(--dt-font);
      border: none;
      border-radius: 4px;
      background: transparent;
      color: var(--dt-n500);
    }
    .tab.on {
      background: color-mix(in srgb, var(--dt-accent) 16%, transparent);
      color: var(--dt-accent-200);
      box-shadow: inset 0 0 0 1px color-mix(in srgb, var(--dt-accent) 50%, transparent);
    }
    .count {
      font-family: var(--dt-mono);
      font-size: 10px;
      padding: 1px 5px;
      border-radius: 999px;
      background: var(--dt-n900);
      color: var(--dt-n500);
    }
    .count.accent { background: var(--dt-accent-800); color: var(--dt-accent-100); }
    .count.err { background: color-mix(in srgb, var(--dt-err) 22%, transparent); color: var(--dt-err); }

    .rule { height: 1px; background: var(--dt-div); margin-top: 9px; }

    .body {
      max-height: 300px;
      overflow-y: auto;
      display: flex;
      flex-direction: column;
      scrollbar-width: thin;
      scrollbar-color: var(--dt-n800) transparent;
    }
    .panel.tall .body { max-height: 460px; }

    .srow {
      display: grid;
      grid-template-columns: 76px minmax(0, 1fr) auto;
      gap: 0 10px;
      align-items: baseline;
      padding: 8px 14px;
      border-bottom: 1px solid color-mix(in srgb, var(--dt-div) 55%, transparent);
    }
    .srow:hover { background: color-mix(in srgb, var(--dt-text) 5%, transparent); }
    .skey { font-family: var(--dt-mono); font-size: 11px; color: var(--dt-n600); }
    .sval { display: flex; flex-direction: column; gap: 2px; min-width: 0; }
    .smain { font-size: 13px; font-family: var(--dt-mono); }
    .sdetail { font-size: 11px; color: var(--dt-n600); text-wrap: pretty; }
    .smeta { font-family: var(--dt-mono); font-size: 11px; color: var(--dt-n700); white-space: nowrap; }

    .sync-row {
      display: flex;
      align-items: center;
      gap: 6px;
      padding: 12px 14px 10px;
    }
    .sync-note { font-size: 11px; color: var(--dt-n600); }

    .erow {
      display: flex;
      flex-direction: column;
      gap: 3px;
      padding: 10px 14px;
      border-bottom: 1px solid color-mix(in srgb, var(--dt-div) 55%, transparent);
      cursor: pointer;
    }
    .erow:hover { background: color-mix(in srgb, var(--dt-text) 5%, transparent); }
    .erow:hover .eact { opacity: 1; }
    .eline { display: flex; align-items: baseline; gap: 8px; }
    .esubj {
      font-size: 13px;
      font-weight: 500;
      min-width: 0;
      overflow: hidden;
      text-overflow: ellipsis;
      white-space: nowrap;
    }
    .esubj.read { color: var(--dt-n400); }
    .udot { width: 5px; height: 5px; border-radius: 50%; background: var(--dt-accent); flex: none; }
    .etime { margin-left: auto; font-family: var(--dt-mono); font-size: 11px; color: var(--dt-n700); flex: none; }
    .eto {
      font-family: var(--dt-mono);
      font-size: 11px;
      color: var(--dt-n500);
      min-width: 0;
      overflow: hidden;
      text-overflow: ellipsis;
      white-space: nowrap;
    }
    .eact {
      margin-left: auto;
      display: flex;
      gap: 8px;
      opacity: 0.35;
      transition: opacity 0.15s;
      flex: none;
    }

    .jbar {
      display: flex;
      align-items: center;
      gap: 8px;
      padding: 9px 14px;
      border-bottom: 1px solid var(--dt-div);
    }
    .filter {
      flex: 1;
      min-width: 0;
      height: 28px;
      padding: 0 8px;
      font: 12px var(--dt-mono);
      color: var(--dt-text);
      background: var(--dt-bg);
      border: 1px solid var(--dt-div);
      border-radius: 8px;
    }
    .filter:focus-visible { outline: 2px solid var(--dt-accent); outline-offset: 0; border-color: var(--dt-accent); }
    .chip {
      padding: 4px 8px;
      font: 11px var(--dt-mono);
      cursor: pointer;
      border-radius: 4px;
      border: 1px solid var(--dt-div);
      background: transparent;
      color: var(--dt-n600);
      white-space: nowrap;
      flex: none;
    }
    .chip.on { border-color: var(--dt-accent); color: var(--dt-accent); }

    .jkind { border-bottom: 1px solid color-mix(in srgb, var(--dt-div) 55%, transparent); }
    .jrow {
      display: grid;
      grid-template-columns: 12px minmax(0, 1fr) auto auto;
      gap: 0 9px;
      align-items: center;
      padding: 7px 14px;
      cursor: pointer;
    }
    .jrow:hover { background: color-mix(in srgb, var(--dt-text) 5%, transparent); }
    .caret {
      font-size: 9px;
      color: var(--dt-n600);
      transition: transform 0.15s;
    }
    .caret.open { transform: rotate(90deg); }
    .jname {
      display: flex;
      align-items: baseline;
      gap: 7px;
      min-width: 0;
      font-family: var(--dt-mono);
      font-size: 12px;
      font-weight: 500;
    }
    .jname > span:first-child {
      overflow: hidden;
      text-overflow: ellipsis;
      white-space: nowrap;
    }
    .xcount {
      font-family: var(--dt-mono);
      font-size: 10px;
      padding: 1px 5px;
      border-radius: 4px;
      background: var(--dt-n900);
      color: var(--dt-n500);
      flex: none;
    }
    .jstat { font-size: 11px; white-space: nowrap; }
    .jstat.pulse { animation: dt-pulse 1.4s infinite; }
    .jtime { font-family: var(--dt-mono); font-size: 11px; color: var(--dt-n700); white-space: nowrap; }

    .jexp {
      display: flex;
      flex-direction: column;
      padding: 2px 14px 9px 35px;
      animation: dt-rise 0.18s ease-out;
    }
    .run {
      display: grid;
      grid-template-columns: minmax(0, 1fr) 54px 62px;
      gap: 0 8px;
      align-items: baseline;
      padding: 3px 0;
      font-family: var(--dt-mono);
      font-size: 11px;
    }
    .run-id { color: var(--dt-n600); overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
    .run-ms { color: var(--dt-n600); text-align: right; }
    .run-ms.warn { color: var(--dt-warn); }
    .run-st { text-align: right; }

    .errbox {
      margin-top: 7px;
      padding: 8px 10px;
      border-radius: 8px;
      border: 1px solid color-mix(in srgb, var(--dt-err) 45%, transparent);
      background: color-mix(in srgb, var(--dt-err) 9%, transparent);
      display: flex;
      flex-direction: column;
      gap: 3px;
    }
    .err-msg { font-family: var(--dt-mono); font-size: 11px; color: #d2cefd; text-wrap: pretty; }
    .err-acts { display: flex; gap: 6px; padding-top: 2px; }

    .empty {
      padding: 28px 14px;
      display: flex;
      flex-direction: column;
      gap: 5px;
      align-items: center;
      text-align: center;
    }
    .empty-title { font-size: 13px; color: var(--dt-n500); }
    .empty-sub { font-size: 11px; color: var(--dt-n700); }

    .fnote {
      font-family: var(--dt-mono);
      font-size: 11px;
      color: var(--dt-n600);
      min-width: 0;
      overflow: hidden;
      text-overflow: ellipsis;
      white-space: nowrap;
    }

    .pill {
      display: flex;
      align-items: center;
      gap: 9px;
      padding: 8px 13px;
      border-radius: 999px;
      border: 1px solid var(--dt-n800);
      background: var(--dt-surface);
      box-shadow: 0 0 0 1px #595d6c, 0 6px 18px rgba(0, 0, 0, 0.55);
      color: var(--dt-text);
      font: 12px var(--dt-font);
      cursor: pointer;
    }
    .pill:hover { border-color: var(--dt-accent); }
    .pcnt { font-family: var(--dt-mono); font-size: 11px; color: var(--dt-n500); }

    button { font-family: inherit; }
    .ghost, .primary, .secondary {
      display: inline-flex;
      align-items: center;
      justify-content: center;
      cursor: pointer;
      background: transparent;
      border: 1px solid transparent;
      border-radius: 8px;
    }
    .ghost {
      height: 26px;
      padding: 0 8px;
      font-size: 11px;
      color: var(--dt-n500);
    }
    .ghost.icon { width: 26px; padding: 0; font-size: 14px; }
    .ghost.sm { height: 22px; padding: 0 6px; font-size: 11px; }
    .ghost.mute { color: var(--dt-n500); }
    .ghost:hover { background: color-mix(in srgb, var(--dt-accent) 10%, transparent); }
    .primary {
      height: 32px;
      padding: 0 12px;
      font-size: 12px;
      color: var(--dt-accent);
      border-color: var(--dt-accent);
    }
    .primary:hover { background: color-mix(in srgb, var(--dt-accent) 12%, transparent); }
    .primary:disabled { opacity: 0.45; cursor: not-allowed; }
    .secondary {
      margin-left: auto;
      height: 26px;
      padding: 0 10px;
      font-size: 11px;
      flex: none;
      border-color: var(--dt-div);
      color: var(--dt-text);
    }
    .secondary:hover { background: color-mix(in srgb, var(--dt-text) 7%, transparent); }

    .spin {
      display: inline-block;
      width: 10px;
      height: 10px;
      margin-right: 7px;
      border: 1.5px solid var(--dt-accent);
      border-top-color: transparent;
      border-radius: 50%;
      animation: dt-spin 0.7s linear infinite;
    }

    :host :focus-visible { outline: 2px solid var(--dt-accent); outline-offset: 2px; }

    @keyframes dt-rise {
      from { opacity: 0; transform: translateY(6px); }
      to { opacity: 1; transform: none; }
    }
    @keyframes dt-pulse {
      0%, 100% { opacity: 1; }
      50% { opacity: 0.35; }
    }
    @keyframes dt-spin { to { transform: rotate(360deg); } }
  `,
})
export class ErnoDevtoolsComponent implements OnInit {
  private readonly destroyRef = inject(DestroyRef);

  readonly visible = isDevMode();
  readonly version = VERSION;
  readonly jobFilterDefs: { key: JobKindFilter; label: string }[] = [
    { key: 'all', label: 'all' },
    { key: 'attention', label: 'attention' },
    { key: 'failed', label: 'failed' },
  ];

  readonly open = signal(true);
  readonly tall = signal(false);
  readonly tab = signal<Tab>('status');
  readonly emails = signal<MockEmail[]>([]);
  readonly jobs = signal<DevJob[]>([]);
  readonly expanded = signal<Set<string>>(new Set());
  readonly jobQuery = signal('');
  readonly jobFilter = signal<JobKindFilter>('all');
  readonly unread = signal<Set<string>>(new Set());
  readonly wsConnected = signal(false);
  readonly syncStatus = signal<SyncStatus>('idle');
  readonly syncBusy = signal(false);
  readonly apiReady = signal<boolean | null>(null);
  readonly note = signal('');
  readonly connectedSince = signal<number | null>(null);
  readonly syncAt = signal<number | null>(null);

  private seenEmailIds = new Set<string>();
  private emailsPrimed = false;
  private noteTimer: ReturnType<typeof setTimeout> | null = null;
  private pollTimer: ReturnType<typeof setInterval> | null = null;

  readonly groups = computed(() => groupJobs(this.jobs()));
  readonly visibleGroups = computed(() =>
    filterJobGroups(this.groups(), this.jobQuery(), this.jobFilter()),
  );
  readonly failing = computed(() => this.jobs().filter(j => j.status === 'failed').length);
  readonly running = computed(
    () => this.jobs().filter(j => j.status === 'running' || j.status === 'pending_retry').length,
  );
  readonly unreadCount = computed(() => this.unread().size);

  readonly healthColor = computed(() => {
    if (this.syncStatus() === 'error' || this.failing() > 0) return ERR;
    if (this.running() > 0 || this.syncStatus() === 'syncing') return WARN;
    return OK;
  });

  readonly tabDefs = computed(() => {
    const emails = this.emails().length;
    const runs = this.jobs().length;
    return [
      {
        key: 'status' as const,
        label: 'Status',
        count: this.syncStatus() === 'error' ? '!' : null,
        tone: 'err' as const,
      },
      {
        key: 'emails' as const,
        label: 'Emails',
        count: emails ? String(emails) : null,
        tone: this.unreadCount() ? ('accent' as const) : ('muted' as const),
      },
      {
        key: 'jobs' as const,
        label: 'Jobs',
        count: runs ? String(runs) : null,
        tone: this.failing() ? ('err' as const) : ('muted' as const),
      },
    ];
  });

  readonly statusRows = computed(() => {
    const up = this.connectedSince();
    const upLabel = up != null ? formatUptime(up, Date.now()) : '—';
    const host = apiHost(this.config.baseUrl);
    const wsHost = this.config.wsUrl.replace(/^wss?:\/\//, '');
    const sync = this.syncStatus();
    const api = this.apiReady();
    return [
      {
        key: 'websocket',
        val: this.wsConnected() ? 'connected' : 'disconnected',
        tone: this.wsConnected() ? ('ok' as const) : ('err' as const),
        meta: `${wsHost} · ${upLabel}`,
        detail: '',
      },
      {
        key: 'sync',
        val: syncLabel(sync),
        tone: syncTone(sync),
        meta: this.syncAt() ? formatClock(this.syncAt()!) : '',
        detail:
          sync === 'error'
            ? 'last pull failed — force a re-sync once the API is reachable'
            : sync === 'offline'
              ? 'waiting for a network path'
              : '',
      },
      {
        key: 'api',
        val: api == null ? 'probing' : api ? 'ready' : 'unreachable',
        tone: api === false ? ('err' as const) : api ? ('ok' as const) : ('warn' as const),
        meta: host,
        detail: '',
      },
      {
        key: 'queue',
        val: `${this.running()} running · ${this.failing()} failed`,
        tone: this.failing() ? ('err' as const) : ('ok' as const),
        meta: `${this.jobs().length} held`,
        detail: '',
      },
    ];
  });

  readonly footNote = computed(() => {
    if (this.note()) return this.note();
    const up = this.connectedSince();
    const upLabel = up != null ? formatUptime(up, Date.now()) : 'down';
    switch (this.tab()) {
      case 'emails':
        return this.emails().length
          ? `${this.emails().length} held · ${this.unreadCount()} unread · nothing left the machine`
          : 'nothing held';
      case 'jobs':
        return `${this.visibleGroups().length} of ${this.groups().length} kinds · ${this.jobs().length} runs · ${this.failing()} failing`;
      default:
        return `ws ${this.wsConnected() ? 'up ' + upLabel : 'down'} · ${this.jobs().length} runs · ${this.emails().length} mails held`;
    }
  });

  readonly pillCounts = computed(() => {
    const bits: string[] = [];
    if (this.unreadCount()) bits.push(`${this.unreadCount()} mail`);
    if (this.failing()) bits.push(`${this.failing()} failed`);
    else bits.push(`${this.jobs().length} runs`);
    return bits.join(' · ');
  });

  readonly syncHint = computed(() => {
    if (this.note()) return this.note();
    if (this.syncStatus() === 'error') return 'pulls missed changes, then reconnects';
    const at = this.syncAt();
    return at ? `last synced ${formatClock(at)}` : '';
  });

  readonly jobsEmptyHint = computed(() =>
    this.jobs().length
      ? `${this.groups().length} kinds held · ${this.jobs().length} runs`
      : 'history cleared',
  );

  constructor(
    private sync: ErnoSyncService,
    private realtime: ErnoRealtimeService,
    private mailService: ErnoDevMailService,
    private jobsService: ErnoDevJobsService,
    private http: HttpClient,
    @Inject(ERNO_CONFIG) private config: ErnoConfig,
  ) {}

  ngOnInit(): void {
    if (!this.visible) return;
    this.realtime.connected$.pipe(takeUntilDestroyed(this.destroyRef)).subscribe(connected => {
      this.wsConnected.set(connected);
      if (connected) this.connectedSince.set(Date.now());
    });
    this.sync.status$.pipe(takeUntilDestroyed(this.destroyRef)).subscribe(status => {
      this.syncStatus.set(status);
      this.syncAt.set(Date.now());
    });
    this.refresh();
    this.pollTimer = setInterval(() => this.refresh(), POLL_MS);
    this.destroyRef.onDestroy(() => {
      if (this.pollTimer) clearInterval(this.pollTimer);
      if (this.noteTimer) clearTimeout(this.noteTimer);
    });
  }

  selectTab(tab: Tab): void {
    this.tab.set(tab);
  }

  toggleDock(): void {
    this.tall.update(v => !v);
  }

  toneColor(tone: Tone): string {
    return TONE_COLOR[tone];
  }

  clock(value: string): string {
    return formatClock(value);
  }

  isUnread(id: string): boolean {
    return this.unread().has(id);
  }

  forceSync(): void {
    if (this.syncBusy()) return;
    this.syncBusy.set(true);
    void this.sync.pullDelta().finally(() => {
      this.syncBusy.set(false);
      this.say(this.syncStatus() === 'error' ? 're-sync failed' : 'caught up');
    });
  }

  openEmail(email: MockEmail): void {
    this.unread.update(set => {
      const next = new Set(set);
      next.delete(email.id);
      return next;
    });
    window.open(this.mailService.previewUrl(email.id), '_blank', 'noopener');
  }

  deleteEmail(id: string): void {
    this.mailService.delete(id).subscribe({
      next: () => this.emails.update(list => list.filter(e => e.id !== id)),
    });
  }

  toggleGroup(type: string): void {
    this.expanded.update(set => {
      const next = new Set(set);
      if (next.has(type)) next.delete(type);
      else next.add(type);
      return next;
    });
  }

  retryJob(id: string): void {
    this.jobsService.retry(id).subscribe({
      next: () => {
        this.say('re-enqueued');
        this.loadJobs();
      },
    });
  }

  clearAll(): void {
    const tab = this.tab();
    if (tab === 'emails') {
      this.mailService.clear().subscribe({
        next: () => {
          this.emails.set([]);
          this.unread.set(new Set());
          this.say('outbox cleared');
        },
      });
    } else if (tab === 'jobs') {
      this.jobsService.clear().subscribe({
        next: () => {
          this.jobs.set([]);
          this.expanded.set(new Set());
          this.say('job history cleared');
        },
      });
    } else {
      this.say('status counters reset');
    }
  }

  groupTiming(group: JobGroup): string {
    if (group.status === 'running') {
      const t = group.jobs[0]?.updated_at ?? group.jobs[0]?.created_at;
      return t ? formatClock(t) : '';
    }
    if (group.avgMs == null) return '';
    return group.runCount > 1 ? `avg ${group.avgMs}ms` : `${group.avgMs}ms`;
  }

  readonly groupRuns = groupRuns;
  readonly statusLabel = statusLabel;
  readonly statusTone = statusTone;
  readonly formatMs = formatMs;

  private refresh(): void {
    this.loadEmails();
    this.loadJobs();
    this.probeApi();
  }

  private loadEmails(): void {
    this.mailService.list().subscribe({
      next: emails => {
        const newestFirst = [...emails].reverse();
        if (!this.emailsPrimed) {
          newestFirst.forEach(e => this.seenEmailIds.add(e.id));
          this.emailsPrimed = true;
        }
        this.unread.update(set => {
          const next = new Set(set);
          for (const e of newestFirst) {
            if (!this.seenEmailIds.has(e.id)) next.add(e.id);
            this.seenEmailIds.add(e.id);
          }
          return next;
        });
        this.emails.set(newestFirst);
      },
    });
  }

  private loadJobs(): void {
    this.jobsService.list().subscribe({
      next: jobs => {
        this.jobs.set(jobs.map(j => ({ ...j, executions: j.executions ?? [] })));
      },
    });
  }

  private probeApi(): void {
    this.http
      .get(`${this.config.baseUrl}/liveness`, { responseType: 'text' })
      .pipe(catchError(() => of(null)))
      .subscribe(body => this.apiReady.set(body !== null));
  }

  private say(msg: string): void {
    if (this.noteTimer) clearTimeout(this.noteTimer);
    this.note.set(msg);
    this.noteTimer = setTimeout(() => this.note.set(''), NOTE_MS);
  }
}
