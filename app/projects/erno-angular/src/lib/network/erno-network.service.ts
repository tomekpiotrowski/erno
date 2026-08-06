import { Injectable, NgZone, OnDestroy } from '@angular/core';
import { BehaviorSubject, Observable } from 'rxjs';
import { distinctUntilChanged, filter, map, pairwise } from 'rxjs/operators';

/** Minimal shape of the `@capacitor/network` plugin we rely on. */
interface CapacitorNetworkPlugin {
  getStatus(): Promise<{ connected: boolean }>;
  addListener(
    eventName: 'networkStatusChange',
    listener: (status: { connected: boolean }) => void,
  ): Promise<PluginListenerHandle> | PluginListenerHandle;
}

interface PluginListenerHandle {
  remove(): Promise<void> | void;
}

/**
 * Tracks whether the device has a network path to the internet.
 *
 * On Capacitor native platforms it wraps `@capacitor/network`; on the web (or
 * if the plugin is not installed) it falls back to `navigator.onLine` plus the
 * browser `online` / `offline` events. `@capacitor/network` is an optional peer
 * dependency, so consumers that don't ship it still get correct web behaviour.
 *
 * `ErnoRealtimeService` and `ErnoSyncService` consume this to suspend the
 * WebSocket and skip delta pulls while offline, then resume when connectivity
 * returns.
 */
@Injectable()
export class ErnoNetworkService implements OnDestroy {
  private _connected = new BehaviorSubject<boolean>(this.readNavigatorOnline());

  /** Current connectivity, replayed to new subscribers. */
  readonly connected$ = this._connected.asObservable();

  /** Fires once on each offline → online transition (no replay). */
  readonly online$: Observable<void> = this.transition(false, true);

  /** Fires once on each online → offline transition (no replay). */
  readonly offline$: Observable<void> = this.transition(true, false);

  private capacitorHandle: PluginListenerHandle | null = null;
  private onlineHandler: (() => void) | null = null;
  private offlineHandler: (() => void) | null = null;

  constructor(private zone: NgZone) {
    void this.init();
  }

  get connected(): boolean {
    return this._connected.value;
  }

  /**
   * Push a connectivity change. Called by the platform listeners; also the
   * seam tests use to simulate online/offline without Capacitor. No-ops when
   * the value is unchanged, deduping repeated platform events.
   */
  notifyStatusChange(connected: boolean): void {
    if (connected === this._connected.value) return;
    this._connected.next(connected);
  }

  ngOnDestroy(): void {
    void this.capacitorHandle?.remove();
    this.capacitorHandle = null;
    if (typeof window !== 'undefined') {
      if (this.onlineHandler) window.removeEventListener('online', this.onlineHandler);
      if (this.offlineHandler) window.removeEventListener('offline', this.offlineHandler);
    }
    this.onlineHandler = null;
    this.offlineHandler = null;
    this._connected.complete();
  }

  private async init(): Promise<void> {
    const capacitor = (globalThis as { Capacitor?: { isNativePlatform?: () => boolean } }).Capacitor;
    if (capacitor?.isNativePlatform?.()) {
      try {
        // Variable specifier + ignore comments keep bundlers from statically
        // resolving the optional `@capacitor/network` peer when it isn't installed.
        const specifier = '@capacitor/network';
        const mod = (await import(
          /* @vite-ignore */
          /* webpackIgnore: true */
          specifier
        )) as { Network: CapacitorNetworkPlugin };

        const status = await mod.Network.getStatus();
        this.zone.run(() => this.notifyStatusChange(!!status.connected));

        this.capacitorHandle = await mod.Network.addListener('networkStatusChange', ({ connected }) =>
          this.zone.run(() => this.notifyStatusChange(!!connected)),
        );
        return;
      } catch {
        // Plugin missing or failed to load — fall back to the web listeners.
      }
    }

    this.attachWebListeners();
  }

  private attachWebListeners(): void {
    if (typeof window === 'undefined') return;

    // Re-sync in case navigator.onLine changed before listeners were attached.
    this.notifyStatusChange(this.readNavigatorOnline());

    this.onlineHandler = () => this.zone.run(() => this.notifyStatusChange(true));
    this.offlineHandler = () => this.zone.run(() => this.notifyStatusChange(false));
    window.addEventListener('online', this.onlineHandler);
    window.addEventListener('offline', this.offlineHandler);
  }

  private readNavigatorOnline(): boolean {
    if (typeof navigator === 'undefined') return true;
    return navigator.onLine !== false;
  }

  private transition(from: boolean, to: boolean): Observable<void> {
    return this._connected.pipe(
      distinctUntilChanged(),
      pairwise(),
      filter(([prev, next]) => prev === from && next === to),
      map(() => undefined),
    );
  }
}
