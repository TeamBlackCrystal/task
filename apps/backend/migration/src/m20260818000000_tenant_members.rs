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
            CREATE TABLE tenant_members (
                id        UUID PRIMARY KEY,
                tenant_id UUID NOT NULL REFERENCES tenants(id) ON DELETE CASCADE,
                user_id   UUID NOT NULL REFERENCES users(id) ON DELETE CASCADE,
                role      VARCHAR NOT NULL,
                UNIQUE (tenant_id, user_id),
                CONSTRAINT tenant_members_role_check CHECK (role IN ('Admin', 'Member', 'Viewer'))
            )
        "#,
            )
            .await?;
        // UNIQUE (tenant_id, user_id) の索引は先頭列が tenant_id なので user_id 単独では効かない。
        // ログインのたびに通る 2FA 強制の判定（`service::login_session`）が user_id だけで引く
        manager
            .get_connection()
            .execute_unprepared(
                "CREATE INDEX idx_tenant_members_user ON tenant_members(user_id)",
            )
            .await?;
        Ok(())
    }

    async fn down(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager
            .get_connection()
            .execute_unprepared("DROP TABLE IF EXISTS tenant_members CASCADE")
            .await?;
        Ok(())
    }
}
