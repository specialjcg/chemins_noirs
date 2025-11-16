# Configuration Mapbox pour la vue 3D

## Pourquoi Mapbox?

Nous avons remplacé Cesium par Mapbox GL JS car:
- ✅ Inscription qui fonctionne (pas de problème d'email)
- ✅ Token gratuit avec 50,000 chargements de carte/mois
- ✅ Excellent rendu 3D avec relief
- ✅ Images satellite de haute qualité

## Comment obtenir votre token Mapbox (GRATUIT)

1. **Allez sur:** https://account.mapbox.com/auth/signup/
2. **Créez un compte** avec votre email
3. **Confirmez votre email** (l'email arrive rapidement!)
4. **Copiez votre token** depuis le dashboard (https://account.mapbox.com/)
5. **Remplacez le token** dans `frontend/mapbox3d.js` ligne 2

```javascript
// Remplacez ce token de démo par le vôtre
mapboxgl.accessToken = 'VOTRE_TOKEN_ICI';
```

## Limites du tier gratuit

- 50,000 chargements de carte par mois
- Largement suffisant pour un projet personnel
- Pas de carte de crédit requise

## En cas de problème

Si vous ne voulez pas créer de compte, le token de démo fonctionnera temporairement mais pourrait être révoqué.
