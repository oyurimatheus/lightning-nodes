-- Read-optimized covering index for GET /nodes: the read query filters on the
-- active snapshot, orders by public_key and only needs the four output columns,
-- so an index-only scan serves the whole request.
CREATE INDEX idx_lightning_nodes_covering
    ON lightning_nodes (snapshot_id, public_key)
    INCLUDE (alias, capacity_sats, first_seen);

-- Singleton pointer to the latest successfully published snapshot.
-- NULL snapshot_id is allowed briefly (fresh database, first import pending).
CREATE TABLE active_snapshot (
    singleton    BOOLEAN PRIMARY KEY CHECK (singleton = TRUE),
    snapshot_id  UUID REFERENCES node_snapshots (id),
    activated_at TIMESTAMPTZ
);

INSERT INTO active_snapshot (singleton, snapshot_id, activated_at)
VALUES (TRUE, NULL, NULL);
