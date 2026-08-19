import fs from "node:fs";
import os from "node:os";
import path from "node:path";
import yaml from "js-yaml";
import { exitWithConfigError } from "../utils/errors";

export type TaskConfig = {
  api_url?: string;
  token?: string;
  tenant_id?: string;
};

const CONFIG_DIR = path.join(os.homedir(), ".config", "task");
const CONFIG_PATH = path.join(CONFIG_DIR, "config.yaml");

export function configPath(): string {
  return CONFIG_PATH;
}

export function loadConfigFile(): TaskConfig {
  if (!fs.existsSync(CONFIG_PATH)) {
    return {};
  }
  const raw = fs.readFileSync(CONFIG_PATH, "utf8");
  const safeSchema = (
    yaml as typeof yaml & { DEFAULT_SAFE_SCHEMA: yaml.Schema }
  ).DEFAULT_SAFE_SCHEMA;
  return (yaml.load(raw, { schema: safeSchema }) as TaskConfig) ?? {};
}

export function saveConfigFile(config: TaskConfig): void {
  fs.mkdirSync(CONFIG_DIR, { recursive: true });
  fs.writeFileSync(CONFIG_PATH, yaml.dump(config), {
    encoding: "utf8",
    mode: 0o600,
  });
  // writeFileSync の mode は新規作成時しか適用されない（既存ファイル更新時は
  // 従来のパーミッションが残る）。トークンを含むため常に 0o600 を強制する。
  fs.chmodSync(CONFIG_PATH, 0o600);
}

export function resolveRuntimeConfig(): Required<
  Pick<TaskConfig, "api_url" | "token" | "tenant_id">
> {
  const file = loadConfigFile();
  const api_url = process.env.TASK_API_URL ?? file.api_url;
  const token = process.env.TASK_TOKEN ?? file.token;
  const tenant_id = process.env.TASK_TENANT ?? file.tenant_id;

  if (!api_url || !token || !tenant_id) {
    const missing = [
      !api_url ? "api_url (TASK_API_URL)" : null,
      !token ? "token (TASK_TOKEN)" : null,
      !tenant_id ? "tenant_id (TASK_TENANT)" : null,
    ].filter(Boolean);
    exitWithConfigError(
      `Missing required configuration: ${missing.join(", ")}. Set env vars or ${CONFIG_PATH}.`,
    );
  }

  return {
    api_url: api_url.replace(/\/$/, ""),
    token,
    tenant_id,
  };
}
