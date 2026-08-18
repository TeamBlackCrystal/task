/**
 * _cache.ts — L1 レンダリングキャッシュ (プロセス内)。
 *
 * 🔴 キーは「入力本文そのもの (full-text)」を含む合成文字列であり、ハッシュ化してはならない。
 * renderDescription はモジュールトップレベル singleton としてプロセス全体 —— SSR では
 * 全リクエスト・全 tenant を跨いで —— 共有される。djb2 等の 32-bit ハッシュは約 77,000
 * エントリで衝突確率 50% (誕生日境界) に達し、衝突した瞬間「別 tenant の private 本文から
 * レンダした HTML」を返す情報漏えいになる。JS の文字列キーは完全一致比較ゆえ衝突不能。
 * キーを hash に戻す変更は kfm-cache テスト (djb2 衝突ペア・キー形式) で落ちる。
 *
 * L2 (ブラウザ永続) は Phase 1 では採用しない。必要性を計測してから
 * SHA-256 ＋ read 時照合 ＋ tenant/logout scope の設計で足す。
 */
import { LRUCache } from 'lru-cache';

/** ブラウザ / Node 両対応の UTF-8 バイト長 (Buffer 非依存) */
function utf8ByteLength(value: string): number {
  return new TextEncoder().encode(value).length;
}

// NUL (U+0000)。JSON.stringify の出力に生では現れない (制御文字は必ずエスケープされる)
const KEY_DELIMITER = String.fromCharCode(0);

/**
 * キャッシュキー = pipeline fingerprint ＋ profile ＋ 解決済み content-scope config ＋ 本文。
 * 前置部は JSON.stringify で正規化し、NUL 区切りで本文 full-text と連結する (JSON 出力に
 * 生の NUL は現れないため、キー全体が一意に定まる)。profile を含めることで同一本文が
 * profile 違いで混線せず、config を含めることで設定変更時に旧 HTML が自動失効する。
 * config も hash せず全文を埋める (衝突不能側へ倒す)。
 */
export function buildCacheKey(
  fingerprint: string,
  profile: string,
  contentConfig: string,
  text: string,
): string {
  return JSON.stringify([fingerprint, profile, contentConfig]) + KEY_DELIMITER + text;
}

export function createL1Cache(): LRUCache<string, string> {
  return new LRUCache<string, string>({
    max: 500,
    maxSize: 4 * 1024 * 1024,
    // maxSize 使用時は全エントリのサイズ提供が必須 (lru-cache 仕様)。キー側も本文
    // full-text を含むため合算する。
    sizeCalculation: (value, key) => utf8ByteLength(value) + utf8ByteLength(key),
  });
}
