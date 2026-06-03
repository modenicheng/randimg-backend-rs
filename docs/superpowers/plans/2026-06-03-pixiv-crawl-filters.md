# Pixiv Crawl Filters Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Add image type (illustration/manga), R18 exclusion, and AI exclusion filters to the Pixiv crawl task creation API, with post-hoc filtering during crawling and metadata persistence in the database.

**Architecture:** Three new optional fields (`exclude_r18`, `exclude_ai`, and enhanced `illust_type` filtering) are added to the HTTP request DTO and CrawlJob struct. During crawling, each illustration is checked against these filters before saving. The `illust_type`, `x_restrict`, and `illust_ai_type` values from Pixiv's API response are persisted in the `images` table for future querying.

**Tech Stack:** Rust, Axum, SeaORM, Apalis, pixiv-client crate, SQLite/PostgreSQL

---

## File Structure

| File | Action | Purpose |
|------|--------|---------|
| `migration/src/m20260603_000001_add_illust_metadata.rs` | Create | Add `illust_type`, `x_restrict`, `illust_ai_type` columns to `images` table |
| `migration/src/lib.rs` | Modify | Register new migration |
| `src/db/entities/image.rs` | Modify | Add new columns to SeaORM entity model |
| `src/task_queue/jobs.rs` | Modify | Add `exclude_r18`, `exclude_ai` fields to `CrawlJob` |
| `src/handlers/crawler.rs` | Modify | Add `exclude_r18`, `exclude_ai` to `CreateCrawlerRequest`; pass to `CrawlJob` |
| `src/task_queue/handlers.rs` | Modify | Add filtering logic in crawl functions; extract and store new fields in `save_illust` |
| `src/db/query/image.rs` | Modify | Handle new fields in `create_image` |

---

### Task 1: Database Migration — Add Illust Metadata Columns

**Files:**
- Create: `migration/src/m20260603_000001_add_illust_metadata.rs`
- Modify: `migration/src/lib.rs`

- [ ] **Step 1: Create the migration file**

```rust
// migration/src/m20260603_000001_add_illust_metadata.rs
use sea_orm_migration::prelude::*;

#[derive(DeriveMigrationName)]
pub struct Migration;

#[async_trait::async_trait]
impl MigrationTrait for Migration {
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        let db = manager.get_connection();
        // illust_type: "illust", "manga", "ugoira" (from Pixiv IllustType enum)
        db.execute_unprepared("ALTER TABLE images ADD COLUMN illust_type TEXT")
            .await?;
        // x_restrict: 0 = safe, 1 = R18, 2 = R18G
        db.execute_unprepared("ALTER TABLE images ADD COLUMN x_restrict INTEGER NOT NULL DEFAULT 0")
            .await?;
        // illust_ai_type: 0 = non-AI, 1 = AI-generated, 2 = AI-assisted
        db.execute_unprepared("ALTER TABLE images ADD COLUMN illust_ai_type INTEGER NOT NULL DEFAULT 0")
            .await?;
        Ok(())
    }

    async fn down(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        let db = manager.get_connection();
        let _ = db.execute_unprepared("ALTER TABLE images DROP COLUMN illust_type").await;
        let _ = db.execute_unprepared("ALTER TABLE images DROP COLUMN x_restrict").await;
        let _ = db.execute_unprepared("ALTER TABLE images DROP COLUMN illust_ai_type").await;
        Ok(())
    }
}
```

- [ ] **Step 2: Register the migration in `migration/src/lib.rs`**

Add to the migrations list (after the last existing migration):

```rust
mod m20260603_000001_add_illust_metadata;
```

And add to the migrations vec:

```rust
Box::new(m20260603_000001_add_illust_metadata::Migration),
```

- [ ] **Step 3: Verify migration compiles**

Run: `cargo build -p migration`
Expected: Compiles without errors

- [ ] **Step 4: Commit**

```bash
git add migration/src/m20260603_000001_add_illust_metadata.rs migration/src/lib.rs
git commit -m "feat(db): add illust_type, x_restrict, illust_ai_type columns to images table"
```

---

### Task 2: Update Image Entity Model

**Files:**
- Modify: `src/db/entities/image.rs`

- [ ] **Step 1: Add new columns to the Model struct**

Add these three fields to the `Model` struct (after `deleted_at` on line 37):

