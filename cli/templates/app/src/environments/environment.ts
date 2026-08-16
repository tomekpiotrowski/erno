function ernoApiUrl(): string {
  const fromWindow = (globalThis as { __ERNO_API_URL__?: string }).__ERNO_API_URL__;
  if (fromWindow) {
    return fromWindow;
  }
  return 'http://localhost:3000';
}

export const environment = {
  production: false,
  get apiUrl(): string {
    return ernoApiUrl();
  },
  get wsUrl(): string {
    return ernoApiUrl().replace(/^http/, 'ws');
  },
};
