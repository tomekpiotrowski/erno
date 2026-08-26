import { ChangeDetectionStrategy, Component, input, output } from '@angular/core';
import { MockEmail } from '../erno-dev-mail.service';
import { ERNO_DEVTOOLS_STYLES } from '../erno-devtools.styles';
import { EmailAuthLink, formatClock, parseEmailAuthLink } from '../erno-devtools.util';

@Component({
  selector: 'erno-devtools-emails-tab',
  imports: [],
  changeDetection: ChangeDetectionStrategy.OnPush,
  host: { style: 'display: contents' },
  styles: [ERNO_DEVTOOLS_STYLES],
  template: `
    @if (emails().length === 0) {
      <div class="empty">
        <span class="empty-title">Outbox empty</span>
        <span class="empty-sub">Mail the app sends in dev lands here instead of going out.</span>
      </div>
    }
    @for (email of emails(); track email.id) {
      <div
        class="erow"
        (click)="openEmail.emit(email)"
        (keydown.enter)="openEmail.emit(email)"
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
            @if (authLink(email); as link) {
              <button
                type="button"
                class="ghost sm"
                (click)="onAuthLink(link, $event)"
              >
                {{ link.kind === 'verify' ? 'verify' : 'open reset ↗' }}
              </button>
            }
            <button type="button" class="ghost sm" (click)="openEmail.emit(email); $event.stopPropagation()">
              open ↗
            </button>
            <button
              type="button"
              class="ghost sm mute"
              (click)="deleteEmail.emit(email.id); $event.stopPropagation()"
            >
              ✕
            </button>
          </span>
        </span>
      </div>
    }
  `,
})
export class ErnoDevtoolsEmailsTab {
  readonly emails = input.required<MockEmail[]>();
  readonly unread = input.required<Set<string>>();
  readonly openEmail = output<MockEmail>();
  readonly deleteEmail = output<string>();
  readonly verify = output<string>();
  readonly openReset = output<string>();

  isUnread(id: string): boolean {
    return this.unread().has(id);
  }

  clock(value: string): string {
    return formatClock(value);
  }

  authLink(email: MockEmail): EmailAuthLink | null {
    return parseEmailAuthLink(email.body_html) ?? parseEmailAuthLink(email.body_text);
  }

  onAuthLink(link: EmailAuthLink, event: Event): void {
    event.stopPropagation();
    if (link.kind === 'verify') this.verify.emit(link.token);
    else this.openReset.emit(link.url);
  }
}
