import {
  ChangeDetectionStrategy,
  Component,
  DestroyRef,
  OnInit,
  inject,
  input,
  output,
  signal,
} from '@angular/core';
import { takeUntilDestroyed } from '@angular/core/rxjs-interop';
import { ErnoSyncService, SyncEntityInfo, SyncStatus } from '../../sync/erno-sync.service';
import { ERNO_DEVTOOLS_STYLES } from '../erno-devtools.styles';
import { LoggedPushEvent, formatClock, shortId, toneColor } from '../erno-devtools.util';

@Component({
  selector: 'erno-devtools-sync-tab',
  imports: [],
  changeDetection: ChangeDetectionStrategy.OnPush,
  host: { style: 'display: contents' },
  styles: [ERNO_DEVTOOLS_STYLES],
  template: `
    @if (entities().length === 0) {
      <div class="empty">
        <span class="empty-title">{{ started() ? 'No entities registered' : 'Sync has not started' }}</span>
        <span class="empty-sub">Call register() then start() on ErnoSyncService.</span>
      </div>
    }
    @for (e of entities(); track e.entity) {
      <div class="srow">
        <span class="skey">{{ e.entity }}</span>
        <span class="sval">
          <span class="smain" [style.color]="toneColor(e.lastError ? 'err' : 'ok')">
            seq {{ e.lastSyncSeq }}
          </span>
          <span class="sdetail">{{ e.lastError ?? e.deltaPath }}</span>
        </span>
        <span class="smeta">{{ e.lastPullAt ? clock(e.lastPullAt) : 'never' }}</span>
      </div>
      <div class="sync-row">
        <button type="button" class="ghost sm" (click)="reset(e.entity)" [disabled]="busy()">
          reset cursor
        </button>
      </div>
    }
    <div class="acts">
      <button type="button" class="primary" (click)="resync()" [disabled]="busy()">
        @if (busy()) {
          <span class="spin" aria-hidden="true"></span>
        }
        Re-sync
      </button>
      <button type="button" class="ghost sm" (click)="clearLog.emit(); note.emit('push log cleared')" [disabled]="events().length === 0">
        clear log
      </button>
    </div>
    @for (ev of events(); track eventKey(ev, $index)) {
      <div class="run" style="padding: 4px 14px; grid-template-columns: minmax(0,1fr) 54px 62px 58px;">
        <span class="run-id">{{ ev.entity }} {{ shortId(ev.id) }}</span>
        <span class="run-ms">#{{ ev.sync_seq }}</span>
        <span class="run-st" [style.color]="toneColor(ev.deleted ? 'err' : 'ok')">
          {{ ev.deleted ? 'deleted' : 'upsert' }}
        </span>
        <span class="run-ms">{{ clock(ev.at) }}</span>
      </div>
    }
  `,
})
export class ErnoDevtoolsSyncTab implements OnInit {
  private readonly sync = inject(ErnoSyncService);
  private readonly destroyRef = inject(DestroyRef);

  readonly events = input<LoggedPushEvent[]>([]);
  readonly note = output<string>();
  readonly clearLog = output<void>();

  readonly entities = signal<SyncEntityInfo[]>([]);
  readonly started = signal(false);
  readonly busy = signal(false);

  readonly toneColor = toneColor;
  readonly shortId = shortId;

  ngOnInit(): void {
    this.started.set(this.sync.isStarted);
    void this.reload();
    this.sync.status$.pipe(takeUntilDestroyed(this.destroyRef)).subscribe((status: SyncStatus) => {
      this.started.set(this.sync.isStarted);
      if (status !== 'syncing') void this.reload();
    });
  }

  clock(value: number): string {
    return formatClock(value);
  }

  eventKey(event: LoggedPushEvent, index: number): string {
    return `${event.entity}:${event.id}:${event.sync_seq}:${event.at}:${index}`;
  }

  async resync(): Promise<void> {
    if (this.busy()) return;
    this.busy.set(true);
    try {
      await this.sync.pullDelta();
      await this.reload();
      this.note.emit(this.sync.lastError ? 're-sync failed' : 'caught up');
    } finally {
      this.busy.set(false);
    }
  }

  async reset(entity: string): Promise<void> {
    if (this.busy()) return;
    this.busy.set(true);
    try {
      await this.sync.resetCursor(entity);
      await this.reload();
      this.note.emit(`reset ${entity}`);
    } finally {
      this.busy.set(false);
    }
  }

  private async reload(): Promise<void> {
    this.entities.set(await this.sync.entities());
  }
}
