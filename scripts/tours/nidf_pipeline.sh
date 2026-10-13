#!/bin/bash
# attend fin w30, puis w5, mix, GPX, découpe 30 km, import, revêtement+IGN
S=$(dirname $(readlink -f $0)); D=$S/nidf; N=14; R=/home/jcgouleau/Projects/personal/chemins-noirs/chemins_noirs
set -e
while [ $(wc -l < $D/run_w30.log) -lt $N ]; do sleep 60; done
echo "== w30 fini $(date +%T)"; cat $D/run_w30.log
for i in $(seq 0 $((N-1))); do $S/reg_leg.sh $D $i 5; done
echo "== mix"; python3 $S/reg_mix.py $D $N
python3 $S/reg_gpx.py $D "Tour des forêts du Nord et d'Île-de-France (mix poids 30 / 5)" $R/tour-forets-nord-idf-mix.gpx
echo "== part forêt"; for i in $(seq -w 0 $((N-1))); do python3 $S/ne_mask.py $S $D/leg_$i.json $D/mask_$i.geojson >/dev/null; FOREST=$D/mask_$i.geojson python3 $S/ne_share.py $D leg_$i.json | head -1; done
echo "== découpe"; LABEL="Tour forêts Nord-IdF" PREFIX=tour-forets-nidf python3 $S/reg_split30.py $R/tour-forets-nord-idf-mix.gpx $R/tour-forets-nord-idf-mix
echo "== import"; PREFIX=tour-forets-nidf TAG=tour-forets-nord-idf python3 $S/reg_import30.py $R/tour-forets-nord-idf-mix | tee $D/import.log
A=$(head -1 $D/import.log | cut -d' ' -f1); B=$(tail -1 $D/import.log | cut -d' ' -f1)
echo "== surfaces $A-$B"; python3 $S/surf30.py $A $B
echo "== FINI $(date +%T)"
