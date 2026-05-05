# Plan: Release Info Sync in psync

This plan adds release synchronization to psync, allowing it to pull release information from Azure DevOps.

> **Note**: Jira support is out of scope for now (future work).

## 0. Design Decision: Single Trait

Use a single `RemoteSync` trait with a `Data` struct containing all syncable entities. Each impl populates what it has:

- Azure DevOps: fills work, people, releases
- Jira: fills work, people (releases = empty for now)

This keeps the API simple and extensible.

---

## 1. Database Layer (`pstore` crate)

### Model (`models.rs`)

```rust
#[derive(FromRow, Debug, Serialize, Deserialize)]
pub struct Release {
    pub id: i64,                      // Auto-increment primary key
    pub remote_id: i64,               // FK to remote
    pub project: String,              // Project name
    pub release_id: String,           // Remote's release identifier
    pub name: String,                 // Release name (e.g., "v2.1.0", "Release-2024-Q1")
    pub environment: Option<String>,  // Dev, Staging, Production
    pub started_at: Option<DateTime<Utc>>,
    pub deployed_at: Option<DateTime<Utc>>,
    pub status: Option<String>,       // pending, in_progress, succeeded, failed, cancelled
    pub url: Option<String>,
}
```

### Update Existing `Data` Struct

```rust
#[derive(Serialize, Deserialize, Default)]
pub struct Data {
    pub work: Vec<Work>,
    pub people: Vec<Person>,
    pub releases: Vec<Release>,  // Add this
}
```

### Migration

Create `pstore/migrations/20260505000001_add_releases.up.sql`:

```sql
CREATE TABLE release (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    remote_id INTEGER NOT NULL,
    project TEXT NOT NULL,
    release_id TEXT NOT NULL,
    name TEXT NOT NULL,
    environment TEXT,
    started_at TEXT,
    deployed_at TEXT,
    status TEXT,
    url TEXT,
    UNIQUE(remote_id, project, release_id)
);
```

### Queries (`queries.rs`)

- `get_releases(pool)` - Fetch all releases
- `get_releases_by_project(pool, project, remote_id)` - Filter by project (optional)
- `Release::save()` - Upsert using `INSERT OR REPLACE`

---

## 2. Sync Abstraction (`psync/src/remote.rs`)

The existing `RemoteSync` trait stays as-is:

```rust
#[async_trait]
pub trait RemoteSync: Send + Sync {
    async fn sync(&self, client: &Client, remote_id: i64, pool: &Pool) -> Result<Data>;
}
```

Each impl populates `Data.releases` if supported, empty if not.

---

## 3. Azure DevOps Implementation (`psync/src/azure_devops.rs`)

### API Endpoint

Releases: `GET https://vsrm.dev.azure.com/{org}/{project}/_apis/release/releases?api-version=7.1`

### New Structs

```rust
#[derive(Deserialize, Debug)]
struct AzureRelease {
    id: i32,
    name: String,
    status: String,
    #[serde(rename = "createdOn")]
    created_on: Option<DateTime<Utc>>,
    #[serde(rename = "modifiedOn")]
    modified_on: Option<DateTime<Utc>>,
    #[serde(rename = "environments")]
    environments: Option<Vec<AzureReleaseEnvironment>>,
}

#[derive(Deserialize, Debug)]
struct AzureReleaseEnvironment {
    id: i32,
    name: String,           // Dev, QA, Production
    status: String,
    #[serde(rename = "deployedOn")]
    deployed_on: Option<DateTime<Utc>>,
}
```

### Implementation

In the existing `impl RemoteSync for AzureDevops` (in `sync()`), add release fetching alongside work item sync:

```rust
// After fetching work items, fetch releases
let releases = fetch_azure_releases(&client, &self.org, &self.pat).await?;
data.releases = releases;
```

Helper function:

