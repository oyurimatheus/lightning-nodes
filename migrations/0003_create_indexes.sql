


CREATE INDEX idx_lightning_nodes_covering
    ON lightning_nodes (snapshot_id, public_key)
    INCLUDE (alias, capacity_sats, first_seen);



CREATE TABLE active_snapshot (
    singleton    BOOLEAN PRIMARY KEY CHECK (singleton = TRUE),
    snapshot_id  UUID REFERENCES node_snapshots (id),
    activated_at TIMESTAMPTZ
);

INSERT INTO active_snapshot (singleton, snapshot_id, activated_at)
VALUES (TRUE, NULL, NULL);
