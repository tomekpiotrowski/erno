import { defineConfig } from 'astro/config';

// https://astro.build/config
export default defineConfig({
  // Static HTML for CDN / nginx — best default for marketing SEO.
  output: 'static',
});
