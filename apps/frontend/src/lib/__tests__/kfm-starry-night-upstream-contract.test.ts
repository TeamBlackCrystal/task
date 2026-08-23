import fs from 'node:fs';
import path from 'node:path';
import { fileURLToPath, pathToFileURL } from 'node:url';
import { describe, expect, it } from 'vitest';
import { starryNightSanitizeSchema } from '../rehype-starry-night';

const STARRY_NIGHT_ROOT = path.dirname(fileURLToPath(import.meta.resolve('@wooorm/starry-night')));
const LOCAL_STYLE_PATH = path.join(
  path.dirname(fileURLToPath(import.meta.url)),
  '../rehype-starry-night/style.css',
);

const readUpstream = (relativePath: string): string =>
  fs.readFileSync(path.join(STARRY_NIGHT_ROOT, relativePath), 'utf8');

const uniqueSorted = (values: Iterable<string>): string[] => [...new Set(values)].sort();

/**
 * theme.js の class 値域は正規表現抽出ではなく実体を import して読む。
 * 抽出パターン (例: pl-[a-z0-9]+ 限定) だと upstream が pl-foo-bar のような
 * パターン外 class を足したとき黙って取り零し、件数固定の試験がそのまま通ってしまう。
 */
async function loadThemeClasses(): Promise<string[]> {
  const themeModule = (await import(
    pathToFileURL(path.join(STARRY_NIGHT_ROOT, 'lib/theme.js')).href
  )) as { classes?: unknown };
  const { classes } = themeModule;
  if (!Array.isArray(classes) || !classes.every((value) => typeof value === 'string')) {
    throw new Error('starry-night lib/theme.js の export const classes を読めなかった');
  }
  return uniqueSorted(classes);
}

function extractSelectorClasses(source: string): string[] {
  // theme.js 側と違い CSS には機械可読な export が無いためセレクタ抽出は残るが、
  // sanitize の許可パターンより広く取る (ハイフン許容) — 抽出漏れで差分が隠れる側に倒さない。
  const withoutComments = source.replace(/\/\*[\s\S]*?\*\//g, '');
  return uniqueSorted(
    [...withoutComments.matchAll(/\.(pl-[a-z0-9-]+)/g)].map((match) => match[1]!),
  );
}

function extractVariables(source: string): Map<string, string> {
  return new Map(
    [...source.matchAll(/(--color-prettylights-syntax-[\w-]+)\s*:\s*([^;]+);/g)].map((match) => [
      match[1]!,
      match[2]!.trim(),
    ]),
  );
}

describe('@wooorm/starry-night upstream 契約', () => {
  it('theme.js の値域 34 class は sanitize schema で全て許可される', async () => {
    const classes = await loadThemeClasses();
    expect(classes).toHaveLength(34);
    expect(
      classes.filter(
        (className) =>
          !starryNightSanitizeSchema.classPatterns.some((pattern) => pattern.test(className)),
      ),
    ).toEqual([]);
  });

  it('light.css と both.css のセレクタ集合は同じ 33 class', async () => {
    const themeClasses = await loadThemeClasses();
    const lightClasses = extractSelectorClasses(readUpstream('style/light.css'));
    const bothClasses = extractSelectorClasses(readUpstream('style/both.css'));
    expect(lightClasses).toHaveLength(33);
    expect(bothClasses).toEqual(lightClasses);
    // pl-kos は grammar 出力には現れ得るが upstream CSS に規則が無い、既知の唯一の差分。
    expect(themeClasses.filter((className) => !lightClasses.includes(className))).toEqual([
      'pl-kos',
    ]);
    expect(lightClasses.filter((className) => !themeClasses.includes(className))).toEqual([]);
  });

  it('style.css の .dark 30 変数は upstream dark.css と完全一致する', () => {
    const upstreamDark = extractVariables(readUpstream('style/dark.css'));
    const localDark = extractVariables(fs.readFileSync(LOCAL_STYLE_PATH, 'utf8'));
    expect(upstreamDark.size).toBe(30);
    expect(localDark).toEqual(upstreamDark);
  });
});
