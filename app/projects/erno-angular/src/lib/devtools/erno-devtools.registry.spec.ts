import { TestBed } from '@angular/core/testing';
import Dexie from 'dexie';
import { ErnoDevtoolsRegistry, registerDevtoolsDatabase } from './erno-devtools.registry';

class StubDb {
  constructor(
    public name: string,
    public tables: unknown[] = [],
  ) {}
}

describe('ErnoDevtoolsRegistry', () => {
  let registry: ErnoDevtoolsRegistry;

  beforeEach(() => {
    TestBed.configureTestingModule({ providers: [ErnoDevtoolsRegistry] });
    registry = TestBed.inject(ErnoDevtoolsRegistry);
  });

  it('starts empty and records registered Dexie instances', () => {
    expect(registry.databases()).toEqual([]);
    const erno = new StubDb('erno') as unknown as Dexie;
    registry.register(erno);
    expect(registry.databases()).toEqual([erno]);
  });

  it('adds a second database through registerDevtoolsDatabase', () => {
    const erno = new StubDb('erno') as unknown as Dexie;
    const todos = new StubDb('todos') as unknown as Dexie;
    registry.register(erno);
    TestBed.runInInjectionContext(() => registerDevtoolsDatabase(todos));
    expect(registry.databases().map(db => db.name)).toEqual(['erno', 'todos']);
  });

  it('replaces a database registered under the same name', () => {
    const first = new StubDb('erno') as unknown as Dexie;
    const second = new StubDb('erno') as unknown as Dexie;
    registry.register(first);
    registry.register(second);
    expect(registry.databases()).toEqual([second]);
  });
});
