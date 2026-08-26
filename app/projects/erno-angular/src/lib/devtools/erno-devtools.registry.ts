import { Injectable, inject } from '@angular/core';
import Dexie from 'dexie';

/**
 * Devtools-facing Dexie instances. The overlay never opens arbitrary IndexedDB
 * names (wrong-version upgrades); apps opt in by registering the live instance.
 */
@Injectable()
export class ErnoDevtoolsRegistry {
  private readonly dbs = new Map<string, Dexie>();

  register(db: Dexie): void {
    const name = db.name || 'erno';
    this.dbs.set(name, db);
  }

  databases(): Dexie[] {
    return [...this.dbs.values()];
  }
}

/**
 * Attach an app-owned Dexie instance to the Data tab.
 * Must run in an injection context (constructor or `provideAppInitializer`).
 */
export function registerDevtoolsDatabase(db: Dexie): void {
  inject(ErnoDevtoolsRegistry).register(db);
}
