FROM oven/bun:1.4.0 AS bun
FROM node:22 AS build
COPY --from=bun /usr/local/bin/bun /usr/local/bin/bun
ENV BUN_FEATURE_FLAG_DISABLE_STREAMING_INSTALL=1
WORKDIR /src
COPY package.json bun.lock ./
RUN bun install --frozen-lockfile
COPY . .
RUN bun run build

FROM nginx:1.27-alpine
COPY docker/nginx.conf /etc/nginx/templates/default.conf.template
COPY docker/entrypoint.sh /entrypoint.sh
RUN chmod +x /entrypoint.sh
COPY --from=build /src/dist/erno-admin/browser /usr/share/nginx/html
ENTRYPOINT ["/entrypoint.sh"]
