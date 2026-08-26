import {
  ChangeDetectionStrategy,
  Component,
  OnInit,
  inject,
  output,
  signal,
} from '@angular/core';
import Dexie, { Table } from 'dexie';
import { ErnoDevtoolsRegistry } from '../erno-devtools.registry';
import { ERNO_DEVTOOLS_STYLES } from '../erno-devtools.styles';
import { downloadJson, toneColor } from '../erno-devtools.util';

const ROW_CAP = 50;

interface TableSnapshot {
  name: string;
  count: number;
  rows: unknown[];
}

interface DbSnapshot {
  name: string;
  tables: TableSnapshot[];
}

@Component({
  selector: 'erno-devtools-data-tab',
  imports: [],
  changeDetection: ChangeDetectionStrategy.OnPush,
  host: { style: 'display: contents' },
  styles: [ERNO_DEVTOOLS_STYLES],
  template: `
    @if (dbs().length === 0) {
      <div class="empty">
        <span class="empty-title">No local databases</span>
        <span class="empty-sub">provideErno registers erno. Apps call registerDevtoolsDatabase() for their own Dexie.</span>
      </div>
    }
    @for (db of dbs(); track db.name) {
      <div class="jkind">
        <div class="jrow" (click)="toggleDb(db.name)" role="button" tabindex="0" (keydown.enter)="toggleDb(db.name)">
          <span class="caret" [class.open]="openDb() === db.name">▸</span>
          <span class="jname"><span>{{ db.name }}</span></span>
          <span class="jstat" [style.color]="toneColor('muted')">{{ db.tables.length }} tables</span>
          <span class="jtime"></span>
        </div>
        @if (openDb() === db.name) {
          <div class="jexp">
            <div class="err-acts" style="padding-bottom: 8px;">
              <button type="button" class="ghost sm" (click)="wipeDb(db.name)">wipe db</button>
            </div>
            @for (table of db.tables; track table.name) {
              <div class="jrow" (click)="toggleTable(db.name, table.name)" role="button" tabindex="0"
                (keydown.enter)="toggleTable(db.name, table.name)">
                <span class="caret" [class.open]="isOpenTable(db.name, table.name)">▸</span>
                <span class="jname"><span>{{ table.name }}</span></span>
                <span class="jstat" [style.color]="toneColor(table.count ? 'ok' : 'muted')">{{ table.count }}</span>
              </div>
              @if (isOpenTable(db.name, table.name)) {
                <div class="jexp">
                  @if (table.name === 'pendingMutations' && table.count === 0) {
                    <span class="sdetail" style="padding: 0 0 8px;">
                      offline write queue — empty until the client write path lands
                    </span>
                  }
                  <div class="jbar" style="padding-left: 0;">
                    <input
                      class="filter"
                      placeholder="filter rows"
                      [value]="query()"
                      (input)="query.set($any($event.target).value)"
                    />
                    <button type="button" class="ghost sm" (click)="exportTable(db.name, table); $event.stopPropagation()">
                      export
                    </button>
                    <button type="button" class="ghost sm mute" (click)="wipeTable(db.name, table.name); $event.stopPropagation()">
                      wipe
                    </button>
                  </div>
                  @for (row of visibleRows(table); track $index) {
                    <div class="erow" (click)="copyRow(row)">
                      <span class="sdetail" style="font-family: var(--dt-mono); white-space: pre-wrap; word-break: break-word;">{{ formatRow(row) }}</span>
                    </div>
                  }
                  @if (visibleRows(table).length === 0) {
                    <div class="empty" style="padding: 12px 0;">
                      <span class="empty-title">{{ table.count === 0 ? 'Empty table' : 'Nothing matches' }}</span>
                    </div>
                  }
                </div>
              }
            }
          </div>
        }
      </div>
    }
  `,
})
export class ErnoDevtoolsDataTab implements OnInit {
  private readonly registry = inject(ErnoDevtoolsRegistry);

  readonly note = output<string>();

  readonly dbs = signal<DbSnapshot[]>([]);
  readonly openDb = signal<string | null>(null);
  readonly openTable = signal<string | null>(null);
  readonly query = signal('');

  readonly toneColor = toneColor;

  ngOnInit(): void {
    void this.reload();
  }

  isOpenTable(db: string, table: string): boolean {
    return this.openDb() === db && this.openTable() === table;
  }

  toggleDb(name: string): void {
    this.openDb.update(current => (current === name ? null : name));
    this.openTable.set(null);
  }

  toggleTable(db: string, table: string): void {
    this.openDb.set(db);
    this.openTable.update(current => (current === table ? null : table));
  }

  visibleRows(table: TableSnapshot): unknown[] {
    const q = this.query().trim().toLowerCase();
    const rows = table.rows;
    if (!q) return rows;
    return rows.filter(row => JSON.stringify(row).toLowerCase().includes(q));
  }

  formatRow(row: unknown): string {
    try {
      return JSON.stringify(row);
    } catch {
      return String(row);
    }
  }

  async wipeTable(dbName: string, tableName: string): Promise<void> {
    const table = this.table(dbName, tableName);
    if (!table) return;
    await table.clear();
    this.note.emit(`wiped ${dbName}.${tableName}`);
    await this.reload();
  }

  async wipeDb(dbName: string): Promise<void> {
    const db = this.db(dbName);
    if (!db) return;
    // Clear tables rather than Dexie.delete() so the live instance (and its
    // schema) stays valid for the rest of the session.
    await Promise.all((db.tables ?? []).map((table: Table) => table.clear()));
    this.note.emit(`wiped ${dbName}`);
    await this.reload();
  }

  async exportTable(dbName: string, table: TableSnapshot): Promise<void> {
    const raw = this.table(dbName, table.name);
    const rows = raw ? await raw.toArray() : table.rows;
    downloadJson(`${dbName}-${table.name}.json`, rows);
    this.note.emit(`exported ${table.name}`);
  }

  async copyRow(row: unknown): Promise<void> {
    try {
      await navigator.clipboard.writeText(JSON.stringify(row, null, 2));
      this.note.emit('copied row');
    } catch {
      this.note.emit('copy failed');
    }
  }

  private db(name: string): Dexie | undefined {
    return this.registry.databases().find(d => (d.name || 'erno') === name);
  }

  private table(dbName: string, tableName: string): Table | undefined {
    return this.db(dbName)?.tables?.find(t => t.name === tableName);
  }

  private async reload(): Promise<void> {
    const snapshots: DbSnapshot[] = [];
    for (const db of this.registry.databases()) {
      const tables: TableSnapshot[] = [];
      for (const table of db.tables ?? []) {
        let count = 0;
        let rows: unknown[] = [];
        try {
          count = await table.count();
          rows = await table.limit(ROW_CAP).toArray();
        } catch {
          count = 0;
          rows = [];
        }
        tables.push({ name: table.name, count, rows });
      }
      snapshots.push({ name: db.name || 'erno', tables });
    }
    this.dbs.set(snapshots);
    if (this.openDb() == null && snapshots.length === 1) {
      this.openDb.set(snapshots[0].name);
    }
  }
}
