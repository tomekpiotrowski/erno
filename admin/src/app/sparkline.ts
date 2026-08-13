import { Component, Input } from '@angular/core';
import { PromPoint } from './core/prometheus';

@Component({
  selector: 'app-sparkline',
  template: `
    @if (points.length) {
      <svg viewBox="0 0 120 32" class="spark" preserveAspectRatio="none">
        <polyline [attr.points]="path" fill="none" stroke="currentColor" stroke-width="1.5" />
      </svg>
    }
  `,
  styles: `
    .spark { width: 120px; height: 32px; color: var(--accent); display: block; }
  `,
})
export class Sparkline {
  @Input({ required: true }) points: PromPoint[] = [];

  get path(): string {
    if (this.points.length === 0) return '';
    const ys = this.points.map((p) => p.v);
    const min = Math.min(...ys);
    const max = Math.max(...ys);
    const span = max - min || 1;
    return this.points
      .map((p, i) => {
        const x = (i / Math.max(this.points.length - 1, 1)) * 120;
        const y = 30 - ((p.v - min) / span) * 28;
        return `${x.toFixed(1)},${y.toFixed(1)}`;
      })
      .join(' ');
  }
}
