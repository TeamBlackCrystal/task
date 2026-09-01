declare const client: { GET(path: string, options: unknown): unknown };
declare const tenantId: string;
declare const projectId: string;

client.GET('/v1/tenants/{tenant_id}/projects/{project_id}/statuses', {
  params: { path: { tenant_id: tenantId, project_id: projectId } },
});
