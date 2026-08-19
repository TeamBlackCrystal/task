import { Command } from "commander";
import { getClient } from "../api/client";
import {
  configPath,
  loadConfigFile,
  saveConfigFile,
} from "../config/store";
import { getOutputOptions } from "../utils/command";
import { print } from "../utils/output";
import { unwrapApiResult } from "../utils/errors";

export function registerAuthCommands(program: Command): void {
  const auth = program.command("auth").description("Authentication commands");

  auth
    .command("whoami")
    .description("Show current user")
    .action(async (_opts, cmd) => {
      const opts = getOutputOptions(cmd);
      const client = getClient();
      const result = await client.GET("/v1/auth/me");
      print(unwrapApiResult(result), opts);
    });

  auth
    .command("token")
    .description("Save personal access token to config")
    .argument("[token]", "Token value (omit to read from stdin)")
    .action(async (tokenArg?: string) => {
      let token = tokenArg?.trim();
      if (!token) {
        token = await readStdin();
      }
      if (!token) {
        throw new Error("Token is required");
      }
      const config = loadConfigFile();
      config.token = token;
      saveConfigFile(config);
      console.log(`Token saved to ${configPath()}`);
    });

  auth
    .command("logout")
    .description("Remove token from local config")
    .action(() => {
      const config = loadConfigFile();
      delete config.token;
      saveConfigFile(config);
      console.log(`Token removed from ${configPath()}`);
    });
}

async function readStdin(): Promise<string> {
  if (process.stdin.isTTY) {
    return "";
  }
  const chunks: Buffer[] = [];
  for await (const chunk of process.stdin) {
    chunks.push(Buffer.from(chunk));
  }
  return Buffer.concat(chunks).toString("utf8").trim();
}
