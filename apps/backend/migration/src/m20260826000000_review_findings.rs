use sea_orm_migration::prelude::*;

#[derive(DeriveMigrationName)]
pub struct Migration;

#[async_trait::async_trait]
impl MigrationTrait for Migration {
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        // レビューラウンド。同一 PR への再レビューは round を増やして新しい行にする
        // （既存行は更新しない）。reviewer_id / actor_id は著者性の記録なので
        // tasks.created_by と同じく NO ACTION（利用者削除は墓標方式で行を残す）。
        manager
            .get_connection()
            .execute_unprepared(
                r#"
            CREATE TABLE reviews (
                id          UUID PRIMARY KEY,
                project_id  UUID NOT NULL REFERENCES projects(id) ON DELETE CASCADE,
                pr_number   INT NOT NULL,
                round       INT NOT NULL,
                head_sha    VARCHAR NOT NULL,
                reviewer_id UUID NOT NULL REFERENCES users(id) ON DELETE NO ACTION,
                summary     TEXT NOT NULL DEFAULT '',
                pr_title    VARCHAR,
                pr_author   VARCHAR,
                created_at  TIMESTAMPTZ NOT NULL DEFAULT now(),
                UNIQUE (project_id, pr_number, round)
            )
        "#,
            )
            .await?;

        // PR 単位の一覧・集計が主な引き方
        manager
            .get_connection()
            .execute_unprepared("CREATE INDEX idx_reviews_project_pr ON reviews(project_id, pr_number)")
            .await?;

        // 指摘。fixed_by は「fixed を宣言した本人は verified にできない」判定に使う。
        // 繰り延べ先タスクが消えてもリンクを NULL にするだけで指摘は残す。
        manager
            .get_connection()
            .execute_unprepared(
                r#"
            CREATE TABLE review_findings (
                id               UUID PRIMARY KEY,
                review_id        UUID NOT NULL REFERENCES reviews(id) ON DELETE CASCADE,
                severity         VARCHAR NOT NULL,
                title            VARCHAR NOT NULL,
                body             TEXT NOT NULL,
                file             VARCHAR,
                line             INT,
                state            VARCHAR NOT NULL,
                deferred_task_id UUID REFERENCES tasks(id) ON DELETE SET NULL,
                fixed_by         UUID REFERENCES users(id) ON DELETE NO ACTION,
                created_at       TIMESTAMPTZ NOT NULL DEFAULT now(),
                updated_at       TIMESTAMPTZ NOT NULL DEFAULT now()
            )
        "#,
            )
            .await?;

        manager
            .get_connection()
            .execute_unprepared("CREATE INDEX idx_review_findings_review ON review_findings(review_id)")
            .await?;

        // 遷移履歴。from_state が NULL の行は起票（登録）を表す。
        manager
            .get_connection()
            .execute_unprepared(
                r#"
            CREATE TABLE review_finding_transitions (
                id         UUID PRIMARY KEY,
                finding_id UUID NOT NULL REFERENCES review_findings(id) ON DELETE CASCADE,
                actor_id   UUID NOT NULL REFERENCES users(id) ON DELETE NO ACTION,
                from_state VARCHAR,
                to_state   VARCHAR NOT NULL,
                note       TEXT,
                created_at TIMESTAMPTZ NOT NULL DEFAULT now()
            )
        "#,
            )
            .await?;

        manager
            .get_connection()
            .execute_unprepared(
                "CREATE INDEX idx_review_finding_transitions_finding ON review_finding_transitions(finding_id, created_at)",
            )
            .await?;

        Ok(())
    }

    async fn down(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        for table in [
            "review_finding_transitions",
            "review_findings",
            "reviews",
        ] {
            manager
                .get_connection()
                .execute_unprepared(&format!("DROP TABLE IF EXISTS {table} CASCADE"))
                .await?;
        }
        Ok(())
    }
}
