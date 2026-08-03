/// <reference types="astro/client" />

interface ImportMetaEnv {
  /** Product app origin (login/register). Dev default: http://localhost:4200 */
  readonly PUBLIC_APP_URL?: string;
}

interface ImportMeta {
  readonly env: ImportMetaEnv;
}
