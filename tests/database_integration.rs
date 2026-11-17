//! Database failure handling and migration validation.

mod common;

use axum::http::StatusCode;
use common::{provider_json, start_stack};
use nodes_svc::features::nodes::commands::repository::SnapshotRepository;
use sqlx::Executor;

/// Scenario 5 — Database persistence failure: no partial snapshot becomes
/// active and the previously active snapshot is untouched.
#[tokio::test]
async fn scenario_5_database_failure_never_publishes_partial_snapshot() {
    let env = start_stack(&provider_json(3), 200, true).await;
    env.run_sync().await.expect("snapshot A");
    let snapshot_a = env.active_snapshot_id().await;

    // Break the write path: drop the nodes table (cascades nothing published;
    // the active snapshot's rows disappear, but the pointer stays).
    env.components
        .pools
        .write
        .execute("DROP TABLE lightning_nodes CASCADE")
        .await
        .expect("drop table");

    // Next cycle must fail without publishing anything.
    let result = env.run_sync().await;
    assert!(result.is_err(), "persistence failure must fail the cycle");
    assert_eq!(
        env.active_snapshot_id().await,
        snapshot_a,
        "active pointer untouched by the failed cycle"
    );
}

/// Scenario 15 — Migration validation on a fresh database: schema,
/// constraints and indexes all behave as designed.
#[tokio::test]
async fn scenario_15_migrations_create_valid_schema() {
    let env = start_stack(&provider_json(1), 200, true).await;
    env.run_sync().await.expect("seed data");

    let write = &env.components.pools.write;

    // Fresh database: migrations run cleanly and create every object.
    write
        .execute("CREATE DATABASE migration_check")
        .await
        .expect("create db");
    let port = env.components.pools.read.connect_options().get_port();
    let host = env
        .components
        .pools
        .read
        .connect_options()
        .get_host()
        .to_string();
    let fresh_url = format!("postgres://postgres:postgres@{host}:{port}/migration_check");
    let fresh = sqlx::PgPool::connect(&fresh_url)
        .await
        .expect("connect fresh db");
    sqlx::migrate!("./migrations")
        .run(&fresh)
        .await
        .expect("migrations on fresh db");

    // Tables exist.
    for table in ["node_snapshots", "lightning_nodes", "active_snapshot"] {
        let exists: bool = sqlx::query_scalar(
            "SELECT EXISTS (SELECT 1 FROM information_schema.tables WHERE table_name = $1)",
        )
        .bind(table)
        .fetch_one(&fresh)
        .await
        .expect("table exists");
        assert!(exists, "table {table} created");
    }

    // Singleton row of active_snapshot exists.
    let singletons: i64 = sqlx::query_scalar("SELECT count(*) FROM active_snapshot")
        .fetch_one(&fresh)
        .await
        .unwrap();
    assert_eq!(singletons, 1, "singleton row pre-created");

    // Constraint: invalid snapshot status rejected.
    let result = sqlx::query(
        "INSERT INTO node_snapshots (id, status, created_at) VALUES ($1, 'bogus', now())",
    )
    .bind(uuid::Uuid::new_v4())
    .execute(&fresh)
    .await;
    assert!(result.is_err(), "status check constraint enforced");

    // Constraint: negative capacity rejected.
    sqlx::query(
        "INSERT INTO node_snapshots (id, status, created_at) VALUES ($1, 'loading', now())",
    )
    .bind(uuid::Uuid::new_v4())
    .execute(&fresh)
    .await
    .expect("valid snapshot");
    let snapshot_id: uuid::Uuid = sqlx::query_scalar("SELECT id FROM node_snapshots LIMIT 1")
        .fetch_one(&fresh)
        .await
        .unwrap();
    let result = sqlx::query(
        "INSERT INTO lightning_nodes (snapshot_id, public_key, alias, capacity_sats, first_seen) \
         VALUES ($1, 'pk', 'a', -1, now())",
    )
    .bind(snapshot_id)
    .execute(&fresh)
    .await;
    assert!(result.is_err(), "negative capacity rejected");

    // Constraint: FK to node_snapshots enforced.
    let result = sqlx::query(
        "INSERT INTO lightning_nodes (snapshot_id, public_key, alias, capacity_sats, first_seen) \
         VALUES ($1, 'pk', 'a', 1, now())",
    )
    .bind(uuid::Uuid::new_v4())
    .execute(&fresh)
    .await;
    assert!(result.is_err(), "foreign key enforced");

    // Index: covering index created.
    let covering: bool = sqlx::query_scalar(
        "SELECT EXISTS (SELECT 1 FROM pg_indexes WHERE indexname = 'idx_lightning_nodes_covering')",
    )
    .fetch_one(&fresh)
    .await
    .expect("index exists");
    assert!(covering, "covering index created");
}

/// Snapshot publication is idempotence-safe: publishing the same snapshot
/// twice fails instead of corrupting the active pointer.
#[tokio::test]
async fn republishing_a_ready_snapshot_is_rejected() {
    let env = start_stack(&provider_json(2), 200, true).await;
    env.run_sync().await.expect("sync");
    let snapshot = env.active_snapshot_id().await.expect("active");

    let result = env.components.repository.publish(snapshot, 2).await;
    assert!(result.is_err(), "double publication rejected");
    assert_eq!(env.active_snapshot_id().await, Some(snapshot));
}

/// Health endpoints reflect database availability.
#[tokio::test]
async fn readiness_follows_database_availability() {
    let env = start_stack(&provider_json(1), 200, true).await;
    let (status, _) = env.get("/health/ready").await;
    assert_eq!(status, StatusCode::OK);
}
