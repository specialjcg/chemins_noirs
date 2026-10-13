#!/bin/bash
# attend fin w30, puis w5, mix, GPX, découpe 30 km, import, revêtement+IGN
S=$(dirname $(readlink -f $0)); D=$S/sud; N=10; R=/home/jcgouleau/Projects/personal/chemins-noirs/chemins_noirs
echo "== mix"; python3 $S/reg_mix.py $D $N
python3 $S/reg_gpx.py $D "Tour des forêts du Sud (mix poids 30 / 5)" $R/tour-forets-sud-mix.gpx
echo "== part forêt"; for i in $(seq -f %02g 0 $((N-1))); do python3 $S/ne_mask.py $S $D/leg_$i.json $D/mask_$i.geojson >/dev/null; FOREST=$D/mask_$i.geojson python3 $S/ne_share.py $D leg_$i.json | head -1; done
echo "== découpe"; LABEL="Tour forêts Sud" PREFIX=tour-forets-sud python3 $S/reg_split30.py $R/tour-forets-sud-mix.gpx $R/tour-forets-sud-mix
echo "== import"; PREFIX=tour-forets-sud TAG=tour-forets-sud python3 $S/reg_import30.py $R/tour-forets-sud-mix | tee $D/import.log
A=$(head -1 $D/import.log | cut -d' ' -f1); B=$(tail -1 $D/import.log | cut -d' ' -f1)
echo "== surfaces $A-$B"; python3 $S/surf30.py $A $B
echo "== FINI $(date +%T)"
