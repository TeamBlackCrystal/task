import { Command } from "commander";
import { getClient, getTenantId } from "../api/client";
import { getOutputOptions } from "../utils/command";
import type { OutputOptions } from "../utils/output";
import { print } from "../utils/output";
import { unwrapApiResult } from "../utils/errors";
import { isUuid, resolveProject } from "../utils/projects";

type SprintCommandOptions = OutputOptions & {
  project?: string;
  status?: string;
  backlog?: boolean;
};

export function registerSprintsCommands(program: Command): void {
  const sprints = program.command("sprints").description("Sprint commands");

  sprints
    .command("list")
    .description("List sprints")
    .requiredOption("--project <key>", "Project key or UUID")
    .option("--status <status>", "Filter by sprint status")
    .action(async (opts: SprintCommandOptions, cmd) => {
      const output = getOutputOptions(cmd);
      const project = await resolveProject(opts.project!);
      const client = getClient();
      const tenantId = getTenantId();
      const result = await client.GET(
        "/v1/tenants/{tenant_id}/projects/{project_id}/sprints",
        {
          params: {
            path: { tenant_id: tenantId, project_id: project.id },
            query: opts.status ? { status: opts.status } : undefined,
          },
        },
      );
      print(unwrapApiResult(result), output);
    });

  sprints
    .command("show")
    .description("Show sprint details")
    .argument("<id>", "Sprint UUID")
    .requiredOption("--project <key>", "Project key or UUID")
    .action(async (id: string, opts: SprintCommandOptions, cmd) => {
      const output = getOutputOptions(cmd);
      const project = await resolveProject(opts.project!);
      const client = getClient();
      const tenantId = getTenantId();
      const result = await client.GET(
        "/v1/tenants/{tenant_id}/projects/{project_id}/sprints/{id}",
        {
          params: {
            path: {
              tenant_id: tenantId,
              project_id: project.id,
              id,
            },
          },
        },
      );
      print(unwrapApiResult(result), output);
    });

  sprints
    .command("start")
    .description("Start a sprint")
    .argument("<id>", "Sprint UUID")
    .requiredOption("--project <key>", "Project key or UUID")
    .action(async (id: string, opts: SprintCommandOptions, cmd) => {
      const output = getOutputOptions(cmd);
      const project = await resolveProject(opts.project!);
      const client = getClient();
      const tenantId = getTenantId();
      const result = await client.POST(
        "/v1/tenants/{tenant_id}/projects/{project_id}/sprints/{id}/start",
        {
          params: {
            path: {
              tenant_id: tenantId,
              project_id: project.id,
              id,
            },
          },
        },
      );
      print(unwrapApiResult(result), output);
    });

  sprints
    .command("complete")
    .description("Complete a sprint")
    .argument("<id>", "Sprint UUID")
    .requiredOption("--project <key>", "Project key or UUID")
    .option("--backlog", "Move incomplete tasks to backlog")
    .action(async (id: string, opts: SprintCommandOptions, cmd) => {
      const output = getOutputOptions(cmd);
      const project = await resolveProject(opts.project!);
      const client = getClient();
      const tenantId = getTenantId();
      const result = await client.POST(
        "/v1/tenants/{tenant_id}/projects/{project_id}/sprints/{id}/complete",
        {
          params: {
            path: {
              tenant_id: tenantId,
              project_id: project.id,
              id,
            },
          },
          body: {
            move_incomplete_to_backlog: Boolean(opts.backlog),
          },
        },
      );
      print(unwrapApiResult(result), output);
    });

  sprints
    .command("burndown")
    .description("Show sprint burndown data")
    .argument("<id>", "Sprint UUID or name")
    .requiredOption("--project <key>", "Project key or UUID")
    .action(async (id: string, opts: SprintCommandOptions, cmd) => {
      const output = getOutputOptions(cmd);
      const project = await resolveProject(opts.project!);
      const sprintId = await resolveSprintId(project.id, id);
      const client = getClient();
      const tenantId = getTenantId();
      const result = await client.GET(
        "/v1/tenants/{tenant_id}/projects/{project_id}/sprints/{id}",
        {
          params: {
            path: {
              tenant_id: tenantId,
              project_id: project.id,
              id: sprintId,
            },
          },
        },
      );
      const detail = unwrapApiResult(result);
      print(
        output.json
          ? { sprint: detail.sprint, burndown: detail.burndown }
          : detail.burndown,
        output,
      );
    });
}

async function resolveSprintId(projectId: string, idOrName: string): Promise<string> {
  if (isUuid(idOrName)) {
    return idOrName;
  }
  const client = getClient();
  const tenantId = getTenantId();
  const result = await client.GET(
    "/v1/tenants/{tenant_id}/projects/{project_id}/sprints",
    {
      params: { path: { tenant_id: tenantId, project_id: projectId } },
    },
  );
  const sprints = unwrapApiResult(result);
  const sprint = sprints.find(
    (item) => item.name.toLowerCase() === idOrName.toLowerCase(),
  );
  if (!sprint) {
    const { handleApiError } = await import("../utils/errors");
    handleApiError({ status: 404, message: `Sprint not found: ${idOrName}` });
  }
  return sprint!.id;
}
