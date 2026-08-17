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
