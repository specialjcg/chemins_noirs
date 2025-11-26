# Contrôle de la Caméra 3D - MapLibre GL v5.x

## Vue d'ensemble

MapLibre GL JS v5.13.0 n'inclut **pas** la Free Camera API (`getFreeCameraOptions`/`setFreeCameraOptions`). Cette API est uniquement disponible dans :
- Mapbox GL JS (version propriétaire)
- MapLibre Native (Android/iOS/C++)

## Solutions Alternatives

Pour contrôler la caméra 3D dans MapLibre GL JS, utilisez les méthodes standard :

### 1. `setCamera3DPosition(options)` - Contrôle précis

Position la caméra avec un contrôle complet sur tous les paramètres :

```javascript
setCamera3DPosition({
  center: [4.5776, 45.9305],  // [longitude, latitude]
  zoom: 15,                    // Niveau de zoom
  pitch: 60,                   // Inclinaison (0-85 degrés)
  bearing: 180,                // Rotation (0-360 degrés)
  animate: true,               // Animation fluide
  duration: 2000               // Durée en ms
});
```

### 2. `flyToLocation(options)` - Animation cinématique

Anime la caméra avec un effet de vol :

```javascript
flyToLocation({
  center: [4.5776, 45.9305],
  zoom: 16,
  pitch: 70,
  bearing: 45,
  duration: 3000,
  essential: true  // Ne peut pas être interrompu
});
```

### 3. Méthodes MapLibre natives

#### `easeTo()` - Animation fluide
```javascript
mapInstance.easeTo({
  center: [lng, lat],
  zoom: 14,
  pitch: 60,
  bearing: 90,
  duration: 1500,
  easing: (t) => t * (2 - t)  // easeOutQuad
});
```

#### `flyTo()` - Animation de vol
```javascript
mapInstance.flyTo({
  center: [lng, lat],
  zoom: 15,
  pitch: 65,
  bearing: 180,
  speed: 1.2,  // Vitesse relative
  curve: 1     // Courbure de la trajectoire
});
```

#### `jumpTo()` - Changement instantané
```javascript
mapInstance.jumpTo({
  center: [lng, lat],
  zoom: 14,
  pitch: 60,
  bearing: 0
});
```

## Animation de la caméra le long d'un parcours

Le code existant utilise `easeTo()` dans la fonction `animateCamera()` :

```javascript
mapInstance.easeTo({
  center: [cameraPoint.lon, cameraPoint.lat],
  bearing: smoothedBearing,
  pitch: HUMAN_PITCH,  // 78 degrés
  zoom,
  duration: 32,        // Animation très courte pour un mouvement fluide
  easing: (t) => t     // Linéaire
});
```

## Paramètres disponibles

### Pitch (Inclinaison)
- **Plage**: 0° (vue aérienne) à 85° (presque horizontal)
- **Par défaut**: 0°
- **Recommandé pour 3D**: 60-80°

### Bearing (Rotation)
- **Plage**: 0° à 360°
- **0°**: Nord en haut
- **90°**: Est en haut
- **180°**: Sud en haut
- **270°**: Ouest en haut

### Zoom
- **Plage typique**: 0 (monde entier) à 22 (très proche)
- **Pour terrain 3D**: 12-17 recommandé

## Exemples d'utilisation

### Vue 3D d'un point d'intérêt
```javascript
setCamera3DPosition({
  center: [4.5776, 45.9305],
  zoom: 16,
  pitch: 75,
  bearing: calculateBearing(startPoint, endPoint),
  animate: true,
  duration: 2000
});
```

### Rotation autour d'un point
```javascript
let bearing = 0;
function rotateCamera() {
  setCamera3DPosition({
    center: [4.5776, 45.9305],
    zoom: 15,
    pitch: 60,
    bearing: bearing,
    animate: true,
    duration: 100
  });
  bearing = (bearing + 1) % 360;
  requestAnimationFrame(rotateCamera);
}
```

### Vol cinématique vers une destination
```javascript
flyToLocation({
  center: destinationCoords,
  zoom: 14,
  pitch: 70,
  bearing: targetBearing,
  duration: 5000
});
```

## Compatibilité

✅ **MapLibre GL JS v5.x** - Toutes les méthodes standard sont disponibles
❌ **Free Camera API** - Non disponible dans MapLibre GL JS
✅ **Terrain 3D** - Pleinement supporté avec `setTerrain()`

## Références

- [MapLibre GL JS API - CameraOptions](https://maplibre.org/maplibre-gl-js/docs/API/type-aliases/CameraOptions/)
- [MapLibre GL JS - Map Class](https://maplibre.org/maplibre-gl-js/docs/API/classes/Map/)
