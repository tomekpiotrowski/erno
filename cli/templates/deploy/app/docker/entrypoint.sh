#!/bin/sh
set -e

# Replace __API_URL__ placeholder in built JS with the runtime API_URL env var.
# This lets a single image be deployed against different API endpoints.
if [ -n "$API_URL" ]; then
  find /usr/share/nginx/html -name "*.js" -exec sed -i "s|__API_URL__|$API_URL|g" {} \;
fi

exec "$@"
