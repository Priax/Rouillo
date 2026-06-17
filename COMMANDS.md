# Commandes utiles

Aide-mémoire de tout ce qu'on peut lancer sur le projet. Workspace Cargo à 3 crates :

| Crate    | Rôle                          | Cible            |
|----------|-------------------------------|------------------|
| `shared` | logique de jeu + protocole    | lib (+ benches)  |
| `server` | serveur autoritatif warp+tokio| binaire natif    |
| `client` | jeu : **natif (notan)** OU **WASM (navigateur)** | bin natif + WASM |

---

## Prérequis

```bash
# Cible WASM pour le client navigateur
rustup target add wasm32-unknown-unknown

# Trunk : sert/compile le client WASM
cargo install trunk

# Dépendances système pour le client NATIF (Linux ; cf. CI release.yml)
sudo apt-get install -y libx11-dev libxi-dev libxcursor-dev libxrandr-dev \
    libgl1-mesa-dev libxkbcommon-dev libasound2-dev
```

---

## Lancer le jeu (dev)

```bash
# 1) Le serveur (écoute sur ws://0.0.0.0:8080/ws — cf. shared/src/config.rs)
cargo run -p server

# 2a) Le client dans le NAVIGATEUR (WASM), servi sur http://localhost:8000
cd client && trunk serve --port 8000 --address 0.0.0.0

# 2b) …OU le client en fenêtre NATIVE (desktop)
cargo run -p client
```

**À quel serveur le client se connecte-t-il ?** (`client/src/main.rs::server_url`)
- WASM **release** : même hôte que la page (`ws[s]://<host>/ws`) — pour la prod.
- Natif, ou WASM **debug** (donc `trunk serve`) : `config::SERVER_URL` = `ws://127.0.0.1:8080/ws`.

---

## Variables d'environnement

| Variable          | Effet | Exemple |
|-------------------|-------|---------|
| `PUYO_PROFILE=1`  | Le serveur imprime toutes les 5 s une ligne `[tick] rooms=N avg=.. max=.. budget=16.6ms peak_load=X%`. Sans la variable : silencieux. | `PUYO_PROFILE=1 cargo run -p server --release` |
| `PUYO_LOAD_ROOMS` | Nombre de rooms du test de charge (défaut 500). | `PUYO_LOAD_ROOMS=2000 cargo test -p server --release -- --ignored --nocapture load_many_rooms` |
| `RUST_LOG`        | (non câblé pour l'instant — le serveur logge via `println!`). | — |

---

## Tests

```bash
cargo test --workspace            # toute la suite (28 tests ; le test de charge est ignoré)
cargo test -p shared              # 18 tests cœur de simulation + protocole
cargo test -p server              # 10 tests machine d'états (rooms / reconnexion)

cargo test -p shared four_connected      # filtrer par sous-chaîne du nom
cargo test -p server -- --nocapture      # voir les println! des tests
```

### Test de charge (perf, opt-in, à lancer en release)

```bash
# 500 rooms par défaut
cargo test -p server --release -- --ignored --nocapture load_many_rooms

# nombre custom
PUYO_LOAD_ROOMS=2000 cargo test -p server --release -- --ignored --nocapture load_many_rooms
```
Affiche `tick avg/max`, coût par room, `peak_load` et l'estimation du nb de rooms par tick.

---

## Benchmarks (Criterion)

```bash
cargo bench -p shared             # encode/decode, board_tick, check_matches, room_step…
```
- Rapport HTML détaillé : `target/criterion/<bench>/report/index.html`.
- Toujours significatif **en release** (Criterion compile déjà optimisé).
- Benches définis dans `shared/benches/sim.rs`.

---

## Qualité / lint

```bash
cargo clippy --workspace --tests          # lint tout, tests inclus (objectif : 0 warning)
cargo clippy --workspace --tests --fix     # applique les corrections auto sûres
cargo fmt                                  # formatage
cargo fmt --check                          # vérifie sans modifier (utile en CI)
```

---

## Build / release

```bash
cargo build --release                      # tout le workspace, optimisé (lto, opt-level 3)
cargo build --release -p server            # serveur seul
cargo build --release -p client            # client NATIF seul

# Client WASM optimisé (sortie dans client/dist/)
cd client && trunk build --release
```

### Publier une release GitHub
Le workflow `.github/workflows/release.yml` se déclenche sur un tag `v*` et construit les
binaires **natifs** du client (Linux + Windows) attachés à une release brouillon :

```bash
git tag v0.5.0
git push origin v0.5.0
```

---

## Profilage

Prérequis : `perf` installé + `kernel.perf_event_paranoid <= 2` (sinon
`sudo sysctl kernel.perf_event_paranoid=1`), et `cargo install flamegraph`.

> **Pas** `cargo flamegraph --bin server` : le serveur est une boucle infinie qui ne se
> termine jamais → perf profilerait un serveur **inactif**. On profile une cible **bornée**.

### Flamegraph (hotspots CPU)

```bash
# Hot path d'encodage (bincode + lz4) via un bench Criterion borné à 12 s
cargo flamegraph -p shared --bench sim -o flamegraph-encode.svg -- --profile-time 12 encode_state_update_full

# Idem sur le vrai travail serveur par room (tick + clone + encode)
cargo flamegraph -p shared --bench sim -o flamegraph-room.svg -- --profile-time 12 room_step_tick_and_encode
```
Ouvrir le `.svg` dans un navigateur (zoom interactif). Le profil `bench` garde les symboles
(`[profile.bench] strip=false, debug=true` dans le Cargo.toml racine).
NB : profiler à travers Criterion ajoute du bruit de harnais (`serde_json`, `clap`) à ignorer.

### tokio-console

```bash
cargo install tokio-console   # une fois

# Terminal 1 : serveur instrumenté (feature + cfg unstable obligatoires)
RUSTFLAGS="--cfg tokio_unstable" cargo run -p server --features console

# Terminal 2 : l'UI temps réel
tokio-console
```
Sans `--features console`, `console-subscriber` n'est pas compilé (zéro coût en prod).

---
