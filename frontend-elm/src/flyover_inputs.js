/**
 * Commandes de la carte pendant le survol : la molette règle la distance, le
 * clic droit glissé tourne autour du point, le clavier pilote la lecture.
 * Les gestes de MapLibre sont coupés le temps du survol, sinon ils se
 * battraient avec la caméra, puis rendus tels qu'ils étaient.
 */

const WHEEL_ZOOM_RATE = 0.0015;
const DRAG_YAW_DEG_PER_PX = 0.3;
const DRAG_ELEVATION_DEG_PER_PX = 0.2;
const RIGHT_BUTTON = 2;
/** Assez pour la vue « Dessus », au-delà de la limite par défaut de MapLibre. */
const FLYOVER_MAX_PITCH = 85;
const TYPING_TARGETS = 'input, textarea, select, [contenteditable]';

const preventDefault = event => event.preventDefault();

/**
 * handlers : { zoom(factor), orbit(yawDeg, elevationDeg), keys: { [event.key]: action } }
 * Rend la fonction qui relâche les commandes.
 */
export function captureMapInputs(map, handlers) {
  const restoreGestures = suspendMapGestures(map);
  const stopListening = listenToInputs(map.getCanvasContainer(), handlers);
  return () => {
    restoreGestures();
    stopListening();
  };
}

function suspendMapGestures(map) {
  const previous = {
    scrollZoom: map.scrollZoom.isEnabled(),
    dragRotate: map.dragRotate.isEnabled(),
    keyboard: map.keyboard.isEnabled(),
    maxPitch: map.getMaxPitch()
  };
  map.scrollZoom.disable();
  map.dragRotate.disable();
  map.keyboard.disable();
  map.setMaxPitch(FLYOVER_MAX_PITCH);
  return () => {
    if (previous.scrollZoom) map.scrollZoom.enable();
    if (previous.dragRotate) map.dragRotate.enable();
    if (previous.keyboard) map.keyboard.enable();
    map.setMaxPitch(previous.maxPitch);
  };
}

function listenToInputs(canvas, handlers) {
  const onWheel = event => {
    event.preventDefault();
    handlers.zoom(Math.exp(event.deltaY * WHEEL_ZOOM_RATE));
  };
  const onOrbitStart = event => {
    if (event.button === RIGHT_BUTTON) followOrbitDrag(event, handlers.orbit);
  };
  const onKey = event => {
    if (event.target.closest?.(TYPING_TARGETS)) return;
    const action = handlers.keys[event.key];
    if (!action) return;
    event.preventDefault();
    action();
  };
  canvas.addEventListener('wheel', onWheel, { passive: false });
  canvas.addEventListener('mousedown', onOrbitStart);
  canvas.addEventListener('contextmenu', preventDefault);
  window.addEventListener('keydown', onKey);
  return () => {
    canvas.removeEventListener('wheel', onWheel);
    canvas.removeEventListener('mousedown', onOrbitStart);
    canvas.removeEventListener('contextmenu', preventDefault);
    window.removeEventListener('keydown', onKey);
  };
}

function followOrbitDrag(startEvent, orbit) {
  let lastX = startEvent.clientX;
  let lastY = startEvent.clientY;
  const onMove = event => {
    orbit((event.clientX - lastX) * DRAG_YAW_DEG_PER_PX, (event.clientY - lastY) * DRAG_ELEVATION_DEG_PER_PX);
    lastX = event.clientX;
    lastY = event.clientY;
  };
  const onUp = () => {
    window.removeEventListener('mousemove', onMove);
    window.removeEventListener('mouseup', onUp);
  };
  window.addEventListener('mousemove', onMove);
  window.addEventListener('mouseup', onUp);
}
