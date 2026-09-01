declare const api: { GET(path: string, options: unknown): unknown };
declare const tenant: string;

api.GET('/v1/tenants/{tenant_id}/projects', {
  params: { path: { tenant_id: tenant } },
});
