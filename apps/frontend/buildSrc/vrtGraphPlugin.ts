import { mkdir, writeFile } from 'node:fs/promises';
import path from 'node:path';
import type { Plugin } from 'vite';

type PreviewGraph = {
  schemaVersion: number;
  modules: Record<string, { reasons: string[] }>;
};

const stripQuery = (value: string) => value.split(/[?#]/, 1)[0] ?? value;

export function vrtGraphPlugin(options: { repositoryRoot: string; outputFile: string }): Plugin {
  const normalize = (moduleId: string) => {
    const clean = stripQuery(moduleId);
    if (path.isAbsolute(clean)) {
      const relative = path.relative(options.repositoryRoot, clean);
      return relative.startsWith('..') ? clean : relative.replaceAll('\\', '/');
    }
    return clean.replaceAll('\\', '/');
  };

  return {
    name: 'vrt-preview-graph',
    apply: 'build',
    async generateBundle() {
      const modules: PreviewGraph['modules'] = {};
      const moduleIds = [...this.getModuleIds()].filter((moduleId) => !moduleId.startsWith('\0'));
      const included = new Set(moduleIds.map(normalize));
      for (const moduleId of moduleIds) {
        const info = this.getModuleInfo(moduleId);
        if (!info) continue;
        const normalizedId = normalize(moduleId);
        const existingReasons = modules[normalizedId]?.reasons ?? [];
        modules[normalizedId] = {
          reasons: [
            ...new Set([
              ...existingReasons,
              ...[...info.importedIds, ...info.dynamicallyImportedIds]
                .filter((importedId) => !importedId.startsWith('\0'))
                .map(normalize)
                .filter((importedId) => included.has(importedId)),
            ]),
          ].sort(),
        };
      }
      const graph: PreviewGraph = { schemaVersion: 1, modules };
      await mkdir(path.dirname(options.outputFile), { recursive: true });
      await writeFile(options.outputFile, `${JSON.stringify(graph)}\n`, 'utf8');
    },
  };
}
