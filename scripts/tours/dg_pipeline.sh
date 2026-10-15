#!/bin/bash
# Diagonale des forêts isolées, mode chemin noir renforcé (population 5, bitume 8, forêt 10).
# Reprise possible : les étapes déjà calculées sont sautées.
S=$(dirname $(readlink -f $0)); D=$S/fr/dg; R=/home/jcgouleau/Projects/personal/chemins-noirs/chemins_noirs
O=$R/diagonale-forets-isolees; W=10
set -e
cd $S && [ -s $D/legs.json ] || python3 dg_cross.py legs
N=$(python3 -c "import json;print(len(json.load(open('$D/legs.json'))))")
for i in $(seq 0 $((N-1))); do WPOP=5 WPAVED=8 $S/fr_leg.sh $D $i $W; done | tee -a $D/run.log
echo "== étapes finies $(date +%T)"
for i in $(seq -f %02g 0 $((N-1))); do
  grep -q '"path"' $D/leg_${i}_w$W.json || { echo "étape $i sans tracé"; exit 1; }
  cp $D/leg_${i}_w$W.json $D/leg_$i.json
done
python3 dg_cross.py trim
mkdir -p $O; python3 $S/fr_gpx.py $D $O > $D/regions.tsv; cat $D/regions.tsv
IFS=$'\t' read -r gpx reg km < $D/regions.tsv
[ -n "$IMPORT" ] || { echo "== GPX prêt, découpe et import non lancés (IMPORT=1 pour les lancer)"; exit 0; }
echo "== découpe"; LABEL="Diagonale forêts isolées" PREFIX=diagonale-forets python3 $S/reg_split30.py $gpx $O/troncons
echo "== import"; PREFIX=diagonale-forets TAG=diagonale-forets-isolees FOLDER="Diagonale des forêts isolées" python3 $S/reg_import30.py $O/troncons | tee $D/import.log
A=$(head -1 $D/import.log | cut -d' ' -f1); B=$(tail -1 $D/import.log | cut -d' ' -f1)
echo "== surfaces $A-$B"; python3 $S/surf30.py $A $B | tee $D/surf.log
echo "== FINI $(date +%T)"
