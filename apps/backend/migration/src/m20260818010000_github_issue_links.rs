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
                id                UUID PRIMARY KEY,
                project_id        UUID NOT NULL REFERENCES projects(id) ON DELETE CASCADE,
                integration_id    UUID NOT NULL REFERENCES github_integrations(id) ON DELETE CASCADE,
                task_id           UUID NOT NULL UNIQUE REFERENCES tasks(id) ON DELETE CASCADE,
                github_number     INT NOT NULL,
                synced_hash       VARCHAR NOT NULL,
                github_updated_at TIMESTAMPTZ NOT NULL,
                pending_push      BOOLEAN NOT NULL DEFAULT false,
                updated_at        TIMESTAMPTZ NOT NULL DEFAULT now(),
                UNIQUE (project_id, github_number)
            )
        "#,
            )
            .await?;
        // 連携解除（github_integrations の削除）のカスケード削除で使う
        manager
            .get_connection()
            .execute_unprepared(
                "CREATE INDEX idx_github_issue_links_integration ON github_issue_links(integration_id)",
            )
            .await?;
        // 書き戻し要求の残留分を掃くスイープが pending_push だけで引く
        manager
            .get_connection()
            .execute_unprepared(
                "CREATE INDEX idx_github_issue_links_pending ON github_issue_links(id) WHERE pending_push",
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