```rust
async fn fetch_azure_releases(
    client: &Client,
    org: &str,
    pat: &str,
) -> Result<Vec<Release>> {
    let mut releases = Vec::new();

    // 1. Fetch all projects
    let projects = get_azure_projects(client, org, pat).await?;

    // 2. For each project, fetch releases
    for project in projects {
        let project_name = project["name"].as_str().unwrap();
        let releases_url = format!(
            "https://vsrm.dev.azure.com/{}/{}/_apis/release/releases?api-version=7.1",
            org, project_name
        );
        
        let response: Value = client
            .get(&releases_url)
            .bearer_auth(pat)
            .send()
            .await?;

        let azure_releases: Vec<AzureRelease> = response["value"]
            .as_array()
            .map(|arr| serde_json::from_value(Value::Array(arr.clone())).unwrap_or_default())
            .unwrap_or_default();

        for azure_release in azure_releases {
            let environments = azure_release.environments.unwrap_or_default();
            
            if environments.is_empty() {
                // Release without environments
                releases.push(Release {
                    remote_id: 0, // Will be set by caller
                    project: project_name.to_string(),
                    release_id: azure_release.id.to_string(),
                    name: azure_release.name.clone(),
                    environment: None,
                    started_at: azure_release.created_on,
                    deployed_at: None,
                    status: Some(map_azure_status(azure_release.status)),
                    url: None,
                });
            } else {
                // One Release per environment
                for env in environments {
                    releases.push(Release {
                        remote_id: 0,
                        project: project_name.to_string(),
                        release_id: format!("{}-{}", azure_release.id, env.id),
                        name: azure_release.name.clone(),
                        environment: Some(env.name),
                        started_at: azure_release.created_on,
                        deployed_at: env.deployed_on,
                        status: Some(map_azure_status(env.status)),
                        url: None,
                    });
                }
            }
        }
    }

    Ok(releases)
}

fn map_azure_status(status: String) -> String {
    match status.as_str() {
        "notStarted" => "pending",
        "inProgress" => "in_progress",
        "succeeded" => "succeeded",
        "failed" => "failed",
        "cancelled" => "cancelled",
        _ => &status,
    }.to_string()
}
```

### Notes

- Each Azure release-environment combo becomes one row
- Use `release_id = "{azure_release_id}-{environment_id}"` for uniqueness

---

## 4. Jira Implementation (`psync/src/jira.rs`)

No changes needed - Jira doesn't support releases (yet). The `Data.releases` will be empty.

---

## 5. Library Function (`psync/src/lib.rs`)

Rename `pull_remote_work()` to `pull()` since it now syncs everything:

```rust
pub async fn pull(remote: &Remote, client: &Client, pool: &Pool) -> Result<(), Error> {
    let data: Data = match remote.kind {
        Kind::AzureDevops => {
            get_password(remote)
                .map(|pat| AzureDevops {
                    org: remote.name.to_owned(),
                    pat: pat.to_owned(),
                })?
                .sync(client, remote.id, pool)
                .await?
        }
        Kind::Jira => {
            // ... existing Jira sync
        }
    };

    let mut tx = pool.begin().await?;
    for work_item in data.work {
        work_item.save(&mut *tx).await?;
    }
    for person in data.people {
        person.save(&mut *tx).await?;
    }
    for release in data.releases {
        release.save(&mut *tx).await?;
    }
    tx.commit().await?;

    Ok(())
}
```

---

## 6. CLI Commands (`psync/src/main.rs`)

Flat command structure - no subcommands for pull/push:

```rust
#[derive(Debug, Parser)]
#[command(name = "psync")]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Debug, Subcommand)]
enum Command {
    Pull,                   // Sync work AND releases from all remotes
    Push,                   // Push time entries to all remotes
    List(ListCommand),      // List stored data
}

#[derive(Debug, Subcommand)]
enum ListCommand {
    Work,       // List work items (default)
    Releases,   // List releases
}
```

### Usage

```bash
psync pull              # Sync work items AND releases from all remotes
psync push              # Push time entries to all remotes
psync list              # List stored work items
psync list work         # List stored work items (explicit)
psync list releases     # List stored releases
```

---

## 7. Implementation Order

1. [x] Add `Release` model to `models.rs`
2. [x] Add `releases: Vec<Release>` to `Data` struct
3. [x] Create migration `20260505000001_add_releases.up.sql`
4. [x] Add `get_releases()` and `Release::save()` to `queries.rs`
5. [x] Add `AzureRelease` and `AzureReleaseEnvironment` structs to `azure_devops.rs`
6. [x] Add `fetch_azure_releases()` helper function
7. [x] Update `AzureDevops::sync()` to populate `data.releases`
8. [x] Update `pull()` in `lib.rs` to save releases
9. [x] Restructure CLI to `psync {pull,push,list}` flat structure
10. [x] Add `psync list releases` command
11. [x] Test with `cargo test -p psync`
12. [x] Update Jira impl to leave `releases` empty
13. [x] Make release sync parallel using JoinSet

---

## Open Questions

1. **Incremental sync**: If Azure DevOps releases API supports `modifiedOn` filtering, use it; otherwise do full sync (can be refined later)