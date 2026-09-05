import { apiProxyPlugin } from '#/middlewares/api-proxy';
import { prehydrationQueryGuard } from '#/middlewares/prehydration-query-guard';
import { renderDescriptionPlugin } from '#/middlewares/render-description';
import { settingInjector } from '#/middlewares/setting-injector';
import { staticPlugin } from '@elysiajs/static';
import vike from '@vikejs/elysia';
import { ZxcvbnFactory } from '@zxcvbn-ts/core';
import * as commonPackage from '@zxcvbn-ts/language-common';
import * as jaPackage from '@zxcvbn-ts/language-ja';
import { Elysia, t } from 'elysia';

type PasswordStrength = '' | 'low' | 'medium' | 'high';

const zxcvbn = new ZxcvbnFactory({
  dictionary: {
    ...commonPackage.dictionary,
    ...jaPackage.dictionary,
  },
  graphs: commonPackage.adjacencyGraphs,
});

function scoreToStrength(score: number): PasswordStrength {
  if (score <= 1) return 'low';
  if (score <= 3) return 'medium';
  return 'high';
}

export function getApp() {
  const app = new Elysia();

  app.use(staticPlugin({ assets: 'server/public', prefix: '/static-assets' }));

  app.post(
    '/internal/password-strength',
    ({ body }) => {
      const { password } = body;
      if (!password) return { strength: '' as const };

      return { strength: scoreToStrength(zxcvbn.check(password).score) };
    },
    {
      body: t.Object({
        password: t.String({ maxLength: 256 }),
      }),
    },
  );

  app.use(renderDescriptionPlugin);

  app.use(apiProxyPlugin);

  vike(app, [prehydrationQueryGuard, settingInjector]);

  return app;
}

export const app = getApp();
