/**
 * Fenêtres de la carte déplaçables par une poignée.
 *
 * Le décalage passe par la propriété CSS `translate`, indépendante de
 * `transform` : le panneau de survol garde son centrage, le sélecteur de fonds
 * sa place dans le coin de MapLibre. Position gardée par navigateur ;
 * double-clic sur la poignée pour revenir à la place d'origine.
 */

function load(key) {
  try {
    const v = JSON.parse(localStorage.getItem(key));
    if (Number.isFinite(v?.x) && Number.isFinite(v?.y)) return v;
  } catch (e) {
    // Stockage indisponible ou valeur illisible : place d'origine.
  }
  return { x: 0, y: 0 };
}

function save(key, off) {
  try {
    localStorage.setItem(key, JSON.stringify(off));
  } catch (e) {
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

export function makeDraggable(el, handle, key) {
  let off = load(key);

  const apply = () => {
    el.style.translate = `${off.x}px ${off.y}px`;
    // Rester dans la carte, même après un redimensionnement.
    const area = el.closest('.maplibregl-map')?.getBoundingClientRect()
      ?? { left: 0, top: 0, right: window.innerWidth, bottom: window.innerHeight };
    const r = el.getBoundingClientRect();
    let dx = 0;
    let dy = 0;
    if (r.left < area.left) dx = area.left - r.left;
    else if (r.right > area.right) dx = area.right - r.right;
    if (r.top < area.top) dy = area.top - r.top;
    else if (r.bottom > area.bottom) dy = area.bottom - r.bottom;
    if (dx || dy) {
      off = { x: off.x + dx, y: off.y + dy };
      el.style.translate = `${off.x}px ${off.y}px`;
    }
  };

  handle.addEventListener('pointerdown', e => {
    if (e.button !== 0) return;
    e.preventDefault();
    e.stopPropagation();
    handle.setPointerCapture(e.pointerId);
    const start = { x: e.clientX - off.x, y: e.clientY - off.y };
    el.classList.add('is-dragging');
    const move = ev => {
      off = { x: ev.clientX - start.x, y: ev.clientY - start.y };
      apply();
    };
    const up = () => {
      handle.removeEventListener('pointermove', move);
      handle.removeEventListener('pointerup', up);
      el.classList.remove('is-dragging');
      save(key, off);
    };
    handle.addEventListener('pointermove', move);
    handle.addEventListener('pointerup', up);
  });

  handle.addEventListener('dblclick', () => {
    off = { x: 0, y: 0 };
    apply();
    save(key, off);
  });

  // Appliqué après insertion dans la page, pour que les bornes soient connues.
  requestAnimationFrame(apply);
}
