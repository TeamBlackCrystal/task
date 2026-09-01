import createClient, { type Client } from "openapi-fetch";
import { resolveRuntimeConfig } from "../config/store";
import type { ApiPaths } from "./paths";

export type TaskApiClient = Client<ApiPaths, `${string}/${string}`>;

let cachedClient: TaskApiClient | null = null;
let cachedTenantId: string | null = null;

export function getClient(): TaskApiClient {
  if (cachedClient) {
    return cachedClient;
  }
  const config = resolveRuntimeConfig();
  cachedClient = createClient<ApiPaths>({
    baseUrl: config.api_url,
    headers: {
      Authorization: `Bearer ${config.token}`,
      Accept: "application/json",
    },
  });
  return cachedClient;
}

export function getTenantId(): string {
  if (cachedTenantId) {
    return cachedTenantId;
  }
  cachedTenantId = resolveRuntimeConfig().tenant_id;
  return cachedTenantId;
}
