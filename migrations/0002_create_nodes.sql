


CREATE TABLE lightning_nodes (
    snapshot_id   UUID NOT NULL,
    public_key    TEXT NOT NULL,
    alias         TEXT NOT NULL,
    capacity_sats BIGINT NOT NULL,
    first_seen    TIMESTAMPTZ NOT NULL,
    PRIMARY KEY (snapshot_id, public_key),
    CONSTRAINT lightning_nodes_snapshot_fk
        FOREIGN KEY (snapshot_id) REFERENCES node_snapshots (id) ON DELETE CASCADE,
    CONSTRAINT lightning_nodes_capacity_non_negative
        CHECK (capacity_sats >= 0)
);