```rust
    pub illust_type: Option<String>,
    #[sea_orm(default_value = 0)]
    pub x_restrict: i32,
    #[sea_orm(default_value = 0)]
    pub illust_ai_type: i32,
```

The full struct should look like:

```rust
#[derive(Clone, Debug, PartialEq, DeriveEntityModel, Serialize, Deserialize)]
#[sea_orm(table_name = "images")]
pub struct Model {
    #[sea_orm(primary_key)]
    pub id: i32,
    pub title: String,
    pub image_path: String,
    pub source_url: Option<String>,
    pub source_id: Option<i32>,
    pub source_image_url: Option<String>,
    pub author_id: i32,
    pub width: i32,
    pub height: i32,
    pub aspect_ratio: f32,
    pub colors: Option<JsonValue>,
    pub primary_l: Option<f64>,
    pub primary_a: Option<f64>,
    pub primary_b: Option<f64>,
    pub accessible: Option<bool>,
    pub avatar_available: Option<bool>,
    pub is_public: bool,
    pub downloaded: bool,
    pub source_created_at: Option<NaiveDateTime>,
    #[sea_orm(column_type = "BigInteger")]
    pub total_view: i64,
    #[sea_orm(column_type = "BigInteger")]
    pub total_bookmarks: i64,
    #[sea_orm(column_type = "BigInteger")]
    pub total_comments: i64,
    pub fetched_times: i32,
    pub created_at: NaiveDateTime,
    pub deleted_at: Option<NaiveDateTime>,
    pub illust_type: Option<String>,
    #[sea_orm(default_value = 0)]
    pub x_restrict: i32,
    #[sea_orm(default_value = 0)]
    pub illust_ai_type: i32,
}
```

- [ ] **Step 2: Verify entity compiles**

Run: `cargo check`
Expected: Compiles without errors

- [ ] **Step 3: Commit**

```bash
git add src/db/entities/image.rs
git commit -m "feat(entity): add illust_type, x_restrict, illust_ai_type to Image model"
```

---

### Task 3: Update CrawlJob Struct

**Files:**
- Modify: `src/task_queue/jobs.rs`

- [ ] **Step 1: Add filter fields to CrawlJob**

Add these fields after `illust_type` (line 17):

```rust
    /// Exclude R18 content (x_restrict > 0). Default: false.
    #[serde(default)]
    pub exclude_r18: Option<bool>,
    /// Exclude AI-generated content (illust_ai_type > 0). Default: false.
    #[serde(default)]
    pub exclude_ai: Option<bool>,
```

The full CrawlJob struct should look like:

```rust
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CrawlJob {
    pub crawler_id: i32,
    pub crawl_type: i32,
    pub target_user_id: Option<String>,
    pub target_start_date: Option<String>,
    pub target_end_date: Option<String>,
    pub target_search_prompt: Option<String>,
    /// Ranking mode: "day", "week", "month", "original", "rookie", "daily_r18", "weekly_r18" (default: "day").
    #[serde(default)]
    pub ranking_mode: Option<String>,
    /// User illust type: "illust", "manga" (default: "illust").
    #[serde(default)]
    pub illust_type: Option<String>,
    /// Exclude R18 content (x_restrict > 0). Default: false.
    #[serde(default)]
    pub exclude_r18: Option<bool>,
    /// Exclude AI-generated content (illust_ai_type > 0). Default: false.
    #[serde(default)]
    pub exclude_ai: Option<bool>,
    /// Maximum total pages to crawl (0 or None = unlimited).
    #[serde(default)]
    pub max_pages: Option<u32>,
    /// Max discover hops to run after crawl (override global default).
    #[serde(default)]
    pub discover_hops: Option<u32>,
    /// Max seed limit for discover (override global default).
    #[serde(default)]
    pub discover_seed_limit: Option<u64>,
    /// Seed selection method for discover: "popularity", "views", "bookmarks", "random".
    #[serde(default)]
    pub discover_seed_method: Option<String>,
    /// Parent task ID for hierarchy tracking.
    #[serde(default)]
    pub parent_job_id: Option<String>,
}
```

- [ ] **Step 2: Verify compilation**

Run: `cargo check`
Expected: Compiles without errors

- [ ] **Step 3: Commit**

