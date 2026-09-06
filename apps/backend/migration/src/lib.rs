pub use sea_orm_migration::prelude::*;

mod m20260520000000_initial_schema;
mod m20260818000000_tenant_members;
mod m20260818010000_github_issue_links;
mod m20260826000000_review_findings;
mod m20260904000000_drive_project_id_backfill;

pub struct Migrator;

#[async_trait::async_trait]
impl MigratorTrait for Migrator {
    fn migrations() -> Vec<Box<dyn MigrationTrait>> {
        vec![
            Box::new(m20260520000000_initial_schema::Migration),
            Box::new(m20260818000000_tenant_members::Migration),
            Box::new(m20260818010000_github_issue_links::Migration),
            Box::new(m20260826000000_review_findings::Migration),
            Box::new(m20260904000000_drive_project_id_backfill::Migration),
        ]
    }
}
