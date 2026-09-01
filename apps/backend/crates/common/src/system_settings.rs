use sea_orm::{ConnectionTrait, DatabaseConnection, DbErr, Statement};

/// `system_settings` の singleton 行を保証する（未適用マイグレーション環境でも register が読めるように）。
pub async fn ensure_system_settings_row(db: &DatabaseConnection) -> Result<(), DbErr> {
    db.execute_raw(Statement::from_string(
        db.get_database_backend(),
        "INSERT INTO system_settings (singleton) VALUES (true) ON CONFLICT (singleton) DO NOTHING"
            .to_string(),
    ))
    .await?;
    Ok(())
}
