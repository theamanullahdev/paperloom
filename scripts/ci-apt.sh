#!/usr/bin/env bash
# Install apt packages on a CI runner without ever hanging: short network
# timeouts, retries, and a hard cap per attempt.
set -u
for attempt in 1 2 3; do
  if sudo timeout 240 bash -c "apt-get -o Acquire::Retries=3 -o Acquire::http::Timeout=20 -o Acquire::https::Timeout=20 update && apt-get -o Acquire::Retries=3 -o Acquire::http::Timeout=20 install -y --no-install-recommends $*"; then
    exit 0
  fi
  echo "apt attempt $attempt failed, retrying"
  sleep 10
done
exit 1
