#!/bin/sh

set -eu

mkdir -p /tmp

if ! mount -t tmpfs -o mode=1777,exec,nosuid,nodev tmpfs /tmp 2>/dev/null; then
  mount -o remount,mode=1777,exec /tmp 2>/dev/null || true
  chmod 1777 /tmp 2>/dev/null || true
fi

exec /usr/bin/supervisord -c /etc/supervisord.conf
