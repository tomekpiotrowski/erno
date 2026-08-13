#!/bin/sh
set -e

sed -e "s/__API_HOST__/${API_HOST:-api}/g" \
    -e "s/__API_PORT__/${API_PORT:-3000}/g" \
    -e "s/__PROM_HOST__/${PROM_HOST:-prometheus}/g" \
    /etc/nginx/templates/default.conf.template > /etc/nginx/conf.d/default.conf

exec nginx -g 'daemon off;'