```bash
git add src/task_queue/jobs.rs
git commit -m "feat(jobs): add exclude_r18 and exclude_ai filter fields to CrawlJob"
```

---

### Task 4: Update HTTP Request DTO and Handler

**Files:**
- Modify: `src/handlers/crawler.rs`

- [ ] **Step 1: Add filter fields to CreateCrawlerRequest**

Add these fields after `illust_type` (line 38):

```rust
    /// Exclude R18 content (x_restrict > 0). Default: false.
    pub exclude_r18: Option<bool>,
    /// Exclude AI-generated content (illust_ai_type > 0). Default: false.
    pub exclude_ai: Option<bool>,
```

The full struct should look like:

```rust
#[derive(Deserialize)]
pub struct CreateCrawlerRequest {
    pub task_name: Option<String>,
    pub crawl_type: Option<i32>,
    pub target_user_id: Option<String>,
    pub target_start_date: Option<chrono::NaiveDateTime>,
    pub target_end_date: Option<chrono::NaiveDateTime>,
    pub target_search_prompt: Option<String>,
    /// Ranking mode (crawl_type=0): "day", "week", "month", "original", "rookie". Default: "day".
    pub ranking_mode: Option<String>,
    /// User illust type (crawl_type=1): "illust", "manga". Default: "illust".
    pub illust_type: Option<String>,
    /// Exclude R18 content (x_restrict > 0). Default: false.
    pub exclude_r18: Option<bool>,
    /// Exclude AI-generated content (illust_ai_type > 0). Default: false.
    pub exclude_ai: Option<bool>,
    /// Maximum total pages to crawl per run (0 = unlimited). Default: unlimited.
    pub max_pages: Option<u32>,
    /// Max discover hops after crawl finishes (overrides global default). Default: use global.
    pub discover_hops: Option<u32>,
    /// Max seed images for discover (overrides global default). Default: use global.
    pub discover_seed_limit: Option<u64>,
    /// Seed selection method for discover: "popularity", "views", "bookmarks", "random". Default: "popularity".
    pub discover_seed_method: Option<String>,
}
```

- [ ] **Step 2: Pass new fields to CrawlJob in create_crawler handler**

In the `create_crawler` function, add the new fields to the CrawlJob construction (after `illust_type: body.illust_type,` on line 92):

```rust
            exclude_r18: body.exclude_r18,
            exclude_ai: body.exclude_ai,
```

The full CrawlJob construction should look like:

```rust
        .push_crawl(CrawlJob {
            crawler_id: crawler.id,
            crawl_type,
            target_user_id: body.target_user_id,
            target_start_date: body.target_start_date.map(|d| d.to_string()),
            target_end_date: body.target_end_date.map(|d| d.to_string()),
            target_search_prompt: body.target_search_prompt,
            ranking_mode: body.ranking_mode,
            illust_type: body.illust_type,
            exclude_r18: body.exclude_r18,
            exclude_ai: body.exclude_ai,
            max_pages: body.max_pages,
            discover_hops: body.discover_hops,
            discover_seed_limit: body.discover_seed_limit,
            discover_seed_method: body.discover_seed_method,
            parent_job_id: None,
        })
```

- [ ] **Step 3: Verify compilation**

Run: `cargo check`
Expected: Compiles without errors

- [ ] **Step 4: Commit**

```bash
git add src/handlers/crawler.rs
git commit -m "feat(api): add exclude_r18 and exclude_ai to CreateCrawlerRequest"
```

---

### Task 5: Update save_illust to Extract and Store Metadata

**Files:**
- Modify: `src/task_queue/handlers.rs`

- [ ] **Step 1: Add filtering helper function**

Add this helper function before `save_illust` (around line 303):

