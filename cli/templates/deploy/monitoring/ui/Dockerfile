# The monitoring operator console.
#
# `npm ci` rather than `npm install`: monitoring/ui ships a package-lock.json,
# and a reproducible install is the point of having one.
FROM node:22-alpine AS build
WORKDIR /src
COPY package.json package-lock.json* ./
RUN npm ci
COPY . .
RUN npm run build

FROM nginx:1.27-alpine
COPY docker/nginx.conf /etc/nginx/templates/default.conf.template
COPY docker/entrypoint.sh /entrypoint.sh
RUN chmod +x /entrypoint.sh
COPY --from=build /src/dist/erno-monitoring-ui/browser /usr/share/nginx/html
ENTRYPOINT ["/entrypoint.sh"]
