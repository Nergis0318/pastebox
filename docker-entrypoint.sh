#!/bin/sh
set -e

if [ "$(id -u)" = "0" ]; then
    chown -R pastebox:pastebox /paste-data
    exec su-exec pastebox "$@"
else
    exec "$@"
fi
