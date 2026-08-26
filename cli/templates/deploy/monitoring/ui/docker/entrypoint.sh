#!/bin/sh
set -e

sed -e "s/__COLLECTOR_HOST__/${COLLECTOR_HOST:-collector}/g" \
    -e "s/__COLLECTOR_PORT__/${COLLECTOR_PORT:-3001}/g" \
    -e "s/__PROM_HOST__/${PROM_HOST:-prometheus}/g" \
    -e "s/__TEMPO_HOST__/${TEMPO_HOST:-tempo}/g" \
    -e "s/__LOKI_HOST__/${LOKI_HOST:-loki}/g" \
    /etc/nginx/templates/default.conf.template > /etc/nginx/conf.d/default.conf

exec nginx -g 'daemon off;'
