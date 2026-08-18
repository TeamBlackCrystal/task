use sea_orm_migration::prelude::*;

#[derive(DeriveMigrationName)]
pub struct Migration;

#[async_trait::async_trait]
impl MigrationTrait for Migration {
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager
            .get_connection()
            .execute_unprepared(
                r#"
            CREATE TABLE github_issue_links (
                id            UUID PRIMARY KEY,
                project_id    UUID NOT NULL REFERENCES projects(id) ON DELETE CASCADE,
                task_id       UUID NOT NULL UNIQUE REFERENCES tasks(id) ON DELETE CASCADE,
                github_number INT NOT NULL,
                synced_hash   VARCHAR NOT NULL,
                updated_at    TIMESTAMPTZ NOT NULL DEFAULT now(),
                UNIQUE (project_id, github_number)
            )
        "#,
            )
            .await?;
        Ok(())
    }

    async fn down(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager
            .get_connection()
            .execute_unprepared("DROP TABLE IF EXISTS github_issue_links CASCADE")
            .await?;
        Ok(())
    }
}
