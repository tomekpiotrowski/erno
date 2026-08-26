import { ChangeDetectionStrategy, Component, input, output } from '@angular/core';
import { ERNO_DEVTOOLS_STYLES } from '../erno-devtools.styles';
import {
  JobGroup,
  JobKindFilter,
  formatMs,
  groupRuns,
  groupTiming,
  statusLabel,
  statusTone,
  toneColor,
} from '../erno-devtools.util';

@Component({
  selector: 'erno-devtools-jobs-tab',
  imports: [],
  changeDetection: ChangeDetectionStrategy.OnPush,
  host: { style: 'display: contents' },
  styles: [ERNO_DEVTOOLS_STYLES],
  template: `
    <div class="jbar">
      <input
        class="filter"
        placeholder="filter jobs"
        [value]="query()"
        (input)="queryChange.emit($any($event.target).value)"
      />
      @for (f of filterDefs; track f.key) {
        <button
          type="button"
          class="chip"
          [class.on]="filter() === f.key"
          (click)="filterChange.emit(f.key)"
        >
          {{ f.label }}
        </button>
      }
    </div>
    @if (groups().length === 0) {
      <div class="empty">
        <span class="empty-title">
          @if (query().trim()) {
            Nothing matches “{{ query().trim() }}”
          } @else {
            No jobs.
          }
        </span>
        <span class="empty-sub">{{ emptyHint() }}</span>
      </div>
    }
    @for (g of groups(); track g.type) {
      <div class="jkind">
        <div
          class="jrow"
          (click)="toggleGroup.emit(g.type)"
          role="button"
          tabindex="0"
          (keydown.enter)="toggleGroup.emit(g.type)"
        >
          <span class="caret" [class.open]="expanded().has(g.type)">▸</span>
          <span class="jname">
            <span>{{ g.type }}</span>
            @if (g.runCount > 1) {
              <span class="xcount">×{{ g.runCount }}</span>
            }
          </span>
          <span
            class="jstat"
            [class.pulse]="g.status === 'running'"
            [style.color]="toneColor(statusTone(g.status))"
          >
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
                    <button
                      type="button"
                      class="ghost sm"
                      (click)="retryJob.emit(failedId); $event.stopPropagation()"
                    >
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
  `,
})
export class ErnoDevtoolsJobsTab {
  readonly groups = input.required<JobGroup[]>();
  readonly query = input('');
  readonly filter = input<JobKindFilter>('all');
  readonly expanded = input.required<Set<string>>();
  readonly emptyHint = input('');
  readonly queryChange = output<string>();
  readonly filterChange = output<JobKindFilter>();
  readonly toggleGroup = output<string>();
  readonly retryJob = output<string>();

  readonly filterDefs: { key: JobKindFilter; label: string }[] = [
    { key: 'all', label: 'all' },
    { key: 'attention', label: 'attention' },
    { key: 'failed', label: 'failed' },
  ];

  readonly groupRuns = groupRuns;
  readonly groupTiming = groupTiming;
  readonly statusLabel = statusLabel;
  readonly statusTone = statusTone;
  readonly formatMs = formatMs;
  readonly toneColor = toneColor;
}
