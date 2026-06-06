use sea_orm_migration::prelude::*;

/// Converts `images.colors` from `json` to `jsonb`.
///
/// **Why this migration exists:**
/// - `json` type lacks an equality operator in PostgreSQL, so SeaORM's
///   `.distinct()` on tag-filtered queries failed with:
///   "could not identify an equality operator for type json"
/// - `jsonb` is the standard PostgreSQL JSON type with full operator support
///   (equality, indexing, containment), making `.distinct()` work correctly.
/// - Alternatives (GROUP BY, DISTINCT ON) were rejected due to JOIN conflicts
///   and ORDER BY constraints respectively.
#[derive(DeriveMigrationName)]
pub struct Migration;

#[async_trait::async_trait]
impl MigrationTrait for Migration {
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager
            .get_connection()
            .execute_unprepared(
                "ALTER TABLE images ALTER COLUMN colors TYPE jsonb USING colors::jsonb",
            )
            .await?;
        Ok(())
    }

    async fn down(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager
            .get_connection()
            .execute_unprepared(
                "ALTER TABLE images ALTER COLUMN colors TYPE json USING colors::json",
            )
            .await?;
        Ok(())
    }
}
