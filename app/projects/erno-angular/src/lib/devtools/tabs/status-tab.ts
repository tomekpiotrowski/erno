import { ChangeDetectionStrategy, Component, input, output } from '@angular/core';
import { SyncStatus } from '../../sync/erno-sync.service';
import { ERNO_DEVTOOLS_STYLES } from '../erno-devtools.styles';
import { StatusRow, toneColor } from '../erno-devtools.util';

@Component({
  selector: 'erno-devtools-status-tab',
  imports: [],
  changeDetection: ChangeDetectionStrategy.OnPush,
  host: { style: 'display: contents' },
  styles: [ERNO_DEVTOOLS_STYLES],
  template: `
    @for (row of rows(); track row.key) {
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
        (click)="resync.emit()"
        [disabled]="syncBusy()"
      >
        @if (syncBusy()) {
          <span class="spin" aria-hidden="true"></span>
        }
        {{ syncBusy() ? 'Re-syncing' : (syncStatus() === 'error' ? 'Force re-sync' : 'Re-sync') }}
      </button>
      <button type="button" class="ghost sm" (click)="toggleNetwork.emit()">
        {{ online() ? 'simulate offline' : 'go online' }}
      </button>
      <span class="sync-note">{{ syncHint() }}</span>
    </div>
  `,
})
export class ErnoDevtoolsStatusTab {
  readonly rows = input.required<StatusRow[]>();
  readonly syncBusy = input(false);
  readonly syncStatus = input<SyncStatus>('idle');
  readonly syncHint = input('');
  readonly online = input(true);
  readonly resync = output<void>();
  readonly toggleNetwork = output<void>();

  readonly toneColor = toneColor;
}
