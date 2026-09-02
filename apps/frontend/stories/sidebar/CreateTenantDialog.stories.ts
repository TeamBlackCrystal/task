import type { Meta, StoryObj } from '@storybook/vue3-vite';
import { createPinia, setActivePinia } from 'pinia';
import { expect, fn, userEvent, within } from 'storybook/test';
import CreateTenantDialog from '@/components/header/CreateTenantDialog.vue';
import { useTenantStore } from '@/stores/tenant';

const createTenant = fn(async () => ({ display_id: 'acme' }));

const render =
  (action: typeof createTenant = createTenant) =>
  () => ({
    components: { CreateTenantDialog },
    setup() {
      setActivePinia(createPinia());
      const store = useTenantStore();
      store.createTenant = action as unknown as typeof store.createTenant;
      return {};
    },
    template: '<CreateTenantDialog :open="true" @update:open="() => {}" />',
  });

const meta = {
  title: 'Sidebar/CreateTenantDialog',
  component: CreateTenantDialog,
  parameters: { layout: 'fullscreen' },
} satisfies Meta<typeof CreateTenantDialog>;

export default meta;
type Story = StoryObj;

export const Initial: Story = { render: render() };

export const Filled: Story = {
  render: render(),
  play: async ({ canvasElement }) => {
    const page = within(canvasElement.ownerDocument.body);
    await userEvent.type(page.getByLabelText('名前'), 'Acme Team');
    await expect(page.getByLabelText('表示ID')).toHaveValue('acme-team');
    await userEvent.type(page.getByLabelText('説明（任意）'), 'Product team');
  },
};

export const Submitting: Story = {
  render: render(fn(() => new Promise(() => {})) as typeof createTenant),
  play: async ({ canvasElement }) => {
    const page = within(canvasElement.ownerDocument.body);
    await userEvent.type(page.getByLabelText('名前'), 'Acme');
    await userEvent.click(page.getByRole('button', { name: '作成' }));
    await expect(page.getByRole('button', { name: '作成中…' })).toBeDisabled();
  },
};

export const ConflictError: Story = {
  render: render(
    fn(async () => {
      throw new Error('この表示IDはすでに使用されています');
    }) as typeof createTenant,
  ),
  play: async ({ canvasElement }) => {
    const page = within(canvasElement.ownerDocument.body);
    await userEvent.type(page.getByLabelText('名前'), 'Acme');
    await userEvent.click(page.getByRole('button', { name: '作成' }));
    await expect(await page.findByRole('alert')).toHaveTextContent(
      'この表示IDはすでに使用されています',
    );
  },
};
