use sea_orm_migration::prelude::*;

#[derive(DeriveMigrationName)]
pub struct Migration;

#[async_trait::async_trait]
impl MigrationTrait for Migration {
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        // projects.icon_emoji と同じ形。未設定は NULL のままにして、既定の絵文字は
        // 画面側で補う（DB に既定値を焼き込むと、後で既定を変えたときに追随できない）
        manager
            .get_connection()
            .execute_unprepared("ALTER TABLE tenants ADD COLUMN IF NOT EXISTS icon_emoji VARCHAR")
            .await?;
        Ok(())
    }

    async fn down(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager
            .get_connection()
            .execute_unprepared("ALTER TABLE tenants DROP COLUMN IF EXISTS icon_emoji")
            .await?;
        Ok(())
    }
}