```rust
/// Check if an illust should be skipped based on crawl job filters.
fn should_skip_illust(illust: &crate::pixiv::Illust, job: &CrawlJob) -> bool {
    // Filter R18 content
    if job.exclude_r18.unwrap_or(false) {
        if let Some(x_restrict) = illust.x_restrict {
            if x_restrict > 0 {
                tracing::debug!(illust_id = illust.id, x_restrict, "Skipping R18 illust");
                return true;
            }
        }
    }

    // Filter AI-generated content
    if job.exclude_ai.unwrap_or(false) {
        if let Some(ai_type) = illust.illust_ai_type {
            if ai_type > 0 {
                tracing::debug!(illust_id = illust.id, illust_ai_type = ai_type, "Skipping AI illust");
                return true;
            }
        }
    }

    // Filter by illust type (for ranking and bookmarks, where API doesn't filter)
    // Note: crawl_user already filters via API param, but this provides consistent behavior
    if let Some(ref filter_type) = job.illust_type {
        if let Some(ref illust_type) = illust.r#type {
            let illust_type_str = match illust_type {
                crate::pixiv::IllustType::Illust => "illust",
                crate::pixiv::IllustType::Manga => "manga",
                crate::pixiv::IllustType::Ugoira => "ugoira",
            };
            if illust_type_str != filter_type.as_str() {
                tracing::debug!(
                    illust_id = illust.id,
                    illust_type = illust_type_str,
                    filter_type = filter_type.as_str(),
                    "Skipping non-matching illust type"
                );
                return true;
            }
        }
    }

    false
}
```

- [ ] **Step 2: Update save_illust to accept CrawlJob and apply filters**

Change the `save_illust` function signature to accept the job:

```rust
async fn save_illust(
    state: &AppState,
    illust: &crate::pixiv::Illust,
    job: &CrawlJob,
) -> Result<Vec<DownloadInfo>, String> {
```

Add the filter check at the beginning of the function (after line 316):

```rust
    // Apply filters before processing
    if should_skip_illust(illust, job) {
        return Ok(Vec::new());
    }
```

- [ ] **Step 3: Update save_illust to extract and store metadata**

In the `image_data` JSON construction (around line 369), add the new fields:

```rust
        let image_data = serde_json::json!({
            "title": illust.title,
            "image_path": image_path,
            "source_url": format!("https://www.pixiv.net/artworks/{}", illust.id),
            "source_id": illust.id as i64,
            "source_image_url": image_url,
            "author_id": author.id,
            "width": width,
            "height": height,
            "aspect_ratio": aspect_ratio,
            "source_created_at": illust.create_date
                .map(|d| d.naive_utc().format("%Y-%m-%d %H:%M:%S").to_string()),
            "total_view": illust.total_view.unwrap_or(0),
            "total_bookmarks": illust.total_bookmarks.unwrap_or(0),
            "total_comments": illust.total_comments.unwrap_or(0),
            "illust_type": illust.r#type.as_ref().map(|t| match t {
                crate::pixiv::IllustType::Illust => "illust",
                crate::pixiv::IllustType::Manga => "manga",
                crate::pixiv::IllustType::Ugoira => "ugoira",
            }),
            "x_restrict": illust.x_restrict.unwrap_or(0),
            "illust_ai_type": illust.illust_ai_type.unwrap_or(0),
        });
```

- [ ] **Step 4: Update all crawl functions to pass job to save_illust**

In `crawl_ranking` (line 124), change:
```rust
let downloads = save_illust(state, illust).await?;
```
to:
```rust
let downloads = save_illust(state, illust, job).await?;
```

In `crawl_user` (line 197), change:
```rust
let downloads = save_illust(state, illust).await?;
```
to:
```rust
let downloads = save_illust(state, illust, job).await?;
```

In `crawl_bookmarks` (line 268), change:
```rust
let downloads = save_illust(state, illust).await?;
```
to:
```rust
let downloads = save_illust(state, illust, job).await?;
```

- [ ] **Step 5: Verify compilation**

Run: `cargo check`
Expected: Compiles without errors

- [ ] **Step 6: Commit**

```bash
git add src/task_queue/handlers.rs
git commit -m "feat(crawl): add R18/AI/type filtering and metadata extraction in save_illust"
```

---

### Task 6: Update create_image to Handle New Fields

**Files:**
- Modify: `src/db/query/image.rs`

- [ ] **Step 1: Add new fields to create_image**

In the `create_image` function (around line 562), add the new fields to the ActiveModel:

