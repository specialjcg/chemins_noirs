/**
 * Profil en long du panneau de survol. Deux images fixes (fond et partie
 * parcourue, avec les fins d'étape) sont peintes une fois ; à chaque image
 * on ne fait que les découper au curseur.
 */

const BOTTOM_PADDING_PX = 4;
/** Marges haute et basse cumulées autour de la courbe. */
const VERTICAL_PADDING_PX = 10;
const STOP_MARK_WIDTH_PX = 2;
const CURSOR_WIDTH_PX = 2;

/** size : { width, height, pixelRatio } en pixels CSS ; colors : { profile, done, stop }. */
function paintProfileLayers({ track, stops, size, colors }) {
  return {
    base: paintLayer({ track, stops, size, colors }, colors.profile),
    done: paintLayer({ track, stops, size, colors }, colors.done)
  };
}

function elevationScale(track, height) {
  // Pas de Math.min(...profil) : un long tracé dépasse le nombre d'arguments permis.
  let lowest = Infinity;
  let highest = -Infinity;
  track.elevations?.forEach(elevation => {
    if (elevation == null) return;
    lowest = Math.min(lowest, elevation);
    highest = Math.max(highest, elevation);
  });
  if (lowest > highest) {
    lowest = 0;
    highest = 1;
  }
  const drawable = height - VERTICAL_PADDING_PX;
  return elevation => height - BOTTOM_PADDING_PX - (elevation - lowest) / Math.max(highest - lowest, 1) * drawable;
}

function paintLayer({ track, stops, size, colors }, fill) {
  const { width, height, pixelRatio } = size;
  const xAt = distanceM => distanceM / track.totalM * width;
  const yAt = elevationScale(track, height);
  const layer = document.createElement('canvas');
  layer.width = width * pixelRatio;
  layer.height = height * pixelRatio;
  const context = layer.getContext('2d');
  context.scale(pixelRatio, pixelRatio);

  context.beginPath();
  context.moveTo(0, height);
  if (track.elevations?.some(elevation => elevation != null)) {
    track.elevations.forEach((elevation, index) => {
      if (elevation != null) context.lineTo(xAt(track.cumul[index]), yAt(elevation));
    });
  } else {
    // Sans profil, une bande plate garde la barre de progression lisible.
    context.lineTo(0, height / 2);
    context.lineTo(width, height / 2);
  }
  context.lineTo(width, height);
  context.closePath();
  context.fillStyle = fill;
  context.fill();

  context.fillStyle = colors.stop;
  stops.forEach(stop => {
    context.fillRect(Math.round(xAt(stop.distanceM)) - STOP_MARK_WIDTH_PX / 2, 0, STOP_MARK_WIDTH_PX, height);
  });
  return layer;
}

/** Peint le profil découpé à `fraction` (0 à 1) et le curseur. */
function drawProfile(canvas, { layers, fraction, cursorColor, pixelRatio }) {
  const context = canvas.getContext('2d');
  const cut = Math.round(fraction * canvas.width);
  context.clearRect(0, 0, canvas.width, canvas.height);
  context.drawImage(layers.base, 0, 0);
  if (cut > 0) context.drawImage(layers.done, 0, 0, cut, canvas.height, 0, 0, cut, canvas.height);
  context.fillStyle = cursorColor;
  context.fillRect(cut - CURSOR_WIDTH_PX / 2 * pixelRatio, 0, CURSOR_WIDTH_PX * pixelRatio, canvas.height);
}

/**
 * Profil peint dans `canvas`. Les images fixes ne sont refaites que si le
 * tracé, les étapes ou la taille changent : tracé et étapes sont remplacés,
 * jamais modifiés, leur identité suffit.
 * view : { track, stops, positionM, colors }
 */
export function createProfileView(canvas) {
  let painted = null;

  function layersFor(view, size) {
    const isStale = !painted || painted.track !== view.track || painted.stops !== view.stops
      || painted.width !== size.width || painted.height !== size.height || painted.pixelRatio !== size.pixelRatio;
    if (isStale) {
      painted = {
        ...size,
        track: view.track,
        stops: view.stops,
        layers: paintProfileLayers({ track: view.track, stops: view.stops, size, colors: view.colors })
      };
    }
    return painted.layers;
  }

  return {
    render(view) {
      const size = { width: canvas.clientWidth, height: canvas.clientHeight, pixelRatio: window.devicePixelRatio || 1 };
      if (view.track.totalM <= 0 || !size.width || !size.height) return;
      if (canvas.width !== size.width * size.pixelRatio || canvas.height !== size.height * size.pixelRatio) {
        canvas.width = size.width * size.pixelRatio;
        canvas.height = size.height * size.pixelRatio;
      }
      drawProfile(canvas, {
        layers: layersFor(view, size),
        fraction: view.positionM / view.track.totalM,
        cursorColor: view.colors.cursor,
        pixelRatio: size.pixelRatio
      });
    }
  };
}
