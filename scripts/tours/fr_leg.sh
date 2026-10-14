#!/bin/bash
# usage: fr_leg.sh DIR idx w_forest -> étape idx de DIR/legs.json, écrit DIR/leg_XX_wW.json
D=$1; i=$2; W=$3
A=$(python3 -c "
import json;l=json.load(open('$D/legs.json'))[$i]
print(json.dumps({'start':l['start'],'end':l['end'],'w_pop':${WPOP:-1},'w_paved':${WPAVED:-1},'w_forest':$W}).replace(' ',''))")
out=$D/leg_$(printf %02d $i)_w$W.json
[ -s $out ] && grep -q '"path"' $out && { echo "leg $i w$W : déjà fait"; exit 0; }
t0=$(date +%s)
curl -s -m 3500 -X POST localhost:8090/api/route -H 'Content-Type: application/json' -d "$A" -o $out
echo "leg $i w$W : $(( $(date +%s)-t0 )) s, $(stat -c %s $out) octets"
