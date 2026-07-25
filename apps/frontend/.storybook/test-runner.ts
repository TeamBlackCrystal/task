import { argosScreenshot } from '@argos-ci/playwright';
import type { TestRunnerConfig } from '@storybook/test-runner';
import { appendFile } from 'node:fs/promises';
import path from 'node:path';

const VIEWPORT = { width: 1440, height: 900 };
const evidenceDirectory = path.resolve(process.env.VRT_EVIDENCE_DIR ?? '../../.vrt');

const recordStory = (file: string, storyId: string) =>
  appendFile(path.join(evidenceDirectory, file), `${storyId}\n`, 'utf8');

const config: TestRunnerConfig = {
  async preVisit(page, context) {
    if (process.env.ARGOS_ENABLED === 'true') {
      await page.setViewportSize(VIEWPORT);
      await recordStory('executed-story-ids.txt', context.id);
    }
  },
  async postVisit(page, context) {
    if (process.env.ARGOS_ENABLED !== 'true') return;

    await argosScreenshot(page, context.id, {
      fullPage: true,
    });
    await recordStory('captured-story-ids.txt', context.id);
  },
};

export default config;
