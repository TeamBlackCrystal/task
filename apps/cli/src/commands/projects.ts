import { Command } from "commander";
import { getClient, getTenantId } from "../api/client";
import { getOutputOptions } from "../utils/command";
import { print } from "../utils/output";
import { unwrapApiResult } from "../utils/errors";
import { listProjects, resolveProject } from "../utils/projects";

export function registerProjectsCommands(program: Command): void {
  const projects = program
    .command("projects")
    .description("Project commands");

  projects
    .command("list")
    .description("List projects")
    .action(async (_opts, cmd) => {
      const opts = getOutputOptions(cmd);
      const data = await listProjects();
      print(opts.json ? data : { projects: data }, opts);
    });

  projects
    .command("show")
    .description("Show a project by key or UUID")
    .argument("<key>", "Project key or UUID")
    .action(async (key: string, _opts, cmd) => {
      const opts = getOutputOptions(cmd);
      const project = await resolveProject(key);
      print(project, opts);
    });
}
