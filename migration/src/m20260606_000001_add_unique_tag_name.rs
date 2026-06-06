use sea_orm_migration::prelude::*;

#[derive(DeriveMigrationName)]
pub struct Migration;

#[async_trait::async_trait]
impl MigrationTrait for Migration {
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        let db = manager.get_connection();

        // Deduplicate existing tags before adding UNIQUE constraint.
        // Strategy: for each group of duplicate tag names, keep the one with
        // the smallest ID, reassign image_tag_association rows to it, then
        // delete the duplicates.
        db.execute_unprepared(
            r#"
DO $$
DECLARE
    keeper_id INT;
    dup_tag RECORD;
BEGIN
    FOR dup_tag IN
        SELECT t.id, t.name
        FROM tags t
        WHERE t.name IN (
            SELECT name FROM tags GROUP BY name HAVING COUNT(*) > 1
        )
        AND t.id != (
            SELECT MIN(t2.id) FROM tags t2 WHERE t2.name = t.name
        )
        ORDER BY t.id
    LOOP
        SELECT MIN(t2.id) INTO keeper_id FROM tags t2 WHERE t2.name = dup_tag.name;

        -- Remove duplicate associations where the image already
        -- has the keeper tag (would violate composite PK on update)
        DELETE FROM image_tag_association
        WHERE tag_id = dup_tag.id
        AND image_id IN (
            SELECT image_id FROM image_tag_association WHERE tag_id = keeper_id
        );

        -- Reassign remaining associations to the keeper tag
        UPDATE image_tag_association SET tag_id = keeper_id WHERE tag_id = dup_tag.id;

        -- Delete the duplicate tag row
        DELETE FROM tags WHERE id = dup_tag.id;
    END LOOP;
END $$;
"#,
        )
        .await?;

        // Add UNIQUE constraint on tags.name
        db.execute_unprepared("ALTER TABLE tags ADD CONSTRAINT uq_tags_name UNIQUE (name)")
            .await?;

        Ok(())
    }

    async fn down(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        let db = manager.get_connection();
        db.execute_unprepared("ALTER TABLE tags DROP CONSTRAINT IF EXISTS uq_tags_name")
            .await?;
        Ok(())
    }
}
