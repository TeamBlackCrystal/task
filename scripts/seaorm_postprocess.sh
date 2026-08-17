#!/usr/bin/env bash
# Re-apply hand-written enum / JSON column types onto pure sea-orm-cli output.
#
# sea-orm-cli emits varchar-backed columns as `String` and json columns as `Json`.
# We want the type-safe wrapper enums (DeriveActiveEnum) and ScopeList instead, so
# the application code can rely on the entity Model carrying the real domain types.
#
# This runs after `sea-orm-cli generate` (see seaorm_generate.sh). It is idempotent:
# re-running it on already-processed files is a no-op.
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
GEN="$ROOT/apps/backend/crates/entity/src/_generated"

# replace <file> <original-field-line> <typed-field-line>
replace() {
  local file="$GEN/$1"
  local from="$2"
  local to="$3"
  if [[ ! -f "$file" ]]; then
    return 0
  fi
  # Only rewrite the pure-output form; if already typed this is a no-op.
  perl -0pi -e "s/\Q$from\E/$to/" "$file"
}

# table file                    pure output field                  typed field
replace tasks.rs                "pub priority: String,"            "pub priority: super::super::tasks::TaskPriority,"
replace project_custom_fields.rs "pub field_type: String,"         "pub field_type: super::super::project_custom_fields::CustomFieldType,"
replace sprints.rs              "pub status: String,"              "pub status: super::super::sprints::SprintStatus,"
replace project_members.rs      "pub role: String,"                "pub role: super::super::project_members::ProjectRole,"
replace tenant_members.rs       "pub role: String,"                "pub role: super::super::tenant_members::TenantRole,"
replace drive_files.rs          "pub storage_type: String,"        "pub storage_type: super::super::drive_files::StorageType,"
replace drive_folder_shares.rs  "pub permission: String,"          "pub permission: super::super::drive_folder_shares::SharePermission,"
replace personal_tokens.rs      "pub scopes: Json,"                "pub scopes: super::super::scopes::ScopeList,"

# These entities provide a custom ActiveModelBehavior (CHECK-constraint validation)
# in their wrapper module, so drop the default impl emitted by sea-orm-cli to avoid
# a conflicting-impl error.
drop_default_active_model_behavior() {
  local file="$GEN/$1"
  [[ -f "$file" ]] || return 0
  perl -0pi -e 's/\nimpl ActiveModelBehavior for ActiveModel \{\}\r?\n//' "$file"
}
drop_default_active_model_behavior drive_files.rs
drop_default_active_model_behavior drive_folder_shares.rs

# sprints.project_id の単一列 unique は誤検出。実際の制約は partial unique
# "idx_sprints_active_per_project ON sprints(project_id) WHERE status = 'active'" で、
# project_id を単純 unique 化すると 1 プロジェクト 1 スプリントしか作れなくなる。除去する。
perl -0pi -e 's/[ \t]*#\[sea_orm\(unique\)\]\r?\n([ \t]*pub project_id: Uuid,)/$1/' "$GEN/sprints.rs"

echo "seaorm_postprocess: enum/json column types applied"
