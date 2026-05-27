use pstore::db::init_with_path;
use pstore::models::{Kind, Timesheet};
use tempfile::tempdir;

#[tokio::test]
async fn save_to_pstore_clears_relevant_entries() {
    // Setup: use temp db with proper schema
    let dir = tempdir().unwrap();
    let db_path = dir.path().join("pstore.db");
    let pool = init_with_path(&db_path).await.unwrap();

    // Seed: add a remote first
    pstore::queries::add_remote(
        &pool,
        Kind::Jira,
        "test-remote",
        "password".to_string(),
    )
    .await
    .unwrap();
    let remotes = pstore::queries::get_remotes(&pool).await.unwrap();
    let remote = remotes.iter().find(|r| r.name == "test-remote").unwrap();

    // Insert an existing timesheet entry for the same remote and date
    let existing_ts = Timesheet {
        remote_id: remote.id,
        ticket_id: "OLD-123".to_string(),
        date: "2026-05-20".to_string(),
        duration: "2h".to_string(),
    };
    existing_ts.save(&pool).await.unwrap();

    // Verify it exists
    let rows: Vec<(String, String, String)> = sqlx::query_as(
        "SELECT ticket_id, date, duration FROM timesheet WHERE remote_id = ?",
    )
    .bind(remote.id)
    .fetch_all(&pool)
    .await
    .unwrap();
    assert!(rows.iter().any(|(tid, _, _)| tid == "OLD-123"));

    // Now simulate the scenario: ptime reads from text file with ticket_id = "PROJ-456"
    // and calls save_to_pstore for the same date. The old entry should be removed.
    use ptime::entries::{Day, Entry, EntryType};
    use chrono::NaiveDate;

    let date = NaiveDate::parse_from_str("2026-05-20", "%Y-%m-%d").unwrap();
    let day = Day {
        date,
        entries: vec![
            Entry {
                start: 900,
                end: Some(1100),
                entry_type: EntryType::Work {
                    client: "test".to_string(),
                    task: "test task".to_string(),
                    ticket_id: Some("PROJ-456".to_string()),
                },
            },
        ],
    };

    // Call save_to_pstore_with_pool - it should clear OLD-123 and insert PROJ-456
    day.save_to_pstore_with_pool(&pool, "test-remote").await.unwrap();

    // Verify: only the new ticket_id should exist
    let rows: Vec<(String, String, String)> = sqlx::query_as(
        "SELECT ticket_id, date, duration FROM timesheet WHERE remote_id = ? AND date = ?",
    )
    .bind(remote.id)
    .bind("2026-05-20")
    .fetch_all(&pool)
    .await
    .unwrap();

    // Should have exactly one entry with the new ticket_id
    assert_eq!(rows.len(), 1, "Expected exactly 1 entry, got {:?}", rows);
    assert_eq!(rows[0].0, "PROJ-456", "Expected new ticket_id PROJ-456, got {}", rows[0].0);
    assert!(
        !rows.iter().any(|(tid, _, _)| tid == "OLD-123"),
        "OLD-123 should be gone"
    );
}