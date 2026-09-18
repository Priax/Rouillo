# Déployer le backend Puyorust sur Oracle Cloud (Always Free) — de A à Z

Procédure complète, telle que réalisée le 2026-09-17. À suivre si la VM est perdue
(supprimée, récupérée par Oracle, compte recréé…).

```
Client ──wss://puyo.priax.org/ws / https://puyo.priax.org/api──▶ Caddy (443, TLS Let's Encrypt)
                                                                   │
                                                                   ▼
                                                  serveur Rust 127.0.0.1:8080 ──▶ PostgreSQL (local)
```

| Élément | Valeur actuelle |
|---|---|
| Région Oracle | France South (Marseille) — `eu-marseille-1`, AD-1 |
| Shape | `VM.Standard.A1.Flex` (ARM aarch64), 2 OCPU / 12 Go |
| OS | Canonical Ubuntu 24.04 (pas « Minimal ») |
| Utilisateur SSH | `ubuntu` (clé `~/.ssh/id_ed25519`) |
| IP publique | `129.151.236.143` (éphémère → **change si la VM est recréée**) |
| Domaine | `puyo.priax.org` (DNS Cloudflare, enregistrement A, *DNS only*) |
| Base | PostgreSQL 16, base `puyorust`, rôle `puyo` |
| Fichiers serveur | `/opt/puyorust/server`, `/opt/puyorust/puyo.env` |
| Services systemd | `puyo`, `caddy`, `postgresql` |

> ⚠️ **Les comptes et parties sont stockés dans PostgreSQL sur la VM.** Si la VM est
> supprimée sans sauvegarde, ils sont perdus. Voir [§10 Sauvegardes](#10-sauvegardes-de-la-base).

---

## 1. Créer la VM (console Oracle)

**Compute → Instances → Create instance.** Le formulaire a 4 étapes + Review.

### Étape 1 — Basic information
- **Name** : `Instance VM Puyorust` (libre).
- **Compartment** : racine (`Priax (root)`).
- **Availability domain** : AD-1 (Marseille n'en a qu'un).
- **Image and shape** — changer le **shape d'abord**, puis l'image :
  1. **Change shape** → onglet **Ampere** (ARM) → `VM.Standard.A1.Flex` (*Always Free-eligible*).
     Déplier la ligne avec la **petite flèche ▸** à gauche du nom pour régler
     **OCPUs = 2**, **Memory = 12 GB** (max gratuit : 4 / 24 au total) → **Select shape**.
  2. **Change image** → **Canonical Ubuntu 24.04** (l'entrée *non* Minimal ; le build aarch64
     est choisi automatiquement) → **Select image**.
- Astuce : si un panneau ne s'ouvre pas, il est hors écran → **dézoomer (Ctrl −)**.
- Si « **Out of capacity** » : réessayer plus tard. En dernier recours `VM.Standard.E2.1.Micro`
  (x86, 1 Go RAM) : ça tient, mais **impossible de compiler dessus** → compiler sur le PC
  (`cargo build --release -p server --target x86_64-unknown-linux-musl`) et `scp` le binaire.

### Étape 2 — Security
Ne rien toucher → **Next**.

### Étape 3 — Networking
- **Primary network** : *Create new virtual cloud network* → `puyo-vcn`.
- **Subnet** : *Create new public subnet* → `puyo-subnet` (CIDR `10.0.0.0/24`).
- **Private IPv4** : automatique.
- **Public IPv4** : l'interrupteur est **grisé** (bug connu : le subnet n'existe pas encore).
  C'est normal → on l'ajoutera après création (§2).
- **IPv6** : non.
- **Add SSH keys** → *Paste public key* → coller la sortie de `cat ~/.ssh/id_ed25519.pub`.

### Étape 4 — Storage
Tout par défaut (boot volume 46,6 Go, inclus dans les 200 Go gratuits) → **Next**.

### Review
Vérifier Ubuntu 24.04 / A1.Flex / clé SSH présente → **Create**.
L'estimation « €1.85/month » pour le boot volume **ignore le free tier** : c'est gratuit.

Attendre **Running** (≈ 1–2 min).

## 2. Ajouter l'IP publique

Page de l'instance → onglet **Networking** → section **Attached VNICs** → cliquer le VNIC →
**IP administration / IPv4 addresses** → ligne de l'IP privée → **⋮ → Edit** →
**Public IP type : Ephemeral public IP** → **Update**.

Noter l'IP publique (→ `IP_VM` dans la suite).

