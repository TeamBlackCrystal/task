import { createPinia, setActivePinia } from 'pinia';
import { beforeEach, describe, expect, it, vi } from 'vitest';
import { apiClient } from '@/lib/api';
import { useTenantStore, type Tenant } from '@/stores/tenant';

vi.mock('@/lib/api', () => ({
  apiClient: { GET: vi.fn(), POST: vi.fn() },
}));

const tenants: Tenant[] = [
  {
    id: 'tenant-1',
    display_id: 'alpha',
    name: 'Alpha',
    description: '',
    icon_url: '',
    owner_id: 'owner-1',
    require_2fa: false,
    membership: 'Owner' as const,
  },
  {
    id: 'tenant-2',
    display_id: 'beta',
    name: 'Beta',
    description: '',
    icon_url: '',
    owner_id: 'owner-1',
    require_2fa: false,
    membership: 'Owner' as const,
  },
];

describe('tenant store', () => {
  beforeEach(() => {
    setActivePinia(createPinia());
    vi.mocked(apiClient.GET).mockReset();
    vi.mocked(apiClient.POST).mockReset();
  });

  it('creates a tenant without refetching the list before navigation', async () => {
    vi.mocked(apiClient.POST).mockResolvedValue({
      data: tenants[1],
      response: new Response(null, { status: 201 }),
    });
    const store = useTenantStore();

    const tenant = await store.createTenant({ name: 'Beta', display_id: 'beta' });

    expect(apiClient.POST).toHaveBeenCalledWith('/v1/tenants', {
      body: { name: 'Beta', display_id: 'beta' },
    });
    expect(apiClient.GET).not.toHaveBeenCalled();
    expect(tenant).toEqual(tenants[1]);
  });

  it('surfaces a display id conflict based on the 409 response status', async () => {
    vi.mocked(apiClient.POST).mockResolvedValue({
      error: { message: 'backend wording may change' },
      response: new Response(null, { status: 409 }),
    });
    const store = useTenantStore();

    await expect(store.createTenant({ name: 'Alpha', display_id: 'alpha' })).rejects.toThrow(
      'この表示IDはすでに使用されています',
    );
  });

  it('does not treat an internal conflict message as a duplicate without a 409 status', async () => {
    vi.mocked(apiClient.POST).mockResolvedValue({
      error: { message: 'conflict' },
      response: new Response(null, { status: 500 }),
    });
    const store = useTenantStore();

    await expect(store.createTenant({ name: 'Alpha', display_id: 'alpha' })).rejects.toThrow(
      'テナントを作成できませんでした',
    );
  });

  it('loads /v1/tenants and selects the tenant from the route', async () => {
    vi.mocked(apiClient.GET).mockResolvedValue({ data: tenants, response: new Response() });
    const store = useTenantStore();

    await store.loadTenants('beta');

    expect(apiClient.GET).toHaveBeenCalledWith('/v1/tenants');
    expect(store.tenants).toEqual(tenants);
    expect(store.selectedTenant?.display_id).toBe('beta');
    expect(store.error).toBeNull();
  });

  it('does not fall back to another tenant when the route tenant does not exist', async () => {
    vi.mocked(apiClient.GET).mockResolvedValue({ data: tenants, response: new Response() });
    const store = useTenantStore();
    store.selectTenant(tenants[0]);

    await store.loadTenants('missing');

    expect(store.tenants).toEqual(tenants);
    expect(store.selectedTenantId).toBeNull();
    expect(store.selectedTenant).toBeNull();
  });

  it('does not issue another tenant request while one is already loading', async () => {
    let resolveRequest: ((value: { data: Tenant[]; response: Response }) => void) | undefined;
    vi.mocked(apiClient.GET).mockImplementation(
      () =>
        new Promise((resolve) => {
          resolveRequest = resolve;
        }),
    );
    const store = useTenantStore();

    const firstRequest = store.loadTenants('alpha');
    await store.loadTenants('beta');

    expect(apiClient.GET).toHaveBeenCalledTimes(1);
    resolveRequest?.({ data: tenants, response: new Response() });
    await firstRequest;
    expect(store.selectedTenant?.display_id).toBe('alpha');
  });

  it('uses the first tenant when there is no route or persisted selection', async () => {
    vi.mocked(apiClient.GET).mockResolvedValue({ data: tenants, response: new Response() });
    const store = useTenantStore();

    await store.loadTenants();

    expect(store.selectedTenantId).toBe('tenant-1');
  });

  it('represents an empty tenant membership without throwing', async () => {
    vi.mocked(apiClient.GET).mockResolvedValue({ data: [], response: new Response() });
    const store = useTenantStore();

    await store.loadTenants();

    expect(store.tenants).toEqual([]);
    expect(store.selectedTenantId).toBeNull();
    expect(store.error).toBeNull();
  });

  it('clears tenant context and exposes a fallback message on API failure', async () => {
    vi.mocked(apiClient.GET).mockResolvedValue({
      error: { message: 'unauthorized' },
      response: new Response(null, { status: 401 }),
    });
    const store = useTenantStore();

    await store.loadTenants('alpha');

    expect(store.tenants).toEqual([]);
    expect(store.selectedTenantId).toBeNull();
    expect(store.error).toBe('テナント一覧を取得できませんでした');
  });
});
