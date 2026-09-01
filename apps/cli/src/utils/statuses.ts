import { getClient, getTenantId } from "../api/client";
import type { ProjectStatus } from "../api/paths";
import { unwrapApiResult } from "./errors";

export async function listStatuses(
  projectId: string,
): Promise<ProjectStatus[]> {
  const client = getClient();
  const tenantId = getTenantId();
  const result = await client.GET(
    "/v1/tenants/{tenant_id}/projects/{project_id}/statuses",
    {
      params: { path: { tenant_id: tenantId, project_id: projectId } },
    },
  );
  return unwrapApiResult(result);
}

export async function resolveStatusId(
  projectId: string,
  statusName: string,
): Promise<string> {
  const statuses = await listStatuses(projectId);
  const match = statuses.find(
    (status) => status.name.toLowerCase() === statusName.toLowerCase(),
  );
  if (!match) {
    const { handleApiError } = await import("./errors");
    handleApiError({
      status: 404,
      message: `Status not found: ${statusName}`,
    });
  }
  return match!.id;
}

export async function findDoneStatusId(projectId: string): Promise<string> {
  const statuses = await listStatuses(projectId);
  const done =
    statuses.find((status) => status.is_done_state) ??
    statuses.find((status) => /done|complete/i.test(status.name));
  if (!done) {
    const { handleApiError } = await import("./errors");
    handleApiError({
      status: 404,
      message: "No done status found for project",
    });
  }
  return done!.id;
}
