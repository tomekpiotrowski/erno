FROM oven/bun:1.4.0 AS bun
FROM node:22 AS builder
COPY --from=bun /usr/local/bin/bun /usr/local/bin/bun
ENV BUN_FEATURE_FLAG_DISABLE_STREAMING_INSTALL=1
WORKDIR /www
COPY package.json bun.lock ./
RUN bun install --frozen-lockfile
COPY . .
# Placeholder rewritten at container start from APP_URL (see docker/entrypoint.sh).
ENV PUBLIC_APP_URL=__APP_URL__
RUN bun run build

FROM nginx:alpine AS runtime
COPY --from=builder /www/dist /usr/share/nginx/html
COPY docker/nginx.conf /etc/nginx/conf.d/default.conf
COPY docker/entrypoint.sh /entrypoint.sh
RUN chmod +x /entrypoint.sh
ENTRYPOINT ["/entrypoint.sh"]
CMD ["nginx", "-g", "daemon off;"]
