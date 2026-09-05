import { describe, it, expect } from 'vitest';
import { computed } from 'vue';

import type { Tenant } from '@/stores/tenant';
import { resolveTenantIdFromRoute, useRouteAlignedTenantId } from '../useRouteAlignedTenantId';

const tenantA: Tenant = {
  id: '11111111-1111-1111-1111-111111111111',
  display_id: 'acme',
  name: 'Acme',
  description: '',
  icon_url: '',
  owner_id: '00000000-0000-0000-0000-000000000002',
  require_2fa: false,
  membership: 'Owner' as const,
};

const tenantB: Tenant = {
  id: '22222222-2222-2222-2222-222222222222',
  display_id: 'beta',
  name: 'Beta',
  description: '',
  icon_url: '',
  owner_id: '00000000-0000-0000-0000-000000000003',
  require_2fa: false,
  membership: 'Owner' as const,
};

describe('resolveTenantIdFromRoute', () => {
  it('resolves display_id to tenant UUID', () => {
    expect(resolveTenantIdFromRoute([tenantA, tenantB], 'beta')).toBe(tenantB.id);
  });

  it('resolves tenant UUID route param directly', () => {
    expect(resolveTenantIdFromRoute([tenantA, tenantB], tenantA.id)).toBe(tenantA.id);
  });

  it('returns null when route slug is empty', () => {
    expect(resolveTenantIdFromRoute([tenantA], '')).toBeNull();
  });

  it('returns null before route tenant is present in loaded tenants', () => {
    // Regression: persisted selectedTenantId may still point at tenant A while route is tenant B.
    expect(resolveTenantIdFromRoute([tenantA], 'beta')).toBeNull();
  });
});

describe('useRouteAlignedTenantId', () => {
  it('tracks route slug changes without using selectedTenantId fallback', () => {
    const tenants = computed(() => [tenantA, tenantB]);
    const routeSlug = computed(() => 'beta');
    const tenantId = useRouteAlignedTenantId(tenants, routeSlug);

    expect(tenantId.value).toBe(tenantB.id);
  });
});
