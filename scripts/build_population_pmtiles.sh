#!/usr/bin/env bash
# Convertit les carreaux INSEE Filosofi 2021 (200 m, métropole) en tuiles
# vectorielles PMTiles pour la couche « Population » de la carte.
# Source : https://www.insee.fr/fr/statistiques/8735162
set -euo pipefail

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
DIR="$ROOT/data/population"
ZIP_URL="https://www.insee.fr/fr/statistiques/fichier/8735162/Filosofi2021_carreaux_200m_gpkg.zip"
ZIP="$DIR/Filosofi2021_carreaux_200m_gpkg.zip"
GPKG="carreaux_200m_met.gpkg"
OUT="population.pmtiles"
# La variante alpine-small lit le PMTiles sans savoir l'écrire (pas de GEOS).
GDAL_IMAGE="ghcr.io/osgeo/gdal:ubuntu-small-3.10.3"
# En dessous, une tuile contient trop de carreaux et GDAL en jetterait :
# des trous qui passeraient pour des zones vides.
MIN_ZOOM=10
MAX_ZOOM=12

mkdir -p "$DIR"
[ -f "$ZIP" ] || curl -fL -o "$ZIP" "$ZIP_URL"
[ -f "$DIR/$GPKG" ] || unzip -o -q "$ZIP" "$GPKG" -d "$DIR"
rm -f "$DIR/$OUT"

docker run --rm --user "$(id -u):$(id -g)" -v "$DIR:/d" "$GDAL_IMAGE" ogr2ogr \
  -f PMTiles "/d/$OUT" "/d/$GPKG" \
  -sql "SELECT geom, CAST(ROUND(ind) AS INTEGER) AS ind FROM carreaux_200m_met" \
  -nln population \
  -dsco MINZOOM=$MIN_ZOOM -dsco MAXZOOM=$MAX_ZOOM

ls -lh "$DIR/$OUT"
