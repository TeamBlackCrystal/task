import { Command } from "commander";
import { getClient, getTenantId } from "../api/client";
import type { TaskPriority } from "../api/paths";
import { getOutputOptions } from "../utils/command";
import type { OutputOptions } from "../utils/output";
import { print } from "../utils/output";
import { unwrapApiResult } from "../utils/errors";
import { parseTaskRef, resolveProject } from "../utils/projects";
import { findDoneStatusId, resolveStatusId } from "../utils/statuses";

type TaskCommandOptions = OutputOptions & {
  project?: string;
  title?: string;
  status?: string;
  priority?: string;
};

export function registerTasksCommands(program: Command): void {
  const tasks = program.command("tasks").description("Task commands");

  tasks
    .command("list")
    .description("List tasks in a project")
    .requiredOption("--project <key>", "Project key or UUID")
    .option("--priority <priority>", "Filter by priority")
    .action(async (opts: TaskCommandOptions, cmd) => {
      const output = getOutputOptions(cmd);
      const project = await resolveProject(opts.project!);
      const client = getClient();
      const tenantId = getTenantId();
      const result = await client.GET(
        "/v1/tenants/{tenant_id}/projects/{project_id}/tasks",
        {
          params: {
            path: { tenant_id: tenantId, project_id: project.id },
            query: opts.priority ? { priority: opts.priority } : undefined,
          },
        },
      );
      print(unwrapApiResult(result), output);
    });

  tasks
    .command("create")
    .description("Create a task")
    .requiredOption("--project <key>", "Project key or UUID")
    .requiredOption("--title <title>", "Task title")
    .option("--priority <priority>", "Task priority")
    .option("--status <status>", "Status name")
    .action(async (opts: TaskCommandOptions, cmd) => {
      const output = getOutputOptions(cmd);
      const project = await resolveProject(opts.project!);
      const body: {
        title: string;
        priority?: TaskPriority;
        status_id?: string;
      } = { title: opts.title! };
      if (opts.priority) {
        body.priority = opts.priority as TaskPriority;
      }
      if (opts.status) {
        body.status_id = await resolveStatusId(project.id, opts.status);
      }
      const client = getClient();
      const tenantId = getTenantId();
      const result = await client.POST(
        "/v1/tenants/{tenant_id}/projects/{project_id}/tasks",
        {
          params: {
            path: { tenant_id: tenantId, project_id: project.id },
          },
          body,
        },
      );
      print(unwrapApiResult(result), output);
    });

  tasks
    .command("show")
    .description("Show a task")
    .argument("<ref>", "Task ref (KEY-N or UUID)")
    .option("--project <key>", "Project key when using UUID")
    .action(async (ref: string, opts: TaskCommandOptions, cmd) => {
      const output = getOutputOptions(cmd);
      const { project, taskId } = await resolveTaskTarget(ref, opts.project);
      const client = getClient();
      const tenantId = getTenantId();
      const result = await client.GET(
        "/v1/tenants/{tenant_id}/projects/{project_id}/tasks/{id}",
        {
          params: {
            path: {
              tenant_id: tenantId,
              project_id: project.id,
              id: taskId,
            },
          },
        },
      );
      print(unwrapApiResult(result), output);
    });

  tasks
    .command("update")
    .description("Update a task")
    .argument("<ref>", "Task ref (KEY-N or UUID)")
    .option("--project <key>", "Project key when using UUID")
    .option("--title <title>", "New title")
    .option("--status <status>", "Status name")
    .option("--priority <priority>", "Priority")
    .action(async (ref: string, opts: TaskCommandOptions, cmd) => {
      const output = getOutputOptions(cmd);
      const { project, taskId } = await resolveTaskTarget(ref, opts.project);
      const body: {
        title?: string;
        status_id?: string;
        priority?: TaskPriority;
      } = {};
      if (opts.title) body.title = opts.title;
      if (opts.priority) body.priority = opts.priority as TaskPriority;
      if (opts.status) {
        body.status_id = await resolveStatusId(project.id, opts.status);
      }
      const client = getClient();
      const tenantId = getTenantId();
      const result = await client.PUT(
        "/v1/tenants/{tenant_id}/projects/{project_id}/tasks/{id}",
        {
          params: {
            path: {
              tenant_id: tenantId,
              project_id: project.id,
              id: taskId,
            },
          },
          body,
        },
      );
      print(unwrapApiResult(result), output);
    });

  tasks
    .command("complete")
    .description("Mark a task as done")
    .argument("<ref>", "Task ref (KEY-N or UUID)")
    .option("--project <key>", "Project key when using UUID")
    .action(async (ref: string, opts: TaskCommandOptions, cmd) => {
      const output = getOutputOptions(cmd);
      const { project, taskId } = await resolveTaskTarget(ref, opts.project);
      const statusId = await findDoneStatusId(project.id);
      const client = getClient();
      const tenantId = getTenantId();
      const result = await client.PUT(
        "/v1/tenants/{tenant_id}/projects/{project_id}/tasks/{id}",
        {
          params: {
            path: {
              tenant_id: tenantId,
              project_id: project.id,
              id: taskId,
            },
          },
          body: { status_id: statusId },
        },
      );
      print(unwrapApiResult(result), output);
    });

  tasks
    .command("comment")
    .description("Add a comment to a task")
    .argument("<ref>", "Task ref (KEY-N or UUID)")
    .argument("<body>", "Comment body")
    .option("--project <key>", "Project key when using UUID")
    .action(async (ref: string, body: string, opts: TaskCommandOptions, cmd) => {
      const output = getOutputOptions(cmd);
      const { project, taskId } = await resolveTaskTarget(ref, opts.project);
      const client = getClient();
      const tenantId = getTenantId();
      const result = await client.POST(
        "/v1/tenants/{tenant_id}/projects/{project_id}/tasks/{id}/comments",
        {
          params: {
            path: {
              tenant_id: tenantId,
              project_id: project.id,
              id: taskId,
            },
          },
          body: { body },
        },
      );
      print(unwrapApiResult(result), output);
    });

  tasks
    .command("delete")
    .description("Delete a task")
    .argument("<ref>", "Task ref (KEY-N or UUID)")
    .option("--project <key>", "Project key when using UUID")
    .action(async (ref: string, opts: TaskCommandOptions, cmd) => {
      const output = getOutputOptions(cmd);
      const { project, taskId } = await resolveTaskTarget(ref, opts.project);
      const client = getClient();
      const tenantId = getTenantId();
      const result = await client.DELETE(
        "/v1/tenants/{tenant_id}/projects/{project_id}/tasks/{id}",
        {
          params: {
            path: {
              tenant_id: tenantId,
              project_id: project.id,
              id: taskId,
            },
          },
        },
      );
      if (result.error) {
        unwrapApiResult(result);
      }
      print(output.json ? { deleted: taskId } : `Deleted ${taskId}`, output);
    });
}

async function resolveTaskTarget(ref: string, projectKey?: string) {
  const parsed = parseTaskRef(ref);
  if ("uuid" in parsed) {
    if (!projectKey) {
      throw new Error("--project is required when using a task UUID");
    }
    return { project: await resolveProject(projectKey), taskId: parsed.uuid };
  }
  const project = await resolveProject(parsed.projectKey);
  return { project, taskId: parsed.taskId };
}
