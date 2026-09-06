FROM oven/bun:1.4.0 AS bun
FROM node:22 AS builder
COPY --from=bun /usr/local/bin/bun /usr/local/bin/bun
ENV BUN_FEATURE_FLAG_DISABLE_STREAMING_INSTALL=1
WORKDIR /app
COPY package.json bun.lock ./
RUN bun install --frozen-lockfile
COPY . .
RUN bun run build

FROM nginx:alpine AS runtime
COPY --from=builder /app/dist/app/browser /usr/share/nginx/html
COPY docker/nginx.conf /etc/nginx/conf.d/default.conf
COPY docker/entrypoint.sh /entrypoint.sh
RUN chmod +x /entrypoint.sh
ENTRYPOINT ["/entrypoint.sh"]
CMD ["nginx", "-g", "daemon off;"]
