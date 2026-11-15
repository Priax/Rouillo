#!/bin/sh
# Prepares the VM, once, so the game server takes WebTransport on UDP 443
# with Caddy's certificate:
#   - a daily timer copies the certificate where the server can read it;
#   - puyo.env gets the WT_* settings, the service may bind port 443;
#   - Caddy stops serving HTTP/3, which held UDP 443;
#   - iptables lets UDP 443 in.
#
#   scripts/setup-webtransport.sh
#   PUYO_HOST=user@host   the VM (default ubuntu@puyo.priax.org)
set -eu

HOST=${PUYO_HOST:-ubuntu@puyo.priax.org}
DOMAIN=${HOST#*@}

ssh -o ServerAliveInterval=30 "$HOST" "DOMAIN=$DOMAIN sh -s" <<'REMOTE'
set -eu
DIR=/opt/puyorust

echo '==> Certificate copy'
sudo tee $DIR/copy-tls.sh >/dev/null <<'EOF'
#!/bin/sh
# Copies Caddy's certificate for the domain $1 where the game server reads it.
set -eu
domain=$1
tls=/opt/puyorust/tls
src=$(find /var/lib/caddy/.local/share/caddy/certificates -type d -name "$domain" | head -n 1)
if [ -z "$src" ]; then
    echo "no certificate for $domain in Caddy's storage" >&2
    exit 1
fi
if cmp -s "$src/$domain.crt" $tls/cert.pem && cmp -s "$src/$domain.key" $tls/key.pem; then
    exit 0
fi
install -D -o puyo -g puyo -m 600 "$src/$domain.key" $tls/key.pem
install -D -o puyo -g puyo -m 644 "$src/$domain.crt" $tls/cert.pem
EOF
sudo chmod 755 $DIR/copy-tls.sh

sudo tee /etc/systemd/system/puyo-tls.service >/dev/null <<EOF
[Unit]
Description=Copy Caddy's certificate for the Puyo game server
After=caddy.service

[Service]
Type=oneshot
ExecStart=$DIR/copy-tls.sh $DOMAIN
EOF

sudo tee /etc/systemd/system/puyo-tls.timer >/dev/null <<'EOF'
[Unit]
Description=Copy Caddy's certificate for the Puyo game server, daily

[Timer]
OnBootSec=2min
OnCalendar=daily
Persistent=true

[Install]
WantedBy=timers.target
EOF

sudo systemctl daemon-reload
sudo systemctl enable --now puyo-tls.timer
sudo systemctl start puyo-tls.service
sudo ls -l $DIR/tls

echo '==> Server settings'
for line in "WT_ADDR=0.0.0.0:443" "WT_URL=https://$DOMAIN/wt" "WT_CERT=$DIR/tls/cert.pem" "WT_KEY=$DIR/tls/key.pem"; do
    key=${line%%=*}
    if ! sudo grep -q "^$key=" $DIR/puyo.env; then
        echo "$line" | sudo tee -a $DIR/puyo.env >/dev/null
    fi
done
sudo mkdir -p /etc/systemd/system/puyo.service.d
sudo tee /etc/systemd/system/puyo.service.d/webtransport.conf >/dev/null <<'EOF'
[Service]
# WebTransport listens on UDP 443.
AmbientCapabilities=CAP_NET_BIND_SERVICE
CapabilityBoundingSet=CAP_NET_BIND_SERVICE
EOF
sudo systemctl daemon-reload

echo '==> Caddy without HTTP/3'
if ! grep -q 'protocols h1 h2' /etc/caddy/Caddyfile; then
    sudo cp -p /etc/caddy/Caddyfile /etc/caddy/Caddyfile.before-webtransport
    {
        cat <<'EOF'
{
    servers {
        protocols h1 h2
    }
}

EOF
        sudo cat /etc/caddy/Caddyfile
    } | sudo tee /etc/caddy/Caddyfile.new >/dev/null
    sudo caddy validate --config /etc/caddy/Caddyfile.new --adapter caddyfile
    sudo mv /etc/caddy/Caddyfile.new /etc/caddy/Caddyfile
    sudo systemctl reload caddy
fi

echo '==> iptables'
rule="-m state --state NEW -p udp --dport 443 -j ACCEPT"
if ! sudo iptables -C INPUT $rule 2>/dev/null; then
    # Before the first REJECT, or the rule would never be reached.
    first_reject=$(sudo iptables -L INPUT --line-numbers -n | awk '$2 == "REJECT" { print $1; exit }')
    sudo iptables -I INPUT "${first_reject:-1}" $rule
    sudo netfilter-persistent save
fi
sudo iptables -L INPUT -n --line-numbers | grep -E 'dpt:443|REJECT'
REMOTE

cat <<EOF

Done on the VM. Left to do:
  - Oracle console: let UDP 443 in (DEPLOY.md §14), once;
  - scripts/deploy.sh: the server restarts with WebTransport.
EOF
