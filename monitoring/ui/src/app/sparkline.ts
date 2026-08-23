import { Component, Input, ChangeDetectionStrategy } from '@angular/core';
import { PromPoint } from './core/api';

@Component({
  selector: 'app-sparkline',
  template: `
    @if (points.length) {
      <svg [attr.viewBox]="'0 0 ' + w + ' ' + h" class="spark" preserveAspectRatio="none">
        <path [attr.d]="area" fill="currentColor" opacity="0.12" />
        <path [attr.d]="line" fill="none" stroke="currentColor" stroke-width="1.7"
              stroke-linejoin="round" />
      </svg>
    }
  `,
  changeDetection: ChangeDetectionStrategy.OnPush,
  styles: `
    .spark { width: 108px; height: 30px; color: var(--cb-accent); display: block; }
  `,
})
export class Sparkline {
  @Input({ required: true }) points: PromPoint[] = [];
  readonly w = 108;
  readonly h = 30;

  get line(): string {
    return this.coords()
      .map((c, i) => `${i ? 'L' : 'M'}${c}`)
      .join('');
  }

  get area(): string {
    const pts = this.coords();
    if (!pts.length) return '';
    return `${this.line}L${this.w},${this.h}L0,${this.h}Z`;
  }

  private coords(): string[] {
    if (this.points.length === 0) return [];
    const ys = this.points.map((p) => p.v);
    const min = Math.min(...ys);
    const max = Math.max(...ys);
    const span = max - min || 1;
    return this.points.map((p, i) => {
      const x = (i / Math.max(this.points.length - 1, 1)) * this.w;
      const y = this.h - ((p.v - min) / span) * (this.h - 4) - 2;
      return `${x.toFixed(1)},${y.toFixed(1)}`;
    });
  }
}