## 3. Ouvrir les ports 80/443 — pare-feu Oracle (Security List)

Onglet Networking de l'instance → lien **`puyo-subnet`** → **Security** →
**Default Security List for puyo-vcn** → **Security rules** → **Add Ingress Rules** :

- Source CIDR `0.0.0.0/0`, protocole **TCP**, Destination Port Range **`80,443`**,
  description `HTTP/HTTPS Puyo`.

**Ne PAS ouvrir 8080** (le serveur reste derrière Caddy). Le ping ne répondra pas (ICMP echo
bloqué) : c'est normal, tester avec `curl` (§9).

## 4. Première connexion + mise à jour

```bash
ssh ubuntu@IP_VM            # répondre "yes" la première fois
sudo apt update && sudo apt upgrade -y
```

Si SSH refuse à cause d'une ancienne empreinte (VM recréée avec une IP déjà connue) :
`ssh-keygen -R IP_VM`.

## 5. Ouvrir 80/443 — pare-feu de la VM (iptables)

Les images Ubuntu d'Oracle ont un `REJECT` en fin de chaîne. **Ne pas utiliser `ufw`.**

```bash
sudo iptables -L INPUT -n --line-numbers
```

Repérer le **numéro de la ligne REJECT** (c'était **5** le 2026-09-17) et insérer **à ce
numéro** (N = ligne du REJECT) :

```bash
sudo iptables -I INPUT N -m state --state NEW -p tcp --dport 80 -j ACCEPT
sudo iptables -I INPUT N -m state --state NEW -p tcp --dport 443 -j ACCEPT
sudo netfilter-persistent save
sudo iptables -L INPUT -n --line-numbers   # 443 et 80 doivent être AU-DESSUS du REJECT
```

Doublon par erreur ? `sudo iptables -D INPUT <num>` puis `sudo netfilter-persistent save`.

## 6. PostgreSQL

```bash
sudo apt install -y postgresql
```

**Tout le bloc suivant dans la même session SSH** (la variable `PGPASS` est perdue sinon).
Ne pas faire `echo $PGPASS`.

```bash
PGPASS=$(openssl rand -hex 24)
sudo -u postgres psql -c "CREATE USER puyo WITH PASSWORD '$PGPASS';" \
                      -c "CREATE DATABASE puyorust OWNER puyo;"

sudo useradd -r -s /usr/sbin/nologin puyo
sudo mkdir -p /opt/puyorust
echo "DATABASE_URL=postgres://puyo:$PGPASS@127.0.0.1/puyorust" | sudo tee /opt/puyorust/puyo.env > /dev/null
sudo chown puyo:puyo /opt/puyorust/puyo.env
sudo chmod 600 /opt/puyorust/puyo.env

psql "postgres://puyo:$PGPASS@127.0.0.1/puyorust" -c "SELECT 1;"   # doit afficher 1
```

Les migrations (`server/migrations`) sont appliquées **automatiquement** au démarrage du serveur.
Pour restaurer une sauvegarde, voir §10 (à faire **avant** le premier démarrage du serveur).

## 7. Compiler et installer le serveur

Compilation directement sur la VM (repo GitHub public) :

```bash
sudo apt install -y build-essential git
curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh -s -- -y
source ~/.cargo/env

git clone https://github.com/Priax/Rouillo.git ~/puyorust
cd ~/puyorust
cargo build --release -p server          # ≈ 3–4 min sur 2 OCPU

sudo cp ~/puyorust/target/release/server /opt/puyorust/server
sudo chmod +x /opt/puyorust/server
```

Service systemd (`deploy/` est gitignoré, donc on le recrée ici) :

```bash
sudo tee /etc/systemd/system/puyo.service > /dev/null <<'EOF'
[Unit]
Description=Puyorust authoritative game server
After=network.target postgresql.service
Wants=postgresql.service

[Service]
EnvironmentFile=/opt/puyorust/puyo.env
ExecStart=/opt/puyorust/server
Restart=always
RestartSec=2
User=puyo
Group=puyo
NoNewPrivileges=true
ProtectSystem=strict
ProtectHome=true
PrivateTmp=true

[Install]
WantedBy=multi-user.target
EOF

sudo systemctl daemon-reload
sudo systemctl enable --now puyo
journalctl -u puyo -n 20 --no-pager      # attendu : "DB connectée" puis "Écoute sur :8080"
curl -i http://127.0.0.1:8080/ws         # attendu : 404 {"error":"Not found"} (normal sans headers WS)
```

## 8. DNS (Cloudflare) puis Caddy

**À faire avant Caddy** (Caddy a besoin du DNS pour obtenir le certificat).

dash.cloudflare.com → `priax.org` → **DNS → Records** → modifier (ou créer) l'enregistrement :

| Type | Name | IPv4 | Proxy status | TTL |
|---|---|---|---|---|
| A | `puyo` | `IP_VM` | **DNS only (nuage gris)** | Auto |

Ne pas toucher aux enregistrements `priax.org` / `www` (portfolio). Vérifier depuis le PC :
`dig +short puyo.priax.org` → doit renvoyer `IP_VM`.

Caddy :

```bash
sudo apt install -y caddy
sudo tee /etc/caddy/Caddyfile > /dev/null <<'EOF'
puyo.priax.org {
	reverse_proxy /ws 127.0.0.1:8080
	reverse_proxy /api/* 127.0.0.1:8080
}
EOF
sudo systemctl restart caddy
journalctl -u caddy -n 30 --no-pager     # attendu : "certificate obtained successfully"
```

## 9. Vérifier de bout en bout (depuis le PC)

```bash
# API (pseudo volontairement invalide → aucun compte créé)
curl -s -X POST -H "Content-Type: application/json" -d '{"username":"a","password":"x"}' \
  -w " [%{http_code}]\n" https://puyo.priax.org/api/register
# attendu : {"error":"Username must be 3-24 ..."} [400]

# WebSocket
curl -s -i --http1.1 -H "Connection: Upgrade" -H "Upgrade: websocket" \
  -H "Sec-WebSocket-Version: 13" -H "Sec-WebSocket-Key: dGhlIHNhbXBsZSBub25jZQ==" \
  https://puyo.priax.org/ws | head -1
# attendu : HTTP/1.1 101 Switching Protocols
```

Puis lancer le client (`cargo run --release -p client` ou la release GitHub), créer un compte,
cliquer Play. Sur la VM, `journalctl -u puyo -f` doit afficher `WS N ouverture`.

Le client n'a **pas besoin d'être republié** tant que le domaine reste `puyo.priax.org`
(URLs dans `shared/src/config.rs`) : seul le DNS change si l'IP change.

## 10. Sauvegardes de la base

Sans sauvegarde, perdre la VM = perdre comptes, ELO et historique.

Depuis le PC, récupérer un dump :

```bash
ssh ubuntu@IP_VM 'sudo -u postgres pg_dump -Fc puyorust' > puyorust-$(date +%F).dump
```

Restaurer sur une VM neuve, **après §6 et avant de démarrer le serveur (§7)** :

```bash
scp puyorust-XXXX.dump ubuntu@IP_VM:/tmp/
ssh ubuntu@IP_VM 'sudo -u postgres pg_restore --no-owner --role=puyo -d puyorust /tmp/puyorust-XXXX.dump'
```

## 11. Éviter la suppression de la VM

Oracle récupère les instances Always Free **inactives** (CPU < 20 % sur 7 jours, etc.).
Parade : **Billing → Upgrade to Pay As You Go** (les ressources Always Free restent gratuites),
puis **Billing → Budgets** : budget avec alerte à **1 €**.

## 12. Mettre à jour le serveur

Sur la VM :

```bash
cd ~/puyorust && git pull && cargo build --release -p server \
  && sudo cp target/release/server /opt/puyorust/server \
  && sudo systemctl restart puyo \
  && journalctl -u puyo -n 5 --no-pager
```

Si `shared/` a changé (protocole), republier aussi le client (tag `vX.Y.Z`).

## Pièges rencontrés

| Symptôme | Cause | Fix |
|---|---|---|
| Interrupteur « public IPv4 » grisé à la création | Subnet créé en même temps que la VM | Ajouter l'IP éphémère après (§2) |
| Règles iptables sans effet | Insérées après le `REJECT` | Insérer au numéro de ligne du REJECT (§5) |
| Client natif : « Not found » à l'inscription | Double `Content-Type` envoyé par ehttp/ureq | Corrigé dans `client/src/http.rs` (`push_header` remplace) — v0.7.0 |
| Client natif : « connexion au serveur perdue » sur Play | ewebsock compilé sans TLS → pas de `wss://` | `ewebsock = { features = ["tls"] }` — v0.7.0 |
| `ping puyo.priax.org` ne répond pas | ICMP echo bloqué par la Security List | Normal, tester avec `curl` (§9) |
| Caddy n'obtient pas de certificat | DNS absent ou proxy Cloudflare orange | Enregistrement A en *DNS only* (§8) |
