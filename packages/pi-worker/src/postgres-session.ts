import { randomUUID } from "node:crypto";
import { readFile } from "node:fs/promises";
import type { Pool, PoolClient } from "pg";
import {
  StorageBackedSession,
  prepareStorageCommit,
  validateCommittedWrites,
  createForkSnapshot,
  projectForkCurrentStateWrite,
  branchTip,
  value,
  resolveListReadOptions,
  type Context,
  type Storage,
  type Write,
  type CommitResult,
  type Entry,
  type EntryScan,
  type StorageBranchScan,
  type UsageRow,
  type UsageScan,
  type SessionStats,
  type Value,
  type ValueList,
  type StoredValue,
  type ListReadOptions,
  type ListElement,
  type Session,
  type SessionMetadata,
  type SessionRepo,
  type SessionCreateOptions,
  type ForkOptions,
} from "@earendil-works/pi-agent-core";
import type { Usage } from "@earendil-works/pi-ai";

const family = "pi-0.85.1";
const storageVersion = 1;
export interface PgOwnership {
  tenantId: string;
  jobId: string;
  ownerId: string;
  epoch: number;
}
export async function migratePiPostgres(pool: Pool) {
  const connection = await pool.connect();
  try {
    await connection.query("BEGIN");
    // One migration lock for this backend; it does not participate in job ownership.
    await connection.query("SELECT pg_advisory_xact_lock(7823491)");
    await connection.query(
      await readFile(new URL("./postgres-schema.sql", import.meta.url), "utf8"),
    );
    await connection.query("COMMIT");
  } catch (error) {
    await connection.query("ROLLBACK");
    throw error;
  } finally {
    connection.release();
  }
}
async function fenced<T>(
  pool: Pool,
  owner: PgOwnership,
  ids: string[],
  context: Context,
  body: (db: PoolClient) => Promise<T>,
): Promise<T> {
  context.abortSignal?.throwIfAborted();
  const db = await pool.connect();
  try {
    await db.query("BEGIN");
    await db.query("SET LOCAL statement_timeout = '15s'");
    for (const id of [...new Set(ids)].sort()) {
      const row = await db.query(
        "SELECT epoch FROM session_leases WHERE tenant_id=$1 AND session_id=$2 AND job_id=$3 AND owner_id=$4 AND epoch=$5 AND expires_at > EXTRACT(EPOCH FROM clock_timestamp())*1000 FOR UPDATE",
        [owner.tenantId, id, owner.jobId, owner.ownerId, owner.epoch],
      );
      if (!row.rowCount)
        throw new Error("Pi session ownership is expired or superseded");
      const deleted = await db.query("SELECT session_id FROM deleted_sessions WHERE tenant_id=$1 AND session_id=$2", [owner.tenantId, id]);
      if (deleted.rowCount) throw new Error("Session was deleted; a new clean session is required");
    }
    const result = await body(db);
    context.abortSignal?.throwIfAborted();
    // A long transaction must not commit after its lease expired while holding the row.
    for (const id of ids) {
      const row = await db.query(
        "SELECT 1 FROM session_leases WHERE tenant_id=$1 AND session_id=$2 AND expires_at > EXTRACT(EPOCH FROM clock_timestamp())*1000",
        [owner.tenantId, id],
      );
      if (!row.rowCount)
        throw new Error("Pi session ownership expired during the operation");
    }
    await db.query("COMMIT");
    return result;
  } catch (error) {
    await db.query("ROLLBACK");
    throw error;
  } finally {
    db.release();
  }
}
function emptyStats(): SessionStats {
  return {
    messageCount: 0,
    usage: {
      input: 0,
      output: 0,
      cacheRead: 0,
      cacheWrite: 0,
      totalTokens: 0,
      cost: { input: 0, output: 0, cacheRead: 0, cacheWrite: 0, total: 0 },
    },
  };
}
function addUsage(stats: SessionStats, usage: Usage) {
  for (const key of [
    "input",
    "output",
    "cacheRead",
    "cacheWrite",
    "totalTokens",
    "cacheWrite1h",
    "reasoning",
  ] as const) {
    if (usage[key] !== undefined)
      stats.usage[key] = (stats.usage[key] ?? 0) + usage[key];
  }
  for (const key of [
    "input",
    "output",
    "cacheRead",
    "cacheWrite",
    "total",
  ] as const)
    stats.usage.cost[key] += usage.cost[key];
}

