# Rouillo

Un Puyo Puyo multijoueur en Rust. Ma copine adore le jeu et on y joue ensemble, alors autant faire le mien !

Jouable dans le navigateur sur [puyo.priax.org](https://puyo.priax.org), ou en natif sur Linux et Windows via les [releases](https://github.com/Priax/Rouillo/releases), qui se mettent à jour toutes seules.

## Ce qu'on peut y faire

- Jouer en ligne jusqu'à 16 joueurs par partie, avec des spectateurs et un chat
- Monter dans le mode classé et le classement
- Revoir ses parties grâce aux replays
- Jouer seul en mode infini, ou contre des CPU
- Ajouter des amis et les inviter
- Jouer au clavier, à la manette ou au tactile, avec des touches configurables

## Comment c'est fait

Un workspace Cargo en quatre crates:

- `shared`: la logique du jeu et le protocole, communs au client et au serveur
- `server`: le serveur de jeu (tokio, axum, PostgreSQL avec sqlx)
- `client`: le jeu, compilé en natif avec notan et en WebAssembly pour le navigateur
- `loadbot`: des bots pour tester la charge du serveur

## Lancer le projet

Il faut PostgreSQL et `trunk`.

```sh
DATABASE_URL=postgres://user:mdp@localhost/puyorust cargo run -p server
cd client && trunk serve --port 8000   # navigateur
cargo run -p client                    # natif
```
