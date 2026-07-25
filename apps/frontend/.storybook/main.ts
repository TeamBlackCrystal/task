import type { StorybookConfig } from '@storybook/vue3-vite';
import { mergeConfig } from 'vite';
import path from 'node:path';
import { fileURLToPath } from 'node:url';
import { vrtGraphPlugin } from '../buildSrc/vrtGraphPlugin.js';

const repositoryRoot = path.resolve(path.dirname(fileURLToPath(import.meta.url)), '../../..');

const config: StorybookConfig = {
  stories: ['../stories/**/*.mdx', '../stories/**/*.stories.@(js|jsx|mjs|ts|tsx)'],
  addons: [
    '@chromatic-com/storybook',
    '@storybook/addon-vitest',
    '@storybook/addon-a11y',
    '@storybook/addon-docs',
  ],
  framework: '@storybook/vue3-vite',
  viteFinal: async (config) =>
    mergeConfig(config, {
      plugins: [
        vrtGraphPlugin({
          repositoryRoot,
          outputFile: path.join(repositoryRoot, '.vrt/preview-graph.json'),
        }),
      ],
      resolve: {
        alias: {
          'vike/client/router': new URL('./mocks/vike-client-router.ts', import.meta.url).pathname,
        },
      },
    }),
};
export default config;
