import { existsSync, readFileSync } from 'node:fs';
import { resolve } from 'node:path';

export const DEFAULT_E2E_DATABASE_URL =
  'postgresql://test:test@localhost:5432/task_e2e';

const E2E_DIR = resolve(import.meta.dirname);
const REPO_ROOT = resolve(E2E_DIR, '..');

function readEnvFile(path: string): Record<string, string> {
  if (!existsSync(path)) {
    return {};
  }

  return Object.fromEntries(
    readFileSync(path, 'utf8')
      .split(/\r?\n/)
      .map((line) => line.trim())
      .filter((line) => line && !line.startsWith('#') && line.includes('='))
      .map((line) => {
        const index = line.indexOf('=');
        const key = line.slice(0, index).trim();
        const rawValue = line.slice(index + 1).trim();
        return [key, rawValue.replace(/^['"]|['"]$/g, '')];
      }),
  );
}

/** Single source of truth for e2e PostgreSQL connection strings. */
export function resolveE2eDatabaseUrl(): string {
  const e2eEnv = readEnvFile(resolve(E2E_DIR, '.env'));
  const rootEnv = readEnvFile(resolve(REPO_ROOT, '.env'));
  const backendEnv = readEnvFile(resolve(REPO_ROOT, 'apps/backend/.env'));

  return (
    process.env.E2E_DATABASE_URL ??
    process.env.DATABASE_URL ??
    e2eEnv.E2E_DATABASE_URL ??
    e2eEnv.DATABASE_URL ??
    rootEnv.E2E_DATABASE_URL ??
    rootEnv.DATABASE_URL ??
    backendEnv.E2E_DATABASE_URL ??
    backendEnv.DATABASE_URL ??
    backendEnv.database_url ??
    DEFAULT_E2E_DATABASE_URL
  );
}
