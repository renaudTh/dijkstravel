# Studio auto-hébergé

Le studio sert les runs du dépôt dans le navigateur : calcul de la tournée, rendu (complet ou des
premières secondes), couverture et légende, avec l'avancement en direct.

| Service | Rôle |
|---|---|
| `studio-api` | API HTTP, flux SSE, interface Angular ; initialise Garage et importe le dépôt au démarrage |
| `studio-worker` | traite les jobs : appelle les libs `sources`, `solve`, `render` |
| `postgres` | runs, thèmes, index des fichiers, file des jobs |
| `garage` | stockage S3 des fichiers produits |
| `vroom` | ordre de visite (ne reçoit que des matrices) |
| `osrm-car`, `osrm-foot`, `osrm-train` | routage, chacun dans le profil compose du même nom |

## Prérequis

- Les graphes OSRM préparés dans `OSRM_DIR`, comme pour la CLI (voir `docker-compose.yml` à la racine).
- Rust (`~/.cargo/bin`) et Node sur l'hôte : le binaire et le front sont compilés ici, l'image ne
  fait que les copier (`ubuntu:26.04`, même glibc que l'hôte).

## Premier lancement

```sh
cp deploy/.env.example deploy/.env
# Remplir les secrets : openssl rand -hex 32 (POSTGRES_PASSWORD, GARAGE_RPC_SECRET, GARAGE_ADMIN_TOKEN,
# S3_SECRET_ACCESS_KEY) ; S3_ACCESS_KEY_ID = GK + openssl rand -hex 12.
deploy/build.sh
docker compose -f deploy/compose.yaml --env-file deploy/.env up -d
```

Le studio répond sur <http://127.0.0.1:8080>. Au premier démarrage, il importe `runs/`, `themes/` et
les vidéos déjà rendues de `IMPORT_OUT_DIR`. Un run déjà connu du studio n'est jamais réimporté.

## Routage

`COMPOSE_PROFILES` choisit les OSRM démarrés (`car` par défaut ; `car,train` pour les gares). Avec
15 Go de RAM, `france-routes` et un rendu tiennent ensemble, pas les trois OSRM en plus.

Pour réutiliser les OSRM déjà lancés par le `docker-compose.yml` de la racine, vider
`COMPOSE_PROFILES` et viser l'hôte : `OSRM_URL=http://host.docker.internal:5000`,
`VROOM_URL=http://host.docker.internal:3000`.

## Disque DATA

DATA est en NTFS et monté à l'ouverture de session (udisks). D'où deux choix :

- Postgres et les métadonnées de Garage vont dans des volumes Docker nommés (Postgres exige un
  dossier en `0700`, ce que NTFS ne permet pas). Les blocs Garage, le cache et les graphes restent
  sur DATA.
- `RESTART_POLICY=no` : au boot, avant le montage, Docker créerait des dossiers vides sur `/`. Pour
  démarrer le studio au boot, monter DATA par fstab, puis passer à `unless-stopped` :

  ```
  UUID=<blkid /dev/sdd1>  /run/media/thomas/DATA  ntfs3  uid=1000,gid=1000,nofail  0  0
  ```

## Commandes utiles

```sh
docker compose -f deploy/compose.yaml --env-file deploy/.env logs -f studio-worker
docker compose -f deploy/compose.yaml --env-file deploy/.env --profile train up -d osrm-train
docker compose -f deploy/compose.yaml --env-file deploy/.env exec studio-api /app/bin/dijkstravel-server import /seed --out /seed-out
```
