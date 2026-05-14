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
          label: 'API',
          autogenerate: { directory: 'api' },
        },
        {
          label: 'App',
          autogenerate: { directory: 'app' },
        },
      ],
    }),
  ],
});
