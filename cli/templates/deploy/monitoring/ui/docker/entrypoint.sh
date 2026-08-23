#!/bin/sh
set -e

sed -e "s/__COLLECTOR_HOST__/${COLLECTOR_HOST:-collector}/g" \
    -e "s/__COLLECTOR_PORT__/${COLLECTOR_PORT:-3001}/g" \
    -e "s/__PROM_HOST__/${PROM_HOST:-prometheus}/g" \
    /etc/nginx/templates/default.conf.template > /etc/nginx/conf.d/default.conf

exec nginx -g 'daemon off;'
