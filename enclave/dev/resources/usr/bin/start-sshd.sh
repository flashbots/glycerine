#!/bin/sh

set -eu

generate_key() {
  key_type="$1"
  key_path="$2"
  bits="${3:-}"

  if [ -s "$key_path" ]; then
    return
  fi

  rm -f "$key_path" "$key_path.pub"

  if [ -n "$bits" ]; then
    /usr/bin/ssh-keygen -q -t "$key_type" -b "$bits" -N "" -f "$key_path"
  else
    /usr/bin/ssh-keygen -q -t "$key_type" -N "" -f "$key_path"
  fi

  chmod 0600 "$key_path"
  chmod 0644 "$key_path.pub"
}

# ----------------------------------------------------------------------

mkdir -p /etc/ssh /run /root/.ssh /var/empty

generate_key ed25519 /etc/ssh/ssh_host_ed25519_key
generate_key rsa /etc/ssh/ssh_host_rsa_key 3072

touch /var/log/lastlog
chmod 664 /var/log/lastlog

exec /usr/sbin/sshd -D -e -f /etc/ssh/sshd_config
