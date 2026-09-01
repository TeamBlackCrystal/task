declare const api: { GET(path: string, options: unknown): unknown };
declare const projectKey: { value: string };
declare const tenantId: { value: string };
declare const projectId: { value: string };

const displayHref = `/acme/${projectKey.value}`;
void displayHref;
api.GET('/v1/tenants/{tenant_id}/projects/{project_id}', {
  params: {
    path: {
      tenant_id: tenantId.value,
      project_id: projectId.value,
    },
  },
});
