/**
 * Fenêtres de la carte déplaçables par une poignée.
 *
 * Le décalage passe par la propriété CSS `translate`, indépendante de
 * `transform` : le panneau de survol garde son centrage, le sélecteur de fonds
 * sa place dans le coin de MapLibre. Position gardée par navigateur ;
 * double-clic sur la poignée pour revenir à la place d'origine.
 */

const LEFT_BUTTON = 0;
const ORIGIN = { x: 0, y: 0 };

function loadOffset(key) {
  try {
    const stored = JSON.parse(localStorage.getItem(key));
    if (Number.isFinite(stored?.x) && Number.isFinite(stored?.y)) return stored;
  } catch (error) {
    // Stockage indisponible ou valeur illisible : place d'origine.
  }
  return ORIGIN;
}

function saveOffset(key, offset) {
  try {
    localStorage.setItem(key, JSON.stringify(offset));
  } catch (error) {
    // Navigation privée : la position ne survivra pas au rechargement.
  }
}

/** Crée une poignée ⠿ à placer dans la fenêtre. */
export function createGrip(title = 'Déplacer (double-clic : remettre en place)') {
  const grip = document.createElement('span');
  grip.className = 'drag-grip';
  grip.textContent = '⠿';
  grip.title = title;
  grip.setAttribute('aria-hidden', 'true');
  return grip;
}

/** Décalage qui ramène `box` dans `area`, nul s'il y est déjà. */
function shiftInside(box, area) {
  let shiftX = 0;
  let shiftY = 0;
  if (box.left < area.left) shiftX = area.left - box.left;
  else if (box.right > area.right) shiftX = area.right - box.right;
  if (box.top < area.top) shiftY = area.top - box.top;
  else if (box.bottom > area.bottom) shiftY = area.bottom - box.bottom;
  return { x: shiftX, y: shiftY };
}

/** Suit le pointeur depuis `startEvent` jusqu'au relâchement. */
function followDrag(handle, startEvent, { onMove, onEnd }) {
  startEvent.preventDefault();
  startEvent.stopPropagation();
  handle.setPointerCapture(startEvent.pointerId);
  const onUp = () => {
    handle.removeEventListener('pointermove', onMove);
    handle.removeEventListener('pointerup', onUp);
    onEnd();
  };
  handle.addEventListener('pointermove', onMove);
  handle.addEventListener('pointerup', onUp);
}

export function makeDraggable(windowElement, handle, storageKey) {
  let offset = loadOffset(storageKey);

  const translate = () => {
    windowElement.style.translate = `${offset.x}px ${offset.y}px`;
  };

  // Rester dans la carte, même après un redimensionnement.
  const applyOffset = () => {
    translate();
    const mapBounds = windowElement.closest('.maplibregl-map')?.getBoundingClientRect()
      ?? { left: 0, top: 0, right: window.innerWidth, bottom: window.innerHeight };
    const shift = shiftInside(windowElement.getBoundingClientRect(), mapBounds);
    if (shift.x || shift.y) {
      offset = { x: offset.x + shift.x, y: offset.y + shift.y };
      translate();
    }
  };

  handle.addEventListener('pointerdown', startEvent => {
    if (startEvent.button !== LEFT_BUTTON) return;
    const grabPoint = { x: startEvent.clientX - offset.x, y: startEvent.clientY - offset.y };
    windowElement.classList.add('is-dragging');
    followDrag(handle, startEvent, {
      onMove: event => {
        offset = { x: event.clientX - grabPoint.x, y: event.clientY - grabPoint.y };
        applyOffset();
      },
      onEnd: () => {
        windowElement.classList.remove('is-dragging');
        saveOffset(storageKey, offset);
      }
    });
  });

  handle.addEventListener('dblclick', () => {
    offset = ORIGIN;
    applyOffset();
    saveOffset(storageKey, offset);
  });

  // Appliqué après insertion dans la page, pour que les bornes soient connues.
  requestAnimationFrame(applyOffset);
}
