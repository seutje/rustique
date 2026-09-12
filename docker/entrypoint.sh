#!/bin/sh
set -eu

# Keep particle-render as PID 1 so Ctrl+C reaches its cancellation handler.
exec /usr/local/bin/particle-render "$@"
