use sea_orm_migration::prelude::*;

/// SQL の本体は `apps/backend/sql/` に置いて統合テストと共有する。
///
/// この crate はワークスペースから `exclude` されているので、テスト側から関数を
/// 呼べない。SQL を両方に書くと片方だけ直して気づけないため、ファイルを 1 つに
/// して `include_str!` で読む（コンパイル時に埋め込むので実行時のファイル依存は無い）。
const BACKFILL_SQL: &str = include_str!("../../sql/backfill_drive_project_ids.sql");

#[derive(DeriveMigrationName)]
pub struct Migration;

#[async_trait::async_trait]
impl MigrationTrait for Migration {
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager
            .get_connection()
            .execute_unprepared(BACKFILL_SQL)
            .await?;
        Ok(())
    }

    /// 巻き戻さない。
    ///
    /// backfill 前の `project_id` は「NULL だった」以上の情報が残っておらず、
    /// 元に戻すと直したはずの穴（プロジェクト限定のファイルが一般ファイルとして
    /// 非メンバーに見える）をわざわざ開けることになる。
    async fn down(&self, _manager: &SchemaManager) -> Result<(), DbErr> {
        Ok(())
    }
}
