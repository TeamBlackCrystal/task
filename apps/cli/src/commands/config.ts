import { Command } from "commander";
import {
  configPath,
  loadConfigFile,
  saveConfigFile,
} from "../config/store";
import { getOutputOptions } from "../utils/command";
import { print } from "../utils/output";

const ALLOWED_KEYS = ["api_url", "token", "tenant_id"] as const;
type ConfigKey = (typeof ALLOWED_KEYS)[number];

export function registerConfigCommands(program: Command): void {
  const config = program
    .command("config")
    .description("Manage ~/.config/task/config.yaml");

  config
    .command("list")
    .description("List all config values")
    .action((_opts, cmd) => {
      const opts = getOutputOptions(cmd);
      print(loadConfigFile(), opts);
    });

  config
    .command("get")
    .description("Get a config value")
    .argument("<key>", "api_url | token | tenant_id")
    .action((key: string, _opts, cmd) => {
      const opts = getOutputOptions(cmd);
      const configKey = assertConfigKey(key);
      const value = loadConfigFile()[configKey];
      if (opts.json) {
        print({ key: configKey, value: value ?? null }, opts);
        return;
      }
      console.log(value ?? "");
    });

  config
    .command("set")
    .description("Set a config value")
    .argument("<key>", "api_url | token | tenant_id")
    .argument("<value>", "Value to store")
    .action((key: string, value: string) => {
      const configKey = assertConfigKey(key);
      const file = loadConfigFile();
      file[configKey] = value;
      saveConfigFile(file);
      console.log(`Set ${configKey} in ${configPath()}`);
    });

  config
    .command("unset")
    .description("Remove a config value")
    .argument("<key>", "api_url | token | tenant_id")
    .action((key: string) => {
      const configKey = assertConfigKey(key);
      const file = loadConfigFile();
      delete file[configKey];
      saveConfigFile(file);
      console.log(`Removed ${configKey} from ${configPath()}`);
    });
}

function assertConfigKey(key: string): ConfigKey {
  if ((ALLOWED_KEYS as readonly string[]).includes(key)) {
    return key as ConfigKey;
  }
  throw new Error(`Unknown config key: ${key}`);
}
