#!/bin/bash
# usage: reg_leg.sh DIR idx w_forest -> étape idx -> idx+1 de DIR/order.json, écrit DIR/leg_XX_wW.json
D=$1; i=$2; W=$3
A=$(python3 -c "
import json;F=json.load(open('$D/forests.json'));o=json.load(open('$D/order.json'));a=F[o[$i]];b=F[o[($i+1)%len(o)]]
print(json.dumps({'start':{'lat':a['lat'],'lon':a['lon']},'end':{'lat':b['lat'],'lon':b['lon']},'w_pop':1,'w_paved':1,'w_forest':$W}).replace(' ',''))")
out=$D/leg_$(printf %02d $i)_w$W.json
t0=$(date +%s)
curl -s -m 3500 -X POST localhost:8090/api/route -H 'Content-Type: application/json' -d "$A" -o $out
echo "leg $i w$W : $(( $(date +%s)-t0 )) s, $(stat -c %s $out) octets"