export class PgStorage implements Storage {
  private closed = false;
  private writes: Promise<unknown> = Promise.resolve();
  constructor(
    private readonly pool: Pool,
    readonly owner: PgOwnership,
    readonly sessionId: string,
  ) {}
  private access<T>(
    context: Context,
    body: (db: PoolClient, keys: string[]) => Promise<T>,
  ) {
    if (this.closed) return Promise.reject(new Error("PgStorage is closed"));
    return fenced(this.pool, this.owner, [this.sessionId], context, (db) =>
      body(db, [this.owner.tenantId, this.sessionId]),
    );
  }
  commit(writes: Write[], context: Context): Promise<CommitResult> {
    if (this.closed) return Promise.reject(new Error("PgStorage is closed"));
    const operation = this.writes.then(() =>
      fenced(this.pool, this.owner, [this.sessionId], context, async (db) => {
        const keys = [this.owner.tenantId, this.sessionId];
        const row = (
          await db.query(
            "SELECT next_seq,stats,EXTRACT(EPOCH FROM clock_timestamp())*1000 AS now FROM pi_sessions.sessions WHERE tenant_id=$1 AND id=$2 FOR UPDATE",
            keys,
          )
        ).rows[0];
        if (!row) throw new Error("Unknown Pi session");
        const prepared = prepareStorageCommit(
          writes,
          Number(row.next_seq),
          Math.floor(Number(row.now)),
        );
        const ids = prepared.writes.flatMap((w) =>
          w.kind === "entry"
            ? [w.id, ...(w.parentId ? [w.parentId] : [])]
            : w.kind === "usage"
              ? [w.id]
              : [],
        );
        const existing = (
          await db.query(
            "SELECT id,kind FROM pi_sessions.items WHERE tenant_id=$1 AND session_id=$2 AND id=ANY($3::text[])",
            [...keys, ids],
          )
        ).rows;
        const allIds = new Set(existing.map((r) => r.id));
        const entries = new Set(
          existing.filter((r) => r.kind === "entry").map((r) => r.id),
        );
        validateCommittedWrites(prepared.writes, prepared.result.firstSeq, {
          hasEntryOrUsageId: (id) => allIds.has(id),
          hasEntryId: (id) => entries.has(id),
        });
        const stats: SessionStats = row.stats;
        for (const write of prepared.writes) {
          if (write.kind === "entry" || write.kind === "usage") {
            const { kind, ...data } = write;
            await db.query(
              "INSERT INTO pi_sessions.items(tenant_id,session_id,id,seq,kind,parent_id,type,custom_type,data) VALUES($1,$2,$3,$4,$5,$6,$7,$8,$9)",
              [
                ...keys,
                write.id,
                write.seq,
                kind,
                write.kind === "entry" ? write.parentId : null,
                write.kind === "entry" ? write.type : null,
                write.kind === "entry" && write.type === "custom"
                  ? write.customType
                  : null,
                JSON.stringify(data),
              ],
            );
            if (write.kind === "usage") addUsage(stats, write.usage);
            if (write.kind === "entry" && write.type === "message")
              stats.messageCount++;
          } else {
            const table =
              write.kind === "value"
                ? "pi_sessions.values"
                : "pi_sessions.lists";
            if (write.op === "delete")
              await db.query(
                `DELETE FROM ${table} WHERE tenant_id=$1 AND session_id=$2 AND namespace=$3 AND key=$4`,
                [...keys, write.namespace, write.key],
              );
            else
              await db.query(
                `INSERT INTO ${table}(tenant_id,session_id,namespace,key,seq,data) VALUES($1,$2,$3,$4,$5,$6) ${write.kind === "value" ? "ON CONFLICT(tenant_id,session_id,namespace,key) DO UPDATE SET seq=excluded.seq,data=excluded.data" : ""}`,
                [
                  ...keys,
                  write.namespace,
                  write.key,
                  write.seq,
                  JSON.stringify(write.value),
                ],
              );
          }
        }
        await db.query(
          "UPDATE pi_sessions.sessions SET next_seq=$3,stats=$4 WHERE tenant_id=$1 AND id=$2",
          [
            ...keys,
            prepared.result.firstSeq + prepared.writes.length,
            JSON.stringify(stats),
          ],
        );
        return { ...prepared.result, stats };
      }),
    );
    this.writes = operation.catch(() => {});
    return operation;
  }
  getEntries(ids: string[], context: Context): Promise<Map<string, Entry>> {
    return this.access(
      context,
      async (db, keys) =>
        new Map(
          (
            await db.query(
              "SELECT id,data FROM pi_sessions.items WHERE tenant_id=$1 AND session_id=$2 AND kind='entry' AND id=ANY($3::text[])",
              [...keys, ids],
            )
          ).rows.map((r) => [r.id, r.data]),
        ),
    );
  }
  getValue<T>(
    address: Value<T>,
    context: Context,
  ): Promise<StoredValue<T> | undefined> {
    return this.access(context, async (db, keys) => {
      const row = (
        await db.query(
          "SELECT seq,data FROM pi_sessions.values WHERE tenant_id=$1 AND session_id=$2 AND namespace=$3 AND key=$4",
          [...keys, address.namespace, address.key],
        )
      ).rows[0];
      return row
        ? { address, seq: Number(row.seq), value: row.data }
        : undefined;
    });
  }
  scanValues<T>(prefix: Value<T>, context: Context): Promise<StoredValue<T>[]> {
    return this.access(context, async (db, keys) =>
      (
        await db.query(
          'SELECT key,seq,data FROM pi_sessions.values WHERE tenant_id=$1 AND session_id=$2 AND namespace=$3 AND starts_with(key,$4) ORDER BY key COLLATE "C"',
          [...keys, prefix.namespace, prefix.key],
        )
      ).rows.map((r) => ({
        address: value<T>(prefix.namespace, r.key),
        seq: Number(r.seq),
        value: r.data,
      })),
    );
  }
  async readList<T>(
    address: ValueList<T>,
    options: ListReadOptions | undefined,
    context: Context,
  ): Promise<ListElement<T>[]> {
    const selected = resolveListReadOptions(options);
    return this.access(context, async (db, keys) =>
      (
        await db.query(
          `SELECT seq,data FROM pi_sessions.lists WHERE tenant_id=$1 AND session_id=$2 AND namespace=$3 AND key=$4 AND ($5::bigint IS NULL OR seq ${selected.order === "asc" ? ">" : "<"} $5) ORDER BY seq ${selected.order === "asc" ? "ASC" : "DESC"} LIMIT $6`,
          [
            ...keys,
            address.namespace,
            address.key,
            selected.cursor?.seq ?? null,
            selected.limit,
          ],
        )
      ).rows.map((r) => ({ seq: Number(r.seq), value: r.data })),
    );
  }
  scanBranch(query: StorageBranchScan, context: Context): Promise<Entry[]> {
    return this.access(context, async (db, keys) => {
      const asc = query.order === "oldestFirst";
      const start = await db.query(
        "SELECT 1 FROM pi_sessions.items WHERE tenant_id=$1 AND session_id=$2 AND id=$3 AND kind='entry'",
        [...keys, query.start],
      );
      if (!start.rowCount) throw new Error("Unknown branch start");
      const rows = await db.query(
        `WITH RECURSIVE ancestry AS (
        SELECT id,parent_id,seq,type,custom_type FROM pi_sessions.items WHERE tenant_id=$1 AND session_id=$2 AND id=$3 AND kind='entry'
        UNION ALL SELECT e.id,e.parent_id,e.seq,e.type,e.custom_type FROM pi_sessions.items e JOIN ancestry a ON e.id=a.parent_id WHERE e.tenant_id=$1 AND e.session_id=$2 AND e.kind='entry'
      ), boundary AS (SELECT ${asc ? "MIN" : "MAX"}(seq) AS seq FROM ancestry WHERE id=$4 OR type=$5)
      SELECT e.data FROM ancestry a JOIN pi_sessions.items e ON e.tenant_id=$1 AND e.session_id=$2 AND e.id=a.id CROSS JOIN boundary b
      WHERE (b.seq IS NULL OR a.seq ${asc ? "<=" : ">="} b.seq) AND ($6::text IS NULL OR a.type=$6) AND ($7::text IS NULL OR a.custom_type=$7) AND ($8::bigint IS NULL OR a.seq ${asc ? ">" : "<"} $8)
      ORDER BY a.seq ${asc ? "ASC" : "DESC"} LIMIT $9`,
        [
          ...keys,
          query.start,
          query.stopAtId ?? null,
          query.stopAtType ?? null,
          query.type ?? null,
          query.customType ?? null,
          query.cursor?.seq ?? null,
          query.limit === undefined
            ? null
            : Math.max(0, Math.trunc(query.limit)),
        ],
      );
      return rows.rows.map((r) => r.data);
    });
  }
  async scanBranchStructure(query: StorageBranchScan, context: Context) {
    return (await this.scanBranch(query, context)).map((e) => ({
      id: e.id,
      parentId: e.parentId,
      seq: e.seq,
      timestamp: e.timestamp,
      type: e.type,
      ...(e.type === "custom" ? { customType: e.customType } : {}),
    }));
  }
  private scan<T>(
    kind: string,
    query: EntryScan,
    context: Context,
  ): Promise<T[]> {
    return this.access(context, async (db, keys) =>
      (
        await db.query(
          `SELECT data FROM pi_sessions.items WHERE tenant_id=$1 AND session_id=$2 AND kind=$3 AND ($4::bigint IS NULL OR seq >= $4) AND ($5::bigint IS NULL OR seq <= $5) AND ($6::text IS NULL OR type=$6) AND ($7::text IS NULL OR custom_type=$7) ORDER BY seq ${query.order === "desc" ? "DESC" : "ASC"} LIMIT $8`,
          [
            ...keys,
            kind,
            query.fromSeq ?? null,
            query.toSeq ?? null,
            query.type ?? null,
            query.customType ?? null,
            query.limit === undefined
              ? null
              : Math.max(0, Math.trunc(query.limit)),
          ],
        )
      ).rows.map((r) => r.data),
    );
  }
  scanEntries(query: EntryScan, context: Context) {
    return this.scan<Entry>("entry", query, context);
  }
  scanUsage(query: UsageScan, context: Context) {
    return this.scan<UsageRow>("usage", query, context);
  }
  getStats(context: Context): Promise<SessionStats> {
    return this.access(context, async (db, keys) => {
      const row = (
        await db.query(
          "SELECT stats FROM pi_sessions.sessions WHERE tenant_id=$1 AND id=$2",
          keys,
        )
      ).rows[0];
      if (!row) throw new Error("Unknown Pi session");
      return row.stats;
    });
  }
  async close(_context: Context) {
    this.closed = true;
    await this.writes;
  }
}

