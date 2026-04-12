#!/bin/bash
# Wrapper around cargo-zigbuild that appends .2.28 to linux-gnu targets
# to ensure glibc 2.28 compatibility (RHEL 8 / AlmaLinux 8).
args=()
for arg in "$@"; do
  args+=("${arg/unknown-linux-gnu/unknown-linux-gnu.2.28}")
done
exec cargo-zigbuild "${args[@]}"
