use sea_orm_migration::prelude::*;

#[derive(DeriveMigrationName)]
pub struct Migration;

fn use_pg_bigm() -> bool {
    matches!(
        std::env::var("USE_PG_BIGM").as_deref(),
        Ok("1") | Ok("true") | Ok("True") | Ok("TRUE")
    )
}

#[async_trait::async_trait]
impl MigrationTrait for Migration {
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        let conn = manager.get_connection();

        // ── base tables ──────────────────────────────────────────────────────

        conn.execute_unprepared(
            r#"
            CREATE TABLE users (
                id                  UUID PRIMARY KEY,
                username            VARCHAR NOT NULL,
                bio                 TEXT,
                avatar_url          TEXT,
                email               VARCHAR NOT NULL UNIQUE,
                email_verified      BOOLEAN NOT NULL DEFAULT false,
                password_hash       VARCHAR,
                is_admin            BOOLEAN NOT NULL DEFAULT false,
                is_suspended        BOOLEAN NOT NULL DEFAULT false,
                sessions_revoked_at TIMESTAMPTZ,
                totp_enabled        BOOLEAN NOT NULL DEFAULT false
            )
        "#,
        )
        .await?;

        conn.execute_unprepared(
            r#"
            CREATE TABLE tenants (
                id                UUID PRIMARY KEY,
                display_id        VARCHAR NOT NULL UNIQUE,
                name              VARCHAR NOT NULL,
                description       TEXT NOT NULL DEFAULT '',
                icon_url          TEXT NOT NULL DEFAULT '',
                owner_id          UUID NOT NULL REFERENCES users(id) ON DELETE NO ACTION,
                drive_quota_bytes BIGINT,
                require_2fa       BOOLEAN NOT NULL DEFAULT false
            )
        "#,
        )
        .await?;

        conn.execute_unprepared(
            r#"
            CREATE TABLE projects (
                id                UUID PRIMARY KEY,
                name              VARCHAR NOT NULL,
                description       TEXT NOT NULL DEFAULT '',
                tenant_id         UUID NOT NULL REFERENCES tenants(id) ON DELETE CASCADE,
                icon_emoji        VARCHAR,
                icon_url          VARCHAR,
                key               VARCHAR(10) NOT NULL,
                is_personal       BOOLEAN NOT NULL DEFAULT false,
                personal_owner_id UUID REFERENCES users(id) ON DELETE CASCADE,
                CONSTRAINT projects_key_format CHECK (key ~ '^[A-Z][A-Z0-9]{1,9}$')
            )
        "#,
        )
        .await?;
        conn.execute_unprepared(
            "CREATE UNIQUE INDEX projects_key_tenant_unique ON projects(tenant_id, key)",
        )
        .await?;
        conn.execute_unprepared("CREATE UNIQUE INDEX idx_projects_personal_owner ON projects(tenant_id, personal_owner_id) WHERE is_personal = true").await?;

        conn.execute_unprepared(
            r#"
            CREATE TABLE labels (
                id          UUID PRIMARY KEY,
                name        VARCHAR NOT NULL,
                description TEXT NOT NULL DEFAULT '',
                color       VARCHAR NOT NULL,
                icon_url    TEXT,
                project_id  UUID REFERENCES projects(id) ON DELETE CASCADE
            )
        "#,
        )
        .await?;
        conn.execute_unprepared(
            "CREATE UNIQUE INDEX labels_project_name_unique ON labels(project_id, name)",
        )
        .await?;

        conn.execute_unprepared(
            r#"
            CREATE TABLE personal_tokens (
                id                  UUID PRIMARY KEY,
                name                VARCHAR NOT NULL,
                token_last_four     VARCHAR NOT NULL,
                token_hash          VARCHAR NOT NULL,
                expires_at          TIMESTAMPTZ,
                last_used_at        TIMESTAMPTZ,
                revoked             BOOLEAN NOT NULL DEFAULT false,
                user_id             UUID NOT NULL REFERENCES users(id) ON DELETE CASCADE,
                scopes              JSONB NOT NULL DEFAULT '[]',
                tenant_id           UUID NOT NULL REFERENCES tenants(id) ON DELETE CASCADE,
                allowed_project_ids JSONB
            )
        "#,
        )
        .await?;
        conn.execute_unprepared(
            "CREATE INDEX idx_personal_tokens_token_hash ON personal_tokens(token_hash)",
        )
        .await?;

