import { defineConfig } from '@hey-api/openapi-ts';

export default defineConfig({
  input: 'divergeapi.json',
  output: 'src/generated',
  plugins: [
    '@hey-api/typescript',
    '@hey-api/sdk',
  ],
});
