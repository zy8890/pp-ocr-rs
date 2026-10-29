#!/usr/bin/env bash
cd "$(dirname "$0")"
kill "$(cat idorc-server.pid)"
rm -f idorc-server.pid
echo "stopped"
