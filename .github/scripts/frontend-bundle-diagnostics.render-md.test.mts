/*
 * SPDX-FileCopyrightText: syuilo and misskey-project
 * SPDX-License-Identifier: AGPL-3.0-only
 */

import { describe, expect, it } from 'vitest';
import {
  baseBootstrapNote,
  collectChunks,
  compressionFootnote,
  getChunkStatus,
  isUnchangedChunk,
  metricCells,
  stableChunkName,
  sum,
  type VisualizerReport,
} from './frontend-bundle-diagnostics.render-md.mts';

const sampleReport = (overrides?: Partial<VisualizerReport>): VisualizerReport => ({
  nodeParts: {
    partA: { renderedLength: 100, gzipLength: 40, brotliLength: 35, zstdLength: 30 },
    partB: { renderedLength: 200, gzipLength: 80, brotliLength: 70, zstdLength: 60 },
    partC: { renderedLength: 50, gzipLength: 20, brotliLength: 18, zstdLength: 15 },
  },
  nodeMetas: {
    meta1: { moduleParts: { 'assets/chunk-alpha.12345678.js': 'partA' } },
    meta2: { moduleParts: { 'assets/chunk-beta.abcdef01.js': 'partB' } },
  },
  ...overrides,
});

describe('frontend-bundle-diagnostics.render-md', () => {
  it('normalizes hashed chunk names', () => {
    expect(stableChunkName('assets/chunk-alpha.12345678.js')).toBe('assets/chunk-alpha.[hash].js');
  });

  it('aggregates chunk metrics and totals across four compression fields', () => {
    const chunks = collectChunks(sampleReport());
    expect(chunks.get('assets/chunk-alpha.[hash].js')).toEqual({
      rendered: 100,
      gzip: 40,
      brotli: 35,
      zstd: 30,
    });
    expect(sum(chunks)).toEqual({
      rendered: 300,
      gzip: 120,
      brotli: 105,
      zstd: 90,
    });
  });

  it('detects new, removed, and changed chunk status', () => {
    const before = collectChunks(sampleReport());
    const after = collectChunks(
      sampleReport({
        nodeMetas: {
          meta1: { moduleParts: { 'assets/chunk-alpha.12345678.js': 'partA' } },
          meta3: { moduleParts: { 'assets/chunk-gamma.99999999.js': 'partC' } },
        },
      }),
    );

    expect(getChunkStatus(before, after, 'assets/chunk-alpha.[hash].js')).toBe('changed');
    expect(getChunkStatus(before, after, 'assets/chunk-beta.[hash].js')).toBe('removed');
    expect(getChunkStatus(before, after, 'assets/chunk-gamma.[hash].js')).toBe('new');
  });

  it('skips unchanged changed chunks', () => {
    const before = collectChunks(sampleReport());
    const after = collectChunks(sampleReport());
    expect(isUnchangedChunk(before, after, 'assets/chunk-alpha.[hash].js')).toBe(true);
  });

  it('renders four metric cells with bytes and percent deltas', () => {
    const before = { rendered: 100, gzip: 40, brotli: 35, zstd: 30 };
    const after = { rendered: 0, gzip: 60, brotli: 45, zstd: 0 };
    expect(metricCells(before, after, 'rendered')).toEqual(['100 B', '0 B', '-100 B', '-100.0%']);
    expect(metricCells(before, after, 'gzip')).toEqual(['40 B', '60 B', '+20 B', '+50.0%']);
    expect(metricCells({ ...before, zstd: 0 }, after, 'zstd')).toEqual(['0 B', '0 B', '0 B', '0.0%']);
    expect(metricCells({ ...before, zstd: 0 }, { ...after, zstd: 12 }, 'zstd')).toEqual([
      '0 B',
      '12 B',
      '+12 B',
      'new',
    ]);
  });

  it('documents per-part compression sums in the report footnote', () => {
    expect(compressionFootnote).toContain('per-part compression sums');
    expect(compressionFootnote).toContain('before/after deltas');
  });

  it('documents legacy base bootstrap caveat when base config was copied from after', () => {
    expect(baseBootstrapNote).toBe('> この差分は依存変更を反映していない');
  });
});
