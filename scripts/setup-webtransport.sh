#!/bin/sh
# Prepares the server VM for WebTransport, once. Running it again changes
# nothing that is already in place.
#
#   scripts/setup-webtransport.sh
#
#   PUYO_HOST=user@host   the VM (default ubuntu@puyo.priax.org)
#
# The game server takes WebTransport on UDP 443 with Caddy's certificate:
#   - a root timer copies the certificate where the server can read it, now
#     and every day (the server reads it again when it changes);
#   - puyo.env gets the WT_* settings, the unit may bind port 443;
#   - Caddy stops serving HTTP/3, which held UDP 443;
#   - iptables lets UDP 443 in.
# The Oracle Security List must let UDP 443 in too: that one is done in the
# console (DEPLOY.md §14). The server uses all this from its next restart,
# which the next scripts/deploy.sh does.
set -eu

HOST=${PUYO_HOST:-ubuntu@puyo.priax.org}
DOMAIN=${HOST#*@}

ssh -o ServerAliveInterval=30 "$HOST" "DOMAIN=$DOMAIN sh -s" <<'REMOTE'
set -eu
DIR=/opt/puyorust

echo '==> Certificate copy'
sudo tee $DIR/copy-tls.sh > /dev/null <<EOF
#!/bin/sh
# Copies Caddy's certificate for $DOMAIN where the game server reads it.
set -eu
src=\$(find /var/lib/caddy/.local/share/caddy/certificates -type d -name '$DOMAIN' | head -n 1)
[ -n "\$src" ] || { echo "no certificate for $DOMAIN in Caddy's storage" >&2; exit 1; }
cmp -s "\$src/$DOMAIN.crt" $DIR/tls/cert.pem && cmp -s "\$src/$DOMAIN.key" $DIR/tls/key.pem && exit 0
install -D -o puyo -g puyo -m 600 "\$src/$DOMAIN.key" $DIR/tls/key.pem
install -D -o puyo -g puyo -m 644 "\$src/$DOMAIN.crt" $DIR/tls/cert.pem
EOF
sudo chmod 755 $DIR/copy-tls.sh
sudo tee /etc/systemd/system/puyo-tls.service > /dev/null <<EOF
[Unit]
Description=Copy Caddy's certificate for the Puyo game server
After=caddy.service

[Service]
Type=oneshot
ExecStart=$DIR/copy-tls.sh
EOF
sudo tee /etc/systemd/system/puyo-tls.timer > /dev/null <<'EOF'
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
    sudo grep -q "^$key=" $DIR/puyo.env || echo "$line" | sudo tee -a $DIR/puyo.env > /dev/null
done
sudo mkdir -p /etc/systemd/system/puyo.service.d
sudo tee /etc/systemd/system/puyo.service.d/webtransport.conf > /dev/null <<'EOF'
[Service]
# WebTransport listens on UDP 443.
AmbientCapabilities=CAP_NET_BIND_SERVICE
CapabilityBoundingSet=CAP_NET_BIND_SERVICE
EOF
sudo systemctl daemon-reload

echo '==> Caddy without HTTP/3'
if ! grep -q 'protocols h1 h2' /etc/caddy/Caddyfile; then
    sudo cp -p /etc/caddy/Caddyfile /etc/caddy/Caddyfile.before-webtransport
    { printf '{\n\tservers {\n\t\tprotocols h1 h2\n\t}\n}\n\n'; sudo cat /etc/caddy/Caddyfile; } \
        | sudo tee /etc/caddy/Caddyfile.new > /dev/null
    sudo caddy validate --config /etc/caddy/Caddyfile.new --adapter caddyfile
    sudo mv /etc/caddy/Caddyfile.new /etc/caddy/Caddyfile
    sudo systemctl reload caddy
fi

echo '==> iptables'
if ! sudo iptables -C INPUT -m state --state NEW -p udp --dport 443 -j ACCEPT 2> /dev/null; then
    n=$(sudo iptables -L INPUT --line-numbers -n | awk '$2 == "REJECT" { print $1; exit }')
    sudo iptables -I INPUT "${n:-1}" -m state --state NEW -p udp --dport 443 -j ACCEPT
    sudo netfilter-persistent save
fi
sudo iptables -L INPUT -n --line-numbers | grep -E 'dpt:443|REJECT'
REMOTE

cat <<EOF

Done on the VM. Left to do:
  - Oracle console: let UDP 443 in (DEPLOY.md §14), once;
  - scripts/deploy.sh: the server restarts with WebTransport.
EOF
