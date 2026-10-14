#!/bin/bash
# Tour de France des forêts domaniales (axe >= 15 km), par région, tronçons de 30 km.
# Reprise possible : les étapes déjà calculées sont sautées.
S=$(dirname $(readlink -f $0)); D=$S/fr; R=/home/jcgouleau/Projects/personal/chemins-noirs/chemins_noirs
O=$R/tour-france-forets
set -e
cd $S && python3 fr_legs.py
N=$(python3 -c "import json;print(len(json.load(open('$D/legs.json'))))")
for W in 30 5; do
  for i in $(seq 0 $((N-1))); do $S/fr_leg.sh $D $i $W; done | tee -a $D/run_w$W.log
  echo "== w$W fini $(date +%T)"
done
echo "== mix"; python3 $S/reg_mix.py $D $N | tee $D/mix.log
mkdir -p $O; python3 $S/fr_gpx.py $D $O > $D/regions.tsv; cat $D/regions.tsv
: > $D/import.log
while IFS=$'\t' read -r gpx reg km; do
  base=$(basename $gpx .gpx)
  echo "== $reg : découpe"
  LABEL="TdF forêts $reg" PREFIX=$base python3 $S/reg_split30.py $gpx $O/$base
  echo "== $reg : import"
  PREFIX=$base TAG=tour-france-forets FOLDER="Tour de France forêts — $reg" python3 $S/reg_import30.py $O/$base | tee -a $D/import.log
done < $D/regions.tsv
A=$(head -1 $D/import.log | cut -d' ' -f1); B=$(tail -1 $D/import.log | cut -d' ' -f1)
echo "== surfaces $A-$B"; python3 $S/surf30.py $A $B | tee $D/surf.log
echo "== FINI $(date +%T)"