        // ── drive ────────────────────────────────────────────────────────────

        conn.execute_unprepared(
            r#"
            CREATE TABLE drive_folders (
                id         UUID PRIMARY KEY,
                name       VARCHAR NOT NULL,
                parent_id  UUID REFERENCES drive_folders(id) ON DELETE SET NULL,
                tenant_id  UUID NOT NULL REFERENCES tenants(id) ON DELETE CASCADE,
                project_id UUID REFERENCES projects(id) ON DELETE CASCADE,
                created_by UUID NOT NULL REFERENCES users(id) ON DELETE NO ACTION,
                created_at TIMESTAMPTZ NOT NULL DEFAULT now()
            )
        "#,
        )
        .await?;

        conn.execute_unprepared(
            r#"
            CREATE TABLE drive_files (
                id           UUID PRIMARY KEY,
                name         VARCHAR NOT NULL,
                size         BIGINT NOT NULL,
                mime_type    VARCHAR NOT NULL,
                storage_type VARCHAR(16) NOT NULL,
                storage_key  VARCHAR NOT NULL,
                tenant_id    UUID NOT NULL REFERENCES tenants(id) ON DELETE CASCADE,
                project_id   UUID REFERENCES projects(id) ON DELETE CASCADE,
                uploader_id  UUID NOT NULL REFERENCES users(id) ON DELETE NO ACTION,
                folder_id    UUID REFERENCES drive_folders(id) ON DELETE SET NULL,
                created_at   TIMESTAMPTZ NOT NULL DEFAULT now(),
                updated_at   TIMESTAMPTZ NOT NULL DEFAULT now(),
                CONSTRAINT drive_files_project_folder_check
                    CHECK (project_id IS NULL OR folder_id IS NOT NULL)
            )
        "#,
        )
        .await?;

        conn.execute_unprepared(
            r#"
            CREATE TABLE drive_folder_shares (
                id                  UUID PRIMARY KEY,
                folder_id           UUID NOT NULL REFERENCES drive_folders(id) ON DELETE CASCADE,
                shared_with_user_id UUID REFERENCES users(id) ON DELETE CASCADE,
                share_token         VARCHAR UNIQUE,
                permission          VARCHAR(16) NOT NULL,
                created_by          UUID NOT NULL REFERENCES users(id) ON DELETE NO ACTION,
                expires_at          TIMESTAMPTZ,
                created_at          TIMESTAMPTZ NOT NULL DEFAULT now(),
                CONSTRAINT drive_folder_shares_xor_check CHECK (
                    (shared_with_user_id IS NOT NULL)::int + (share_token IS NOT NULL)::int = 1
                )
            )
        "#,
        )
        .await?;

        // ── auth ─────────────────────────────────────────────────────────────

        conn.execute_unprepared(
            r#"
            CREATE TABLE oauth_connections (
                id                UUID PRIMARY KEY,
                user_id           UUID NOT NULL REFERENCES users(id) ON DELETE CASCADE,
                provider          VARCHAR NOT NULL,
                provider_user_id  VARCHAR NOT NULL,
                provider_email    VARCHAR,
                instance_url      VARCHAR,
                access_token_enc  TEXT,
                refresh_token_enc TEXT,
                token_expires_at  TIMESTAMPTZ,
                created_at        TIMESTAMPTZ NOT NULL DEFAULT now(),
                updated_at        TIMESTAMPTZ NOT NULL DEFAULT now(),
                UNIQUE NULLS NOT DISTINCT (provider, provider_user_id, instance_url)
            )
        "#,
        )
        .await?;
        conn.execute_unprepared(
            "CREATE INDEX idx_oauth_connections_user ON oauth_connections(user_id)",
        )
        .await?;

        conn.execute_unprepared(
            r#"
            CREATE TABLE passkeys (
                id            UUID PRIMARY KEY,
                user_id       UUID NOT NULL REFERENCES users(id) ON DELETE CASCADE,
                credential_id BYTEA NOT NULL UNIQUE,
                public_key    BYTEA NOT NULL,
                aaguid        BYTEA,
                sign_count    BIGINT NOT NULL DEFAULT 0,
                name          VARCHAR(255) NOT NULL,
                last_used_at  TIMESTAMPTZ,
                created_at    TIMESTAMPTZ NOT NULL DEFAULT now()
            )
        "#,
        )
        .await?;
        conn.execute_unprepared("CREATE INDEX idx_passkeys_user ON passkeys(user_id)")
            .await?;

