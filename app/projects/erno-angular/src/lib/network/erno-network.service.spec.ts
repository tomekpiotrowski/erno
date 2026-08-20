import { TestBed } from '@angular/core/testing';
import { ErnoNetworkService } from './erno-network.service';

describe('ErnoNetworkService', () => {
  let service: ErnoNetworkService;

  beforeEach(() => {
    TestBed.configureTestingModule({ providers: [ErnoNetworkService] });
    service = TestBed.inject(ErnoNetworkService);
  });

  it('starts from navigator.onLine (defaults to online in tests)', () => {
    expect(service.connected).toBe(true);
  });

  it('emits offline$ on online -> offline', () => {
    const offline = vi.fn().mockName('offline');
    service.offline$.subscribe(offline);

    service.notifyStatusChange(false);

    expect(offline).toHaveBeenCalledTimes(1);
    expect(service.connected).toBe(false);
  });

  it('emits online$ only on offline -> online, not on subscribe', () => {
    const online = vi.fn().mockName('online');
    service.online$.subscribe(online);

    expect(online).not.toHaveBeenCalled();

    service.notifyStatusChange(false);
    expect(online).not.toHaveBeenCalled();

    service.notifyStatusChange(true);
    expect(online).toHaveBeenCalledTimes(1);
  });

  it('dedupes repeated same-status notifications', () => {
    const offline = vi.fn().mockName('offline');
    service.offline$.subscribe(offline);

    service.notifyStatusChange(false);
    service.notifyStatusChange(false);

    expect(offline).toHaveBeenCalledTimes(1);
  });

  it('replays the current value on connected$', () => {
    service.notifyStatusChange(false);
    const values: boolean[] = [];
    service.connected$.subscribe((v) => values.push(v));
    expect(values).toEqual([false]);
  });
});
