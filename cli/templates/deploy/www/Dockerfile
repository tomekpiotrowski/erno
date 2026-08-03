FROM node:22 AS builder
WORKDIR /www
COPY package*.json ./
RUN npm ci
COPY . .
# Placeholder rewritten at container start from APP_URL (see docker/entrypoint.sh).
ENV PUBLIC_APP_URL=__APP_URL__
RUN npm run build

FROM nginx:alpine AS runtime
COPY --from=builder /www/dist /usr/share/nginx/html
COPY docker/nginx.conf /etc/nginx/conf.d/default.conf
COPY docker/entrypoint.sh /entrypoint.sh
RUN chmod +x /entrypoint.sh
ENTRYPOINT ["/entrypoint.sh"]
CMD ["nginx", "-g", "daemon off;"]
