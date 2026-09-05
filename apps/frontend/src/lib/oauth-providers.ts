/**
 * OAuth プロバイダーの表示名と、認可フローの開始。
 *
 * サインイン画面のボタンと、設定画面の認証方法セクションが同じ入口を通る。
 * 片方だけ直すと同じプロバイダーが画面ごとに違う名前で出たり、戻り先の指定が
 * 食い違ったりするため、ラベルとパラメータの組み立てをここに集める。
 */

const PROVIDER_LABELS: Record<string, string> = {
  github: 'GitHub',
  gitlab: 'GitLab',
  gitlab_selfhosted: 'GitLab (セルフホスト)',
  google: 'Google',
  oidc: 'OIDC',
};

export function providerLabel(provider: string): string {
  return PROVIDER_LABELS[provider] ?? provider;
}

/**
 * 知っているプロバイダーか。
 *
 * `providerLabel` は知らない値をそのまま返すので、URL から拾った文字列を通知文へ
 * 混ぜる前にこれで濾す。濾さないと `?linked=<任意の文字列>` を開かせるだけで、
 * 正規の設定画面が出した通知として攻撃者の文面を読ませられる。
 * `/v1/auth/oauth/providers` の応答は画面を開いた直後にはまだ無いので、判定には使えない。
 */
export function isKnownProvider(value: string): boolean {
  return Object.hasOwn(PROVIDER_LABELS, value);
}

export type OAuthStartOptions = {
  /** 承認後の戻り先（フロントの相対パス）。 */
  redirectAfter: string;
  /** プロバイダーがエラーを返したときの戻り先。backend がここへ `?oauth_error=` を付けて返す。 */
  errorRedirectAfter: string;
  /** `requires_instance_url` のプロバイダーだけ渡す。 */
  instanceUrl?: string;
};

export function oauthStartUrl(provider: string, options: OAuthStartOptions): string {
  const apiBase = import.meta.env.VITE_API_BASE ?? '/api';
  const params = new URLSearchParams();
  params.set('redirect_after', options.redirectAfter);
  params.set('error_redirect_after', options.errorRedirectAfter);
  if (options.instanceUrl) params.set('instance_url', options.instanceUrl);
  return `${apiBase}/v1/auth/oauth/${provider}?${params.toString()}`;
}

/** openapi-fetch クライアントは 302 をパースできないため、必ずフルページ遷移させる。 */
export function startOAuth(provider: string, options: OAuthStartOptions): void {
  window.location.assign(oauthStartUrl(provider, options));
}
