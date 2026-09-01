import { getClient, getTenantId } from "../api/client";
import type { Project } from "../api/paths";
import { unwrapApiResult } from "./errors";

const UUID_RE =
  /^[0-9a-f]{8}-[0-9a-f]{4}-[1-5][0-9a-f]{3}-[89ab][0-9a-f]{3}-[0-9a-f]{12}$/i;

export function isUuid(value: string): boolean {
  return UUID_RE.test(value);
}

export async function listProjects(): Promise<Project[]> {
  const client = getClient();
  const tenantId = getTenantId();
  const result = await client.GET("/v1/tenants/{tenant_id}/projects", {
    params: { path: { tenant_id: tenantId } },
  });
  return unwrapApiResult(result);
}

export async function resolveProject(keyOrId: string): Promise<Project> {
  const client = getClient();
  const tenantId = getTenantId();

  if (isUuid(keyOrId)) {
    const result = await client.GET("/v1/tenants/{tenant_id}/projects/{id}", {
      params: { path: { tenant_id: tenantId, id: keyOrId } },
    });
    return unwrapApiResult(result);
  }

  const projects = await listProjects();
  const project = projects.find(
    (item) => item.key.toUpperCase() === keyOrId.toUpperCase(),
  );
  if (!project) {
    const { handleApiError } = await import("./errors");
    handleApiError({ status: 404, message: `Project not found: ${keyOrId}` });
  }
  return project!;
}

export function parseTaskRef(
  ref: string,
): { projectKey: string; taskId: string } | { uuid: string } {
  if (isUuid(ref)) {
    return { uuid: ref };
  }
  const dash = ref.lastIndexOf("-");
  if (dash <= 0) {
    throw new Error(`Invalid task reference: ${ref}`);
  }
  const projectKey = ref.slice(0, dash);
  const seq = ref.slice(dash + 1);
  if (!/^\d+$/.test(seq)) {
    throw new Error(`Invalid task reference: ${ref}`);
  }
  return { projectKey, taskId: ref };
}
