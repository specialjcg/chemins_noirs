#!/usr/bin/env bash
# Exporte les carreaux INSEE Filosofi 2021 (200 m, métropole) en CSV pour le
# routage : coin sud-ouest du carreau en ETRS89-LAEA (EPSG:3035) et nombre
# d'habitants. La grille INSEE est alignée en 3035 : la géométrie du
# GeoPackage, reprojetée en Lambert-93, ne l'est plus, d'où la lecture de
# l'identifiant : CRS3035RES200mN<nord sur 7 chiffres>E<est>. La lettre E
# figure aussi dans « RES », d'où des positions fixes plutôt qu'un instr.
# Le GeoPackage vient de build_population_pmtiles.sh.
set -euo pipefail

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
DIR="$ROOT/data/population"
GPKG="carreaux_200m_met.gpkg"
OUT="insee_200m.csv"
GDAL_IMAGE="ghcr.io/osgeo/gdal:ubuntu-small-3.10.3"

[ -f "$DIR/$GPKG" ] || { echo "Lancer d'abord build_population_pmtiles.sh" >&2; exit 1; }
rm -f "$DIR/$OUT"

docker run --rm --user "$(id -u):$(id -g)" -v "$DIR:/d" "$GDAL_IMAGE" ogr2ogr \
  -f CSV "/d/$OUT" "/d/$GPKG" \
  -dialect SQLite \
  -sql "SELECT CAST(substr(idcar_200m, 24) AS INTEGER) AS east,
               CAST(substr(idcar_200m, 16, 7) AS INTEGER) AS north,
               CAST(ROUND(ind) AS INTEGER) AS ind
        FROM carreaux_200m_met
        WHERE length(idcar_200m) = 30"

ls -lh "$DIR/$OUT"