export class PgSessionRepo implements SessionRepo {
  private readonly opened = new Map<string, Session>();
  private readonly opening = new Set<string>();
  private closed = false;
  constructor(
    private readonly pool: Pool,
    readonly owner: PgOwnership,
  ) {}
  private available(id?: string) {
    if (this.closed) throw new Error("PgSessionRepo is closed");
    if (id && (this.opened.has(id) || this.opening.has(id)))
      throw new Error("Session is already open");
  }
  private attach(metadata: SessionMetadata) {
    const session = new StorageBackedSession(
      metadata,
      new PgStorage(this.pool, this.owner, metadata.id),
      { onClose: () => this.opened.delete(metadata.id) },
    );
    this.opened.set(metadata.id, session);
    return session;
  }
  async create(options: SessionCreateOptions, context: Context) {
    const id = options.id ?? randomUUID();
    this.available(id);
    this.opening.add(id);
    try {
      const metadata: SessionMetadata = {
        id,
        createdAt: Date.now(),
        storageVersion,
        ...(options.parentSessionId
          ? { parentSessionId: options.parentSessionId }
          : {}),
      };
      await fenced(this.pool, this.owner, [id], context, (db) =>
        db.query(
          "INSERT INTO pi_sessions.sessions(tenant_id,id,metadata,worker_family,stats) VALUES($1,$2,$3,$4,$5)",
          [
            this.owner.tenantId,
            id,
            JSON.stringify(metadata),
            family,
            JSON.stringify(emptyStats()),
          ],
        ),
      );
      return this.attach(metadata);
    } finally {
      this.opening.delete(id);
    }
  }
  async open(metadata: SessionMetadata, context: Context) {
    this.available(metadata.id);
    this.opening.add(metadata.id);
    try {
      const stored = await fenced(
        this.pool,
        this.owner,
        [metadata.id],
        context,
        async (db) =>
          (
            await db.query(
              "SELECT metadata,worker_family FROM pi_sessions.sessions WHERE tenant_id=$1 AND id=$2",
              [this.owner.tenantId, metadata.id],
            )
          ).rows[0],
      );
      if (!stored) throw new Error("Unknown Pi session");
      if (
        stored.worker_family !== family ||
        stored.metadata.storageVersion !== storageVersion
      )
        throw new Error("Pi session requires an offline format migration");
      return this.attach(stored.metadata);
    } finally {
      this.opening.delete(metadata.id);
    }
  }
  async list(
    _options: undefined,
    context: Context,
  ): Promise<SessionMetadata[]> {
    this.available();
    context.abortSignal?.throwIfAborted();
    return (
      await this.pool.query(
        "SELECT s.metadata FROM pi_sessions.sessions s JOIN session_leases l ON l.tenant_id=s.tenant_id AND l.session_id=s.id WHERE s.tenant_id=$1 AND l.job_id=$2 AND l.owner_id=$3 AND l.epoch=$4 AND l.expires_at > EXTRACT(EPOCH FROM clock_timestamp())*1000 AND NOT EXISTS (SELECT 1 FROM deleted_sessions d WHERE d.tenant_id=s.tenant_id AND d.session_id=s.id) ORDER BY s.metadata->>'createdAt',s.id",
        [
          this.owner.tenantId,
          this.owner.jobId,
          this.owner.ownerId,
          this.owner.epoch,
        ],
      )
    ).rows.map((r) => r.metadata);
  }
  async delete(metadata: SessionMetadata, context: Context) {
    this.available(metadata.id);
    this.opening.add(metadata.id);
    try {
      await fenced(
        this.pool,
        this.owner,
        [metadata.id],
        context,
        async (db) => {
          const result = await db.query(
            "DELETE FROM pi_sessions.sessions WHERE tenant_id=$1 AND id=$2",
            [this.owner.tenantId, metadata.id],
          );
          if (!result.rowCount) throw new Error("Unknown Pi session");
        },
      );
    } finally {
      this.opening.delete(metadata.id);
    }
  }
  async fork(source: SessionMetadata, options: ForkOptions, context: Context) {
    const id = options.id ?? randomUUID();
    this.available(id);
    this.opening.add(id);
    try {
      const metadata: SessionMetadata = {
        id,
        createdAt: Date.now(),
        storageVersion,
        parentSessionId: source.id,
      };
      await fenced(
        this.pool,
        this.owner,
        [source.id, id],
        context,
        async (db) => {
          const keys = [this.owner.tenantId, source.id];
          const original = (
            await db.query(
              "SELECT metadata,worker_family,next_seq FROM pi_sessions.sessions WHERE tenant_id=$1 AND id=$2 FOR UPDATE",
              keys,
            )
          ).rows[0];
          if (
            !original ||
            original.worker_family !== family ||
            original.metadata.storageVersion !== storageVersion
          )
            throw new Error("Unknown or incompatible fork source");
          const entries: Entry[] = (
            await db.query(
              "SELECT data FROM pi_sessions.items WHERE tenant_id=$1 AND session_id=$2 AND kind='entry' ORDER BY seq",
              keys,
            )
          ).rows.map((r) => r.data);
          const scalarValues: StoredValue<unknown>[] = (
            await db.query(
              "SELECT namespace,key,seq,data FROM pi_sessions.values WHERE tenant_id=$1 AND session_id=$2 ORDER BY seq",
              keys,
            )
          ).rows.map((r) => ({
            address: value(r.namespace, r.key),
            seq: Number(r.seq),
            value: r.data,
          }));
          // Only branch tips establish lanes. Unattached configuration rows are
          // unrelated to the tree being copied (the upstream streaming contract).
          const branches = new Set(
            scalarValues
              .filter((v) => v.address.namespace === "pi.branch.tip")
              .map((v) => v.address.key),
          );
          const forkValues = scalarValues.filter(
            (v) =>
              !["pi.lane.config", "pi.lane.state"].includes(
                v.address.namespace,
              ) || branches.has(v.address.key),
          );
          const snapshot = createForkSnapshot(
            { entries, scalarValues: forkValues },
            options,
          );
          const stats = emptyStats();
          stats.messageCount = [...snapshot.entries.values()].filter(
            (e) => e.type === "message",
          ).length;
          await db.query(
            "INSERT INTO pi_sessions.sessions(tenant_id,id,metadata,worker_family,next_seq,stats) VALUES($1,$2,$3,$4,$5,$6)",
            [
              this.owner.tenantId,
              id,
              JSON.stringify(metadata),
              family,
              snapshot.nextSeq,
              JSON.stringify(stats),
            ],
          );
          for (const entry of snapshot.entries.values())
            await db.query(
              "INSERT INTO pi_sessions.items(tenant_id,session_id,id,seq,kind,parent_id,type,custom_type,data) VALUES($1,$2,$3,$4,'entry',$5,$6,$7,$8)",
              [
                this.owner.tenantId,
                id,
                entry.id,
                entry.seq,
                entry.parentId,
                entry.type,
                entry.type === "custom" ? entry.customType : null,
                JSON.stringify(entry),
              ],
            );
          let next = Math.max(snapshot.nextSeq, Number(original.next_seq));
          for (const stored of snapshot.scalarValues)
            await db.query(
              "INSERT INTO pi_sessions.values(tenant_id,session_id,namespace,key,seq,data) VALUES($1,$2,$3,$4,$5,$6)",
              [
                this.owner.tenantId,
                id,
                stored.address.namespace,
                stored.address.key,
                next++,
                JSON.stringify(stored.value),
              ],
            );
          const lists = (
            await db.query(
              "SELECT namespace,key,seq,data FROM pi_sessions.lists WHERE tenant_id=$1 AND session_id=$2 ORDER BY seq",
              keys,
            )
          ).rows;
          const tip =
            options.scope === "branch"
              ? (snapshot.scalarValues.find(
                  (v) =>
                    v.address.namespace === branchTip("").namespace &&
                    v.address.key === options.branch,
                )?.value as string | null)
              : null;
          for (const row of lists) {
            const projected = projectForkCurrentStateWrite(
              {
                kind: "list",
                op: "append",
                namespace: row.namespace,
                key: row.key,
                seq: Number(row.seq),
                value: row.data,
              },
              options.scope === "tree"
                ? { scope: "tree" }
                : {
                    scope: "branch",
                    branch: options.branch,
                    destinationTip: tip,
                  },
              (entryId) => snapshot.entries.has(entryId),
            );
            if (projected)
              await db.query(
                "INSERT INTO pi_sessions.lists(tenant_id,session_id,namespace,key,seq,data) VALUES($1,$2,$3,$4,$5,$6)",
                [
                  this.owner.tenantId,
                  id,
                  row.namespace,
                  row.key,
                  Number(row.seq),
                  JSON.stringify(projected.value),
                ],
              );
          }
          await db.query(
            "UPDATE pi_sessions.sessions SET next_seq=$3 WHERE tenant_id=$1 AND id=$2",
            [this.owner.tenantId, id, next],
          );
        },
      );
      return this.attach(metadata);
    } finally {
      this.opening.delete(id);
    }
  }
  async close(context: Context) {
    this.closed = true;
    await Promise.all([...this.opened.values()].map((s) => s.close(context)));
  }
}

/** Offline administration after the Host has committed its revocation barrier. */
export async function purgePostgresSession(pool: Pool, tenantId: string, sessionId: string) {
  const db = await pool.connect();
  try {
    await db.query("BEGIN");
    // Serialize with all writers, then require a Host deletion registration.
    await db.query("SELECT epoch FROM session_leases WHERE tenant_id=$1 AND session_id=$2 FOR UPDATE", [tenantId, sessionId]);
    const deleted = await db.query("SELECT session_id FROM deleted_sessions WHERE tenant_id=$1 AND session_id=$2", [tenantId, sessionId]);
    if (!deleted.rowCount) throw new Error("Host revocation must precede Pi deletion");
    await db.query("DELETE FROM pi_sessions.sessions WHERE tenant_id=$1 AND id=$2", [tenantId, sessionId]);
    await db.query("COMMIT");
  } catch (error) {
    await db.query("ROLLBACK");
    throw error;
  } finally { db.release(); }
}
