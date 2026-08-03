#!/bin/sh
set -e

# Replace __APP_URL__ placeholder in built assets with the runtime APP_URL env var.
# This lets a single image be deployed against different product app origins.
if [ -n "$APP_URL" ]; then
  find /usr/share/nginx/html \( -name "*.html" -o -name "*.js" -o -name "*.css" \) \
    -exec sed -i "s|__APP_URL__|$APP_URL|g" {} \;
fi

exec "$@"
