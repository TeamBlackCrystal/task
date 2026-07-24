import createFetchClient from 'openapi-fetch';
import { createClient } from '@koyori-app/openapi-vue-query';
import { computed, toValue, type MaybeRefOrGetter } from 'vue';
import { useQuery } from '@tanstack/vue-query';
import type { paths } from '@/generated/api';
import type { ProjectUuid, TenantUuid } from '@/lib/api-ids';

/** Session /me cache duration — auth source of truth refresh interval. */
export const AUTH_ME_STALE_TIME_MS = 60_000;
export const TASK_SEARCH_STALE_TIME_MS = 30_000;

export const LIST_PROJECTS_PATH = '/v1/tenants/{tenant_id}/projects' as const;
export const TASK_SEARCH_PATH =
  '/v1/tenants/{tenant_id}/projects/{project_id}/tasks/search' as const;

export const fetchClient = createFetchClient<paths>({
  baseUrl: import.meta.env.VITE_API_BASE ?? '/api',
  fetch: (req: Request) => globalThis.fetch(req),
  credentials: 'include',
});

/** Typed TanStack Vue Query helpers for task OpenAPI paths. */
export const apiClient = createClient<paths>(fetchClient);

export const meQueryOptions = () =>
  apiClient.queryOptions('get', '/v1/auth/me', undefined, {
    staleTime: AUTH_ME_STALE_TIME_MS,
    retry: false,
  });

export const projectLabelsQueryOptions = (
  tenantId: TenantUuid | null | undefined,
  projectId: ProjectUuid | null | undefined,
) =>
  apiClient.queryOptions('get', '/v1/tenants/{tenant_id}/projects/{project_id}/labels', {
    params: { path: { tenant_id: tenantId ?? '', project_id: projectId ?? '' } },
  });

export const projectsQueryOptions = (tenantId: TenantUuid | null | undefined) =>
  apiClient.queryOptions(
    'get',
    LIST_PROJECTS_PATH,
    { params: { path: { tenant_id: tenantId ?? '' } } },
    { staleTime: AUTH_ME_STALE_TIME_MS },
  );

export const taskSearchQueryOptions = (
  tenantId: string,
  projectId: string,
  query: string,
  pagination: { limit?: number; offset?: number } = {},
) =>
  apiClient.queryOptions(
    'get',
    TASK_SEARCH_PATH,
    {
      params: {
        path: { tenant_id: tenantId, project_id: projectId },
        query: { q: query, ...pagination },
      },
    },
    { staleTime: TASK_SEARCH_STALE_TIME_MS, retry: false },
  );

export function useProjectsQuery(tenantId: MaybeRefOrGetter<TenantUuid | null | undefined>) {
  const resolvedTenantId = computed(() => toValue(tenantId) ?? null);

  return useQuery(
    computed(() => {
      const id = resolvedTenantId.value;
      return {
        ...projectsQueryOptions(id),
        enabled: !!id,
      };
    }),
  );
}

export function useMeQuery(options?: { enabled?: MaybeRefOrGetter<boolean> }) {
  return apiClient.useQuery('get', '/v1/auth/me', undefined, {
    staleTime: AUTH_ME_STALE_TIME_MS,
    retry: false,
    ...(options?.enabled !== undefined ? { enabled: options.enabled } : {}),
  });
}

/** 有効な OAuth ログインプロバイダー一覧（backend 起動時に固定されるため長めにキャッシュ）。 */
export function useOAuthProvidersQuery() {
  return apiClient.useQuery('get', '/v1/auth/oauth/providers', undefined, {
    staleTime: Infinity,
    retry: false,
  });
}

export function useLoginMutation() {
  return apiClient.useMutation('post', '/v1/auth/login');
}

export function useRegisterMutation() {
  return apiClient.useMutation('post', '/v1/auth/register');
}

export function useLogoutMutation() {
  return apiClient.useMutation('post', '/v1/auth/logout');
}

export function useResendVerificationEmailMutation() {
  return apiClient.useMutation('post', '/v1/auth/resend-verification-email');
}

export function usePasswordResetRequestMutation() {
  return apiClient.useMutation('post', '/v1/auth/password-reset/request');
}

export function usePasswordResetVerifyMutation() {
  return apiClient.useMutation('post', '/v1/auth/password-reset/verify');
}

export function usePasswordResetCompleteMutation() {
  return apiClient.useMutation('post', '/v1/auth/password-reset/complete');
}

export function createTestApiClient(fetchImpl: (input: Request) => Promise<Response>) {
  return createClient<paths>(
    createFetchClient<paths>({
      baseUrl: 'http://test.local/api',
      fetch: fetchImpl,
    }),
  );
}
