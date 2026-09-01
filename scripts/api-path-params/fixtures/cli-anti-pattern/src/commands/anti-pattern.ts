declare const client: { GET(path: string, options: unknown): unknown };
declare const tenantDisplayId: string;

client.GET('/v1/tenants/{tenant_id}/projects', {
  params: { path: { tenant_id: tenantDisplayId } },
});
