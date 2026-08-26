import { Command } from "commander";
import pkg from "../package.json";
import { registerAuthCommands } from "./commands/auth";
import { registerConfigCommands } from "./commands/config";
import { registerMyCommands } from "./commands/my";
import { registerProjectsCommands } from "./commands/projects";
import { registerReviewCommands } from "./commands/reviews";
import { registerSprintsCommands } from "./commands/sprints";
import { registerTasksCommands } from "./commands/tasks";
import { CliError } from "./utils/errors";

async function main(): Promise<void> {
  const program = new Command().allowExcessArguments();

  program
    .name("task")
    .version(pkg.version)
    .description("Task management CLI")
    .option("--json", "Output JSON", false);

  registerAuthCommands(program);
  registerConfigCommands(program);
  registerProjectsCommands(program);
  registerTasksCommands(program);
  registerMyCommands(program);
  registerSprintsCommands(program);
  registerReviewCommands(program);

  try {
    await program.parseAsync(process.argv);
  } catch (error) {
    if (error instanceof CliError) {
      console.error(error.message);
      process.exit(error.exitCode);
    }
    console.error(error instanceof Error ? error.message : String(error));
    process.exit(1);
  }
}

void main();