```rust
    let model = image::ActiveModel {
        title: Set(data["title"].as_str().unwrap_or("").to_string()),
        image_path: Set(data["image_path"].as_str().unwrap_or("").to_string()),
        source_url: Set(data["source_url"].as_str().map(|s| s.to_string())),
        source_id: Set(data["source_id"].as_i64().map(|v| v as i32)),
        source_image_url: Set(data["source_image_url"].as_str().map(|s| s.to_string())),
        author_id: Set(data["author_id"].as_i64().unwrap_or(0) as i32),
        width: Set(data["width"].as_i64().unwrap_or(0) as i32),
        height: Set(data["height"].as_i64().unwrap_or(0) as i32),
        aspect_ratio: Set(data["aspect_ratio"].as_f64().unwrap_or(0.0) as f32),
        colors: Set(data.get("colors").cloned()),
        source_created_at: Set(source_created_at),
        total_view: Set(data["total_view"].as_i64().unwrap_or(0)),
        total_bookmarks: Set(data["total_bookmarks"].as_i64().unwrap_or(0)),
        total_comments: Set(data["total_comments"].as_i64().unwrap_or(0)),
        illust_type: Set(data["illust_type"].as_str().map(|s| s.to_string())),
        x_restrict: Set(data["x_restrict"].as_i64().unwrap_or(0) as i32),
        illust_ai_type: Set(data["illust_ai_type"].as_i64().unwrap_or(0) as i32),
        ..Default::default()
    };
```

- [ ] **Step 2: Verify compilation**

Run: `cargo check`
Expected: Compiles without errors

- [ ] **Step 3: Commit**

```bash
git add src/db/query/image.rs
git commit -m "feat(db): store illust_type, x_restrict, illust_ai_type in create_image"
```

---

### Task 7: Verify End-to-End Compilation

**Files:**
- None (verification only)

- [ ] **Step 1: Run full cargo check**

Run: `cargo check`
Expected: Compiles without errors

- [ ] **Step 2: Run cargo check with postgres feature**

Run: `cargo check --no-default-features --features postgres`
Expected: Compiles without errors

- [ ] **Step 3: Run existing tests**

Run: `cargo test`
Expected: All tests pass

- [ ] **Step 4: Commit any fixes if needed**

If any compilation errors were found and fixed:

```bash
git add -A
git commit -m "fix: resolve compilation issues for illust metadata feature"
```

---

### Task 8: Manual Testing Verification

**Files:**
- None (manual testing)

- [ ] **Step 1: Start the server**

Run: `cargo run`
Expected: Server starts without errors, migration runs automatically

- [ ] **Step 2: Test API with new fields**

Create a crawl task with the new filter options:

```bash
# Test exclude R18
curl -X POST http://localhost:8000/crawler \
  -H "Content-Type: application/json" \
  -H "Authorization: Bearer <token>" \
  -d '{
    "task_name": "Test Crawl - No R18",
    "crawl_type": 0,
    "target_start_date": "2024-01-01T00:00:00",
    "target_end_date": "2024-01-02T00:00:00",
    "ranking_mode": "day",
    "exclude_r18": true,
    "exclude_ai": true,
    "max_pages": 1
  }'
```

Expected: Returns JSON with `id`, `task_name`, `crawl_type`, `status`

- [ ] **Step 3: Verify filtering behavior**

Check the server logs for "Skipping R18 illust" or "Skipping AI illust" messages when crawling with filters enabled.

- [ ] **Step 4: Verify metadata storage**

Query the database to confirm new columns are populated:

```sql
SELECT id, illust_type, x_restrict, illust_ai_type FROM images LIMIT 10;
```

Expected: Values are populated (not all NULL/0)

---

## Summary

This plan adds three filter options to the Pixiv crawl task creation API:

1. **`exclude_r18`** (boolean) — Skip R18 content (x_restrict > 0)
2. **`exclude_ai`** (boolean) — Skip AI-generated content (illust_ai_type > 0)
3. **Enhanced `illust_type` filtering** — Post-hoc filtering for ranking and bookmarks crawls (user crawl already filters at API level)

The implementation:
- Adds 3 new columns to the `images` table for metadata storage
- Updates the HTTP request DTO and CrawlJob struct with new filter fields
- Implements post-hoc filtering in the crawl worker before saving to database
- Extracts and stores Pixiv metadata (illust_type, x_restrict, illust_ai_type) for future querying

**Expected API format after implementation:**

```json
{
  "task_name": "My Crawl",
  "crawl_type": 0,
  "target_start_date": "2024-01-01T00:00:00",
  "target_end_date": "2024-01-07T00:00:00",
  "ranking_mode": "day",
  "illust_type": "illust",
  "exclude_r18": true,
  "exclude_ai": true,
  "max_pages": 10
}
```
