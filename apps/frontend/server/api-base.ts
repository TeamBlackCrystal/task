import arkenv from 'arkenv';
import dotenv from 'dotenv';

// The production SSR entry runs independently of Vite, so load runtime values
// before arkenv validates process.env. Existing process variables still win.
dotenv.config({ quiet: true });

// backend (Rust) の待ち受けオリジン。既定値 http://localhost:3400 の単一ソース。
// 消費側は 2 つ: middlewares/api-proxy.ts (Elysia の /api/* 転送) と、タスク詳細の
// +data.ts (Vike server ランタイムで proxy を経由せず backend を直接叩く)。
// vite.config.ts の dev proxy target も同じ値だが、あちらは tsconfig.node.json の
// composite project 境界の外にあり本ファイルを import できないため重複のままにしている。
const env = arkenv({
  API_BASE: "string.url = 'http://localhost:3400'",
});

export const API_BASE = env.API_BASE;
