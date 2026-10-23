#!/usr/bin/env bash
# Install apt packages on a CI runner without ever hanging: short network
# timeouts, retries, and a hard cap per attempt.
set -u
# No prompts, no service restarts: those are what stall an unattended install.
export DEBIAN_FRONTEND=noninteractive NEEDRESTART_MODE=a NEEDRESTART_SUSPEND=1
for attempt in 1 2; do
  if sudo -E timeout 420 bash -c "apt-get -o Acquire::Retries=3 -o Acquire::http::Timeout=20 -o Acquire::https::Timeout=20 update && apt-get -o Acquire::Retries=3 -o Acquire::http::Timeout=20 install -y --no-install-recommends -o Dpkg::Options::=--force-confold $*"; then
    exit 0
  fi
  echo "apt attempt $attempt failed, retrying"
  sleep 10
done
exit 1
