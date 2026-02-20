#!/bin/bash

# Usage: ./check-chromium-debug.sh [port] [host]
# Defaults: port=9222, host=localhost

PORT=${1:-9222}
HOST=${2:-localhost}

if curl -s "http://${HOST}:${PORT}/json/version" | grep -q "Browser"; then
  echo "Chromium debug is running at ${HOST}:${PORT}"
else
  echo "Chromium debug is not running at ${HOST}:${PORT}"
  exit 1
fi