        conn.execute_unprepared(
            r#"
            CREATE TABLE totp_credentials (
                user_id     UUID PRIMARY KEY REFERENCES users(id) ON DELETE CASCADE,
                secret_enc  TEXT NOT NULL,
                is_verified BOOLEAN NOT NULL DEFAULT false,
                created_at  TIMESTAMPTZ NOT NULL DEFAULT now()
            )
        "#,
        )
        .await?;

        conn.execute_unprepared(
            r#"
            CREATE TABLE recovery_codes (
                id         UUID PRIMARY KEY,
                user_id    UUID NOT NULL REFERENCES users(id) ON DELETE CASCADE,
                code_hash  VARCHAR NOT NULL,
                used_at    TIMESTAMPTZ,
                created_at TIMESTAMPTZ NOT NULL DEFAULT now()
            )
        "#,
        )
        .await?;
        conn.execute_unprepared("CREATE INDEX idx_recovery_codes_user ON recovery_codes(user_id)")
            .await?;

        // ── project structure ─────────────────────────────────────────────────

        conn.execute_unprepared(
            r#"
            CREATE TABLE project_members (
                id         UUID PRIMARY KEY,
                project_id UUID NOT NULL REFERENCES projects(id) ON DELETE CASCADE,
                user_id    UUID NOT NULL REFERENCES users(id) ON DELETE CASCADE,
                role       VARCHAR NOT NULL,
                UNIQUE (project_id, user_id),
                CONSTRAINT project_members_role_check CHECK (role IN ('Admin', 'Member', 'Viewer'))
            )
        "#,
        )
        .await?;

        conn.execute_unprepared(
            r#"
            CREATE TABLE project_statuses (
                id           UUID PRIMARY KEY,
                project_id   UUID NOT NULL REFERENCES projects(id) ON DELETE CASCADE,
                name         VARCHAR(100) NOT NULL,
                color        VARCHAR(7) NOT NULL,
                position     SMALLINT NOT NULL,
                is_default   BOOLEAN NOT NULL DEFAULT false,
                is_done_state BOOLEAN NOT NULL DEFAULT false,
                created_at   TIMESTAMPTZ NOT NULL DEFAULT now(),
                UNIQUE (project_id, name)
            )
        "#,
        )
        .await?;

        conn.execute_unprepared(
            r#"
            CREATE TABLE project_task_counters (
                project_id UUID PRIMARY KEY REFERENCES projects(id) ON DELETE CASCADE,
                last_seq   INTEGER NOT NULL DEFAULT 0
            )
        "#,
        )
        .await?;

        conn.execute_unprepared(
            r#"
            CREATE TABLE milestones (
                id          UUID PRIMARY KEY,
                project_id  UUID NOT NULL REFERENCES projects(id) ON DELETE CASCADE,
                name        VARCHAR(255) NOT NULL,
                description TEXT,
                due_date    DATE NOT NULL,
                created_by  UUID NOT NULL REFERENCES users(id) ON DELETE NO ACTION,
                created_at  TIMESTAMPTZ NOT NULL DEFAULT now(),
                updated_at  TIMESTAMPTZ NOT NULL DEFAULT now()
            )
        "#,
        )
        .await?;

