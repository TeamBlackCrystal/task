#!/usr/bin/env bash
set -euo pipefail

root="${1:-src}"

if [[ ! -d "$root" ]]; then
  echo "API path-param gate: source directory not found: $root" >&2
  exit 2
fi

# API path parameters ending in `_id` must receive resolved IDs, not route-facing
# display IDs/keys/slugs. Tests, stories, and generated clients are intentionally
# excluded: this gate protects production API calls rather than mocks or fixtures.
# A raw route value hidden behind a resolved-looking name (for example, tenantId)
# is undetectable by this grep, so this does not prevent the actual #357 bug shape.
# It reliably catches the bare route-param shape from #361. Fully preventing #357
# requires a follow-up companion rule that constrains values at the routeParams source.
mapfile -d '' files < <(
  find "$root" -type f \( -name '*.ts' -o -name '*.tsx' -o -name '*.js' -o -name '*.jsx' -o -name '*.vue' \) \
    ! -path '*/__tests__/*' \
    ! -path '*/stories/*' \
    ! -path '*/generated/*' \
    ! -name '*.test.*' \
    ! -name '*.spec.*' \
    ! -name '*.stories.*' \
    -print0
)

if [[ ${#files[@]} -eq 0 ]]; then
  echo "API path-param gate: no production source files found under $root"
  exit 0
fi

# GNU grep's -z mode lets the expression cover formatted, multi-line object
# literals while [^}] confines it to one `path` object.
pattern='(?s)params\s*:\s*\{\s*path\s*:\s*\{[^}]*?\b[A-Za-z_$][A-Za-z0-9_$]*_id\s*:\s*(?:(?:pageContext\.)?routeParams\.[A-Za-z_$][A-Za-z0-9_$]*|[A-Za-z_$][A-Za-z0-9_$]*(?:DisplayId|displayId|Key|Slug)(?:\.value)?|\b(?:tenant|project)\b(?:\.value)?(?!\.\w))'

violations=0
for file in "${files[@]}"; do
  if grep -Pzqm1 "$pattern" "$file"; then
    echo "$file: unresolved route/display value passed to an API *_id path parameter" >&2
    violations=1
  fi
done

if [[ $violations -ne 0 ]]; then
  cat >&2 <<'EOF'
API path-param gate failed.
Resolve route display IDs/keys/slugs first (for example with useResolvedTenantId
or useResolvedProjectId), then pass tenantId/projectId to params.path.
EOF
  exit 1
fi

echo "API path-param gate passed for $root"
