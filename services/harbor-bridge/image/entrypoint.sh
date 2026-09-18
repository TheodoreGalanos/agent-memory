#!/bin/sh
set -eu
ssh-keygen -A >/dev/null 2>&1
# SSH replies belong to established connections. New outbound traffic is denied.
iptables -A OUTPUT -m conntrack --ctstate ESTABLISHED,RELATED -j ACCEPT
iptables -A OUTPUT -j REJECT
ip6tables -A OUTPUT -m conntrack --ctstate ESTABLISHED,RELATED -j ACCEPT
ip6tables -A OUTPUT -j REJECT
python3 /opt/memory/lease_watchdog.py &
exec /usr/sbin/sshd -D -e
