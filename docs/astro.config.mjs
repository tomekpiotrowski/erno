import { defineConfig } from 'astro/config';
import starlight from '@astrojs/starlight';

export default defineConfig({
  integrations: [
    starlight({
      title: 'Erno',
      social: [
        { icon: 'github', label: 'GitHub', href: 'https://github.com/tomekpiotrowski/erno' },
      ],
      sidebar: [
        {
          label: 'Introduction',
          items: [
            { label: 'Getting started', link: '/getting-started/' },
            { label: 'Architecture', link: '/architecture/' },
          ],
        },
        {
          label: 'CLI',
          items: [
            { label: 'Overview', link: '/cli/' },
            { label: 'Deploy', link: '/cli/deploy/' },
          ],
        },
        {
          label: 'API',
          items: [
            { label: 'Overview', link: '/api/' },
            {
              label: 'Core',
              items: [
                { label: 'Boot & configuration', link: '/api/boot/' },
                { label: 'Database', link: '/api/database/' },
              ],
            },
            {
              label: 'Security',
              items: [
                { label: 'Authentication', link: '/api/authentication/' },
                { label: 'Authorization', link: '/api/authorization/' },
                { label: 'Rate limiting', link: '/api/rate-limiting/' },
              ],
            },
            {
              label: 'Data & realtime',
              items: [
                { label: 'Sync', link: '/api/sync/' },
                { label: 'Sharing', link: '/api/share/' },
                { label: 'File storage', link: '/api/storage/' },
                { label: 'WebSocket', link: '/api/websocket/' },
              ],
            },
            {
              label: 'Product',
              items: [
                { label: 'Billing', link: '/api/billing/' },
              ],
            },
            {
              label: 'Background & ops',
              items: [
                { label: 'Jobs', link: '/api/jobs/' },
                { label: 'Email', link: '/api/email/' },
                { label: 'Telemetry', link: '/api/telemetry/' },
                { label: 'Admin console', link: '/api/console/' },
                { label: 'Business stats', link: '/api/business-stats/' },
              ],
            },
          ],
        },
        {
          label: 'App',
          items: [
            { label: 'Overview', link: '/app/' },
            { label: 'Authentication', link: '/app/authentication/' },
            { label: 'Sync', link: '/app/sync/' },
            { label: 'Realtime', link: '/app/realtime/' },
            { label: 'File storage', link: '/app/storage/' },
            { label: 'Billing', link: '/app/billing/' },
            { label: 'Sharing', link: '/app/share/' },
            { label: 'Devtools', link: '/app/devtools/' },
          ],
        },
        {
          label: 'Guides',
          items: [
            { label: 'Sync an entity end-to-end', link: '/guides/sync-an-entity/' },
            { label: 'Gate features with billing', link: '/guides/billing-gates/' },
          ],
        },
      ],
    }),
  ],
});
