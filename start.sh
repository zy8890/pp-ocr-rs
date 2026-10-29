#!/usr/bin/env bash
cd "$(dirname "$0")"
nohup ./idorc-server --config default.toml > log.txt 2>&1 &
echo $! > idorc-server.pid
echo "started pid=$(cat idorc-server.pid)"
