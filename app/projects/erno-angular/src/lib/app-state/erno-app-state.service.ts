import { Injectable, NgZone, OnDestroy } from '@angular/core';
import { BehaviorSubject, Observable, Subject } from 'rxjs';
import { distinctUntilChanged, filter, map, pairwise } from 'rxjs/operators';

export type ErnoAppState = 'active' | 'background';

/** Minimal shape of the `@capacitor/app` plugin we rely on. */
interface CapacitorAppPlugin {
  addListener(
    eventName: 'appStateChange',
    listener: (state: { isActive: boolean }) => void,
  ): Promise<PluginListenerHandle> | PluginListenerHandle;
}

interface PluginListenerHandle {
  remove(): Promise<void> | void;
}

/**
 * Tracks whether the app is in the foreground (`active`) or background.
 *
 * On Capacitor native platforms it wraps `@capacitor/app`'s `appStateChange`
 * event; on the web (or if the plugin is not installed) it falls back to the
 * `visibilitychange` event. `@capacitor/app` is an optional peer dependency, so
 * consumers that don't ship it still get correct web behaviour.
 *
 * `ErnoRealtimeService` and `ErnoSyncService` consume this to suspend the
 * WebSocket on background and reconnect + pull a delta on foreground resume.
 */
@Injectable()
export class ErnoAppStateService implements OnDestroy {
  private _state = new BehaviorSubject<ErnoAppState>('active');

  /** Current state, replayed to new subscribers. */
  readonly state$ = this._state.asObservable();

  /** Fires once on each background -> active transition (no replay). */
  readonly resumed$: Observable<void> = this.transition('background', 'active');

  /** Fires once on each active -> background transition (no replay). */
  readonly paused$: Observable<void> = this.transition('active', 'background');

  private capacitorHandle: PluginListenerHandle | null = null;
  private visibilityHandler: (() => void) | null = null;

  constructor(private zone: NgZone) {
    void this.init();
  }

  get state(): ErnoAppState {
    return this._state.value;
  }

  /**
   * Push a state change. Called by the platform listeners; also the seam tests
   * use to simulate background/foreground without Capacitor. No-ops when the
   * state is unchanged, deduping repeated platform events.
   */
  notifyStateChange(state: ErnoAppState): void {
    if (state === this._state.value) return;
    this._state.next(state);
  }

  ngOnDestroy(): void {
    void this.capacitorHandle?.remove();
    this.capacitorHandle = null;
    if (this.visibilityHandler && typeof document !== 'undefined') {
      document.removeEventListener('visibilitychange', this.visibilityHandler);
    }
    this.visibilityHandler = null;
    this._state.complete();
  }

  private async init(): Promise<void> {
    const capacitor = (globalThis as { Capacitor?: { isNativePlatform?: () => boolean } }).Capacitor;
    if (capacitor?.isNativePlatform?.()) {
      try {
        // Variable specifier + ignore comments keep bundlers from statically
        // resolving the optional `@capacitor/app` peer when it isn't installed.
        // `@vite-ignore` is for Vite; `webpackIgnore` silences webpack's
        // "Critical dependency: the request of a dependency is an expression".
        const specifier = '@capacitor/app';
        const mod = (await import(
          /* @vite-ignore */
          /* webpackIgnore: true */
          specifier
        )) as { App: CapacitorAppPlugin };
        this.capacitorHandle = await mod.App.addListener('appStateChange', ({ isActive }) =>
          this.zone.run(() => this.notifyStateChange(isActive ? 'active' : 'background')),
        );
        return;
      } catch {
        // Plugin missing or failed to load — fall back to the web listener.
      }
    }

    if (typeof document !== 'undefined') {
      this.visibilityHandler = () =>
        this.zone.run(() =>
          this.notifyStateChange(document.visibilityState === 'hidden' ? 'background' : 'active'),
        );
      document.addEventListener('visibilitychange', this.visibilityHandler);
    }
  }

  private transition(from: ErnoAppState, to: ErnoAppState): Observable<void> {
    return this._state.pipe(
      distinctUntilChanged(),
      pairwise(),
      filter(([prev, next]) => prev === from && next === to),
      map(() => undefined),
    );
  }
}
