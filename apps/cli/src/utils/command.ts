import type { Command } from "commander";
import type { OutputOptions } from "./output";

export function getOutputOptions(command: Command): OutputOptions {
  let current: Command | null = command;
  while (current) {
    const opts = current.opts() as Partial<OutputOptions>;
    if (typeof opts.json === "boolean") {
      return { json: opts.json };
    }
    current = current.parent;
  }
  return { json: false };
}
