import { TestBed } from '@angular/core/testing';
import { ErnoAppStateService } from './erno-app-state.service';

describe('ErnoAppStateService', () => {
  let service: ErnoAppStateService;

  beforeEach(() => {
    TestBed.configureTestingModule({ providers: [ErnoAppStateService] });
    service = TestBed.inject(ErnoAppStateService);
  });

  it('starts in the active state', () => {
    expect(service.state).toBe('active');
  });

  it('emits paused$ on active -> background', () => {
    const paused = jasmine.createSpy('paused');
    service.paused$.subscribe(paused);

    service.notifyStateChange('background');

    expect(paused).toHaveBeenCalledTimes(1);
    expect(service.state).toBe('background');
  });

  it('emits resumed$ only on background -> active, not on subscribe', () => {
    const resumed = jasmine.createSpy('resumed');
    service.resumed$.subscribe(resumed);

    expect(resumed).not.toHaveBeenCalled();

    service.notifyStateChange('background');
    expect(resumed).not.toHaveBeenCalled();

    service.notifyStateChange('active');
    expect(resumed).toHaveBeenCalledTimes(1);
  });

  it('dedupes repeated same-state notifications', () => {
    const paused = jasmine.createSpy('paused');
    service.paused$.subscribe(paused);

    service.notifyStateChange('background');
    service.notifyStateChange('background');

    expect(paused).toHaveBeenCalledTimes(1);
  });
});