        conn.execute_unprepared(r#"
            CREATE TABLE sprints (
                id         UUID PRIMARY KEY,
                project_id UUID NOT NULL REFERENCES projects(id) ON DELETE CASCADE,
                name       VARCHAR(255) NOT NULL,
                goal       TEXT,
                start_date DATE NOT NULL,
                end_date   DATE NOT NULL,
                status     VARCHAR NOT NULL DEFAULT 'planning',
                created_by UUID NOT NULL REFERENCES users(id) ON DELETE NO ACTION,
                created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
                updated_at TIMESTAMPTZ NOT NULL DEFAULT now(),
                CHECK (start_date <= end_date),
                CONSTRAINT sprints_status_check CHECK (status IN ('planning', 'active', 'completed'))
            )
        "#).await?;
        conn.execute_unprepared("CREATE INDEX idx_sprints_project ON sprints(project_id)")
            .await?;
        conn.execute_unprepared("CREATE UNIQUE INDEX idx_sprints_active_per_project ON sprints(project_id) WHERE status = 'active'").await?;

        conn.execute_unprepared(r#"
            CREATE TABLE project_custom_fields (
                id          UUID PRIMARY KEY,
                project_id  UUID NOT NULL REFERENCES projects(id) ON DELETE CASCADE,
                name        VARCHAR(100) NOT NULL,
                field_type  VARCHAR NOT NULL CHECK (field_type IN ('text', 'number', 'select', 'date', 'url', 'checkbox')),
                options     JSONB,
                is_required BOOLEAN NOT NULL DEFAULT false,
                position    SMALLINT NOT NULL,
                created_at  TIMESTAMPTZ NOT NULL DEFAULT now(),
                UNIQUE (project_id, name)
            )
        "#).await?;
        conn.execute_unprepared("CREATE INDEX idx_project_custom_fields_project_position ON project_custom_fields(project_id, position)").await?;

        conn.execute_unprepared(
            r#"
            CREATE TABLE project_task_views (
                id         UUID PRIMARY KEY,
                project_id UUID NOT NULL REFERENCES projects(id) ON DELETE CASCADE,
                created_by UUID NOT NULL REFERENCES users(id) ON DELETE CASCADE,
                name       VARCHAR(100) NOT NULL,
                is_shared  BOOLEAN NOT NULL DEFAULT false,
                filters    JSONB NOT NULL DEFAULT '{}',
                sort       JSONB NOT NULL DEFAULT '{}',
                view_type  VARCHAR NOT NULL DEFAULT 'list',
                created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
                updated_at TIMESTAMPTZ NOT NULL DEFAULT now()
            )
        "#,
        )
        .await?;
        conn.execute_unprepared(
            "CREATE INDEX idx_project_task_views_project ON project_task_views(project_id)",
        )
        .await?;
        conn.execute_unprepared("CREATE INDEX idx_project_task_views_project_created_by ON project_task_views(project_id, created_by)").await?;

        // ── tasks ─────────────────────────────────────────────────────────────

        conn.execute_unprepared(r#"
            CREATE TABLE tasks (
                id                UUID PRIMARY KEY,
                project_id        UUID NOT NULL REFERENCES projects(id) ON DELETE CASCADE,
                seq_id            INTEGER NOT NULL,
                title             VARCHAR(255) NOT NULL,
                description       TEXT,
                status_id         UUID NOT NULL REFERENCES project_statuses(id) ON DELETE NO ACTION,
                priority          VARCHAR NOT NULL DEFAULT 'medium',
                progress_pct      SMALLINT NOT NULL DEFAULT 0 CHECK (progress_pct BETWEEN 0 AND 100),
                parent_task_id    UUID REFERENCES tasks(id) ON DELETE SET NULL,
                milestone_id      UUID REFERENCES milestones(id) ON DELETE SET NULL,
                soft_deadline     TIMESTAMPTZ,
                hard_deadline     TIMESTAMPTZ,
                estimated_minutes INTEGER CHECK (estimated_minutes > 0),
                is_archived       BOOLEAN NOT NULL DEFAULT false,
                created_by        UUID NOT NULL REFERENCES users(id) ON DELETE NO ACTION,
                created_at        TIMESTAMPTZ NOT NULL DEFAULT now(),
                updated_at        TIMESTAMPTZ NOT NULL DEFAULT now(),
                deleted_at        TIMESTAMPTZ,
                sprint_id         UUID REFERENCES sprints(id) ON DELETE SET NULL,
                completed_at      TIMESTAMPTZ,
                UNIQUE (project_id, seq_id),
                CONSTRAINT soft_before_hard CHECK (
                    soft_deadline IS NULL OR hard_deadline IS NULL OR soft_deadline <= hard_deadline
                )
            )
        "#).await?;
        conn.execute_unprepared(
            "CREATE INDEX idx_tasks_project ON tasks(project_id) WHERE deleted_at IS NULL",
        )
        .await?;
        conn.execute_unprepared("CREATE INDEX idx_tasks_status ON tasks(status_id)")
            .await?;
        conn.execute_unprepared("CREATE INDEX idx_tasks_parent ON tasks(parent_task_id) WHERE parent_task_id IS NOT NULL").await?;
        conn.execute_unprepared(
            "CREATE INDEX idx_tasks_sprint ON tasks(sprint_id) WHERE sprint_id IS NOT NULL",
        )
        .await?;
        // 全文検索: USE_PG_BIGM=true なら pg_bigm の trigram GIN を、そうでなければ
        // 生成列 search_vector + GIN を作成する（旧 m20260610_create_task_extensions と同じ分岐）。
        if use_pg_bigm() {
            conn.execute_unprepared(
                r#"
                CREATE INDEX idx_tasks_title_bigm ON tasks USING GIN(title gin_bigm_ops);
                CREATE INDEX idx_tasks_description_bigm ON tasks USING GIN(description gin_bigm_ops)
            "#,
            )
            .await?;
        } else {
            conn.execute_unprepared(
                r#"
                ALTER TABLE tasks
                    ADD COLUMN search_vector TSVECTOR
                    GENERATED ALWAYS AS (
                        to_tsvector('pg_catalog.simple',
                            coalesce(title, '') || ' ' || coalesce(description, ''))
                    ) STORED;
                CREATE INDEX idx_tasks_search_vector ON tasks USING GIN(search_vector)
            "#,
            )
            .await?;
        }

        conn.execute_unprepared(
            r#"
            CREATE TABLE task_assignees (
                id          UUID PRIMARY KEY,
                task_id     UUID NOT NULL REFERENCES tasks(id) ON DELETE CASCADE,
                user_id     UUID NOT NULL REFERENCES users(id) ON DELETE CASCADE,
                role        VARCHAR NOT NULL DEFAULT 'secondary',
                assigned_at TIMESTAMPTZ NOT NULL DEFAULT now()
            )
        "#,
        )
        .await?;
        conn.execute_unprepared(
            "CREATE UNIQUE INDEX uq_task_assignees_task_user ON task_assignees(task_id, user_id)",
        )
        .await?;

        conn.execute_unprepared(
            r#"
            CREATE TABLE task_relations (
                id              UUID PRIMARY KEY,
                blocker_task_id UUID NOT NULL REFERENCES tasks(id) ON DELETE CASCADE,
                blocked_task_id UUID NOT NULL REFERENCES tasks(id) ON DELETE CASCADE,
                created_at      TIMESTAMPTZ NOT NULL DEFAULT now(),
                UNIQUE (blocker_task_id, blocked_task_id),
                CHECK (blocker_task_id <> blocked_task_id)
            )
        "#,
        )
        .await?;
        conn.execute_unprepared(
            "CREATE INDEX idx_task_relations_blocked_task_id ON task_relations(blocked_task_id)",
        )
        .await?;

        conn.execute_unprepared(
            r#"
            CREATE TABLE task_labels (
                task_id  UUID NOT NULL REFERENCES tasks(id) ON DELETE CASCADE,
                label_id UUID NOT NULL REFERENCES labels(id) ON DELETE CASCADE,
                PRIMARY KEY (task_id, label_id)
            )
        "#,
        )
        .await?;
        conn.execute_unprepared("CREATE INDEX idx_task_labels_label_id ON task_labels(label_id)")
            .await?;

        conn.execute_unprepared(
            r#"
            CREATE TABLE task_comments (
                id                UUID PRIMARY KEY,
                task_id           UUID NOT NULL REFERENCES tasks(id) ON DELETE CASCADE,
                user_id           UUID NOT NULL REFERENCES users(id) ON DELETE CASCADE,
                body              TEXT NOT NULL,
                parent_comment_id UUID REFERENCES task_comments(id) ON DELETE SET NULL,
                created_at        TIMESTAMPTZ NOT NULL DEFAULT now(),
                updated_at        TIMESTAMPTZ NOT NULL DEFAULT now(),
                deleted_at        TIMESTAMPTZ
            )
        "#,
        )
        .await?;
        conn.execute_unprepared("CREATE INDEX idx_comments_task ON task_comments(task_id, created_at) WHERE deleted_at IS NULL").await?;

        conn.execute_unprepared(
            r#"
            CREATE TABLE task_activities (
                id         UUID PRIMARY KEY,
                task_id    UUID NOT NULL REFERENCES tasks(id) ON DELETE CASCADE,
                user_id    UUID REFERENCES users(id) ON DELETE SET NULL,
                event_type VARCHAR NOT NULL,
                payload    JSONB NOT NULL,
                created_at TIMESTAMPTZ NOT NULL DEFAULT now()
            )
        "#,
        )
        .await?;
        conn.execute_unprepared(
            "CREATE INDEX idx_activities_task ON task_activities(task_id, created_at DESC)",
        )
        .await?;

        conn.execute_unprepared(
            r#"
            CREATE TABLE task_custom_field_values (
                task_id  UUID NOT NULL REFERENCES tasks(id) ON DELETE CASCADE,
                field_id UUID NOT NULL REFERENCES project_custom_fields(id) ON DELETE CASCADE,
                value    TEXT,
                PRIMARY KEY (task_id, field_id)
            )
        "#,
        )
        .await?;

        conn.execute_unprepared(
            r#"
            CREATE TABLE task_attachments (
                id            UUID PRIMARY KEY,
                task_id       UUID NOT NULL REFERENCES tasks(id) ON DELETE CASCADE,
                drive_file_id UUID NOT NULL REFERENCES drive_files(id) ON DELETE CASCADE,
                created_by    UUID NOT NULL REFERENCES users(id) ON DELETE CASCADE,
                created_at    TIMESTAMPTZ NOT NULL DEFAULT now(),
                UNIQUE (task_id, drive_file_id)
            )
        "#,
        )
        .await?;
        conn.execute_unprepared(
            "CREATE INDEX idx_task_attachments_task ON task_attachments(task_id)",
        )
        .await?;

        conn.execute_unprepared(
            r#"
            CREATE TABLE task_watchers (
                task_id    UUID NOT NULL REFERENCES tasks(id) ON DELETE CASCADE,
                user_id    UUID NOT NULL REFERENCES users(id) ON DELETE CASCADE,
                created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
                PRIMARY KEY (task_id, user_id)
            )
        "#,
        )
        .await?;

        // ── time tracking ─────────────────────────────────────────────────────

        conn.execute_unprepared(
            r#"
            CREATE TABLE task_timers (
                task_id    UUID NOT NULL REFERENCES tasks(id) ON DELETE CASCADE,
                user_id    UUID NOT NULL REFERENCES users(id) ON DELETE CASCADE,
                started_at TIMESTAMPTZ NOT NULL DEFAULT now(),
                PRIMARY KEY (task_id, user_id)
            )
        "#,
        )
        .await?;

        conn.execute_unprepared(
            r#"
            CREATE TABLE time_logs (
                id             UUID PRIMARY KEY,
                task_id        UUID NOT NULL REFERENCES tasks(id) ON DELETE CASCADE,
                user_id        UUID NOT NULL REFERENCES users(id) ON DELETE CASCADE,
                logged_minutes INTEGER NOT NULL CHECK (logged_minutes > 0),
                logged_at      DATE NOT NULL,
                note           TEXT,
                created_at     TIMESTAMPTZ NOT NULL DEFAULT now()
            )
        "#,
        )
        .await?;
        conn.execute_unprepared("CREATE INDEX idx_time_logs_task ON time_logs(task_id)")
            .await?;
        conn.execute_unprepared(
            "CREATE INDEX idx_time_logs_user_date ON time_logs(user_id, logged_at)",
        )
        .await?;

        // ── notifications ─────────────────────────────────────────────────────

        conn.execute_unprepared(
            r#"
            CREATE TABLE notifications (
                id                UUID PRIMARY KEY,
                user_id           UUID NOT NULL REFERENCES users(id) ON DELETE CASCADE,
                task_id           UUID REFERENCES tasks(id) ON DELETE CASCADE,
                notification_type VARCHAR NOT NULL,
                payload           JSONB NOT NULL,
                read_at           TIMESTAMPTZ,
                created_at        TIMESTAMPTZ NOT NULL DEFAULT now()
            )
        "#,
        )
        .await?;
        conn.execute_unprepared("CREATE INDEX idx_notifications_user_unread ON notifications(user_id, created_at DESC) WHERE read_at IS NULL").await?;

        conn.execute_unprepared(r#"
            CREATE TABLE notification_settings (
                user_id       UUID NOT NULL REFERENCES users(id) ON DELETE CASCADE,
                project_id    UUID NOT NULL REFERENCES projects(id) ON DELETE CASCADE,
                email_events  VARCHAR[] NOT NULL DEFAULT '{}',
                in_app_events VARCHAR[] NOT NULL DEFAULT '{assigned,mentioned,status_changed,deadline_soon,comment_added,pr_merged}',
                PRIMARY KEY (user_id, project_id)
            )
        "#).await?;

        // ── audit & system ────────────────────────────────────────────────────

        conn.execute_unprepared(
            r#"
            CREATE TABLE audit_logs (
                id            UUID PRIMARY KEY,
                actor_id      UUID REFERENCES users(id) ON DELETE SET NULL,
                actor_type    VARCHAR NOT NULL,
                action        VARCHAR NOT NULL,
                resource_type VARCHAR NOT NULL,
                resource_id   VARCHAR NOT NULL,
                tenant_id     UUID REFERENCES tenants(id) ON DELETE SET NULL,
                metadata      JSONB,
                ip_address    VARCHAR(45),
                user_agent    VARCHAR,
                created_at    TIMESTAMPTZ NOT NULL DEFAULT now()
            )
        "#,
        )
        .await?;
        conn.execute_unprepared("CREATE INDEX idx_audit_logs_actor_id ON audit_logs(actor_id)")
            .await?;
        conn.execute_unprepared("CREATE INDEX idx_audit_logs_action ON audit_logs(action)")
            .await?;
        conn.execute_unprepared(
            "CREATE INDEX idx_audit_logs_resource ON audit_logs(resource_type, resource_id)",
        )
        .await?;
        conn.execute_unprepared("CREATE INDEX idx_audit_logs_tenant_id ON audit_logs(tenant_id)")
            .await?;
        conn.execute_unprepared(
            "CREATE INDEX idx_audit_logs_created_at ON audit_logs(created_at DESC)",
        )
        .await?;
        // 監査ログはアプリロールに対して追記専用（改竄耐性）。app_role は外部プロビジョニング前提で
        // 存在しない環境もあるため、ロールが存在する場合のみ REVOKE する。
        conn.execute_unprepared(
            r#"
            DO $$ BEGIN
                IF EXISTS (SELECT 1 FROM pg_roles WHERE rolname = 'app_role') THEN
                    REVOKE UPDATE, DELETE ON audit_logs FROM app_role;
                END IF;
            END $$;
        "#,
        )
        .await?;

        conn.execute_unprepared(r#"
            CREATE TABLE system_settings (
                singleton                  BOOLEAN PRIMARY KEY DEFAULT true CHECK (singleton = true),
                user_registration_enabled  BOOLEAN NOT NULL DEFAULT true,
                drive_default_quota_mb     BIGINT NOT NULL DEFAULT 10240,
                updated_at                 TIMESTAMPTZ NOT NULL DEFAULT now(),
                drive_system_max_quota_mb  BIGINT NOT NULL DEFAULT 102400
            )
        "#).await?;
        conn.execute_unprepared(
            "INSERT INTO system_settings DEFAULT VALUES ON CONFLICT DO NOTHING",
        )
        .await?;

        // ── integrations ──────────────────────────────────────────────────────

        conn.execute_unprepared(
            r#"
            CREATE TABLE github_integrations (
                id               UUID PRIMARY KEY,
                project_id       UUID NOT NULL UNIQUE REFERENCES projects(id) ON DELETE CASCADE,
                installation_id  BIGINT NOT NULL,
                repo_owner       VARCHAR NOT NULL,
                repo_name        VARCHAR NOT NULL,
                access_token_enc TEXT NOT NULL,
                token_expires_at TIMESTAMPTZ NOT NULL,
                created_by       UUID NOT NULL REFERENCES users(id) ON DELETE NO ACTION,
                created_at       TIMESTAMPTZ NOT NULL DEFAULT now()
            )
        "#,
        )
        .await?;

        Ok(())
    }

    async fn down(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        let conn = manager.get_connection();
        let tables = [
            "github_integrations",
            "system_settings",
            "audit_logs",
            "notification_settings",
            "notifications",
            "task_timers",
            "time_logs",
            "task_watchers",
            "task_attachments",
            "task_custom_field_values",
            "task_activities",
            "task_comments",
            "task_labels",
            "task_relations",
            "task_assignees",
            "tasks",
            "project_task_views",
            "project_custom_fields",
            "sprints",
            "milestones",
            "project_task_counters",
            "project_statuses",
            "project_members",
            "recovery_codes",
            "totp_credentials",
            "passkeys",
            "oauth_connections",
            "drive_folder_shares",
            "drive_files",
            "drive_folders",
            "personal_tokens",
            "labels",
            "projects",
            "tenants",
            "users",
        ];
        for table in tables {
            conn.execute_unprepared(&format!("DROP TABLE IF EXISTS {table} CASCADE"))
                .await?;
        }
        Ok(())
    }
}
