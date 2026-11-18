




CREATE TABLE node_snapshots (
    id            UUID PRIMARY KEY,
    status        TEXT NOT NULL,
    created_at    TIMESTAMPTZ NOT NULL,
    saved_at      TIMESTAMPTZ,
    node_count    INTEGER NOT NULL DEFAULT 0,
    error_message TEXT,
    CONSTRAINT node_snapshots_status_check
        CHECK (status IN ('loading', 'ready', 'failed')),
    CONSTRAINT node_snapshots_ready_requires_saved_at
        CHECK (status <> 'ready' OR saved_at IS NOT NULL),
    CONSTRAINT node_snapshots_ready_requires_error_free
        CHECK (status <> 'failed' OR error_message IS NOT NULL)
);
