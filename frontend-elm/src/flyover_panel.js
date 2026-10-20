/**
 * Panneau du survol : lecture, vitesse, chiffres du point, profil cliquable,
 * vues prédéfinies et curseurs de caméra. Il ne garde aucun état de lecture :
 * il affiche la vue qu'on lui passe et renvoie les gestes à `actions`.
 */
import { createGrip, makeDraggable } from './draggable.js';
import { elevationAt, slopeAt } from './flyover_track.js';
import {
  PRESETS,
  ELEVATION_MIN_DEG,
  ELEVATION_MAX_DEG,
  DISTANCE_SLIDER_MAX,
  FREE_CAMERA_ID,
  distanceToSlider,
  sliderToDistance
} from './flyover_camera.js';
import { createProfileView } from './flyover_profile.js';

const KM = 1000;
const DRAG_STORAGE_KEY = 'cn.drag.flyover';
const HINT = 'Molette : distance · clic droit glissé : tourner · ← → : 1 km · espace : pause';

function element(tag, className, text) {
  const node = document.createElement(tag);
  if (className) node.className = className;
  if (text != null) node.textContent = text;
  return node;
}

function button(className, text, { title, onClick }) {
  const node = element('button', className, text);
  node.type = 'button';
  if (title) node.title = title;
  node.onclick = onClick;
  return node;
}

function slider(label, { min, max, onInput }) {
  const wrap = element('label', 'flyover-slider');
  const input = element('input');
  input.type = 'range';
  input.min = min;
  input.max = max;
  input.addEventListener('input', () => onInput(Number(input.value)));
  wrap.append(element('span', null, label), input);
  return { wrap, input };
}

/**
 * actions : { togglePlay, changeSpeed(step), setCamera(changes), seekFraction(fraction), close }
 * view : { isPlaying, speed, positionM, track, stops, camera, colors }
 */
export function createFlyoverPanel(container, actions) {
  const panel = element('div', 'flyover-panel');
  panel.setAttribute('role', 'region');
  panel.setAttribute('aria-label', 'Survol 3D');

  const controls = buildControls(actions);
  const profile = element('canvas', 'flyover-profile');
  profile.title = 'Cliquer ou glisser pour se déplacer';
  profile.addEventListener('pointerdown', event => followProfileDrag(profile, event, actions.seekFraction));
  const presets = buildPresets(actions.setCamera);
  const sliders = buildSliders(actions.setCamera);

  panel.append(
    controls.row,
    profile,
    presets.row,
    sliders.row,
    element('p', 'flyover-hint', HINT)
  );
  container.append(panel);
  makeDraggable(panel, controls.grip, DRAG_STORAGE_KEY);

  const profileView = createProfileView(profile);

  return {
    update: view => showView({ controls, presets, sliders, profileView }, view),
    remove: () => panel.remove()
  };
}

function showView({ controls, presets, sliders, profileView }, view) {
  controls.play.textContent = view.isPlaying ? '⏸' : '▶';
  controls.play.title = view.isPlaying ? 'Pause (espace)' : 'Lecture (espace)';
  controls.speed.textContent = `×${view.speed}`;
  controls.stats.textContent = statsText(view);
  presets.buttons.forEach(([presetId, presetButton]) => {
    presetButton.classList.toggle('active', presetId === view.camera.id);
  });
  sliders.distance.input.value = distanceToSlider(view.camera.distanceM);
  sliders.elevation.input.value = Math.round(view.camera.elevationDeg);
  profileView.render(view);
}

function buildControls(actions) {
  const grip = createGrip();
  const play = button('flyover-play', null, { onClick: actions.togglePlay });
  const slower = button('flyover-speed-btn', '−', { title: 'Plus lent (−)', onClick: () => actions.changeSpeed(-1) });
  const speed = element('span', 'flyover-speed t-metric');
  const faster = button('flyover-speed-btn', '+', { title: 'Plus vite (+)', onClick: () => actions.changeSpeed(1) });
  const stats = element('span', 'flyover-stats t-metric');
  const close = button('flyover-close', '✕', { title: 'Fermer le survol (Échap)', onClick: actions.close });
  const row = element('div', 'flyover-row');
  row.append(grip, play, slower, speed, faster, stats, close);
  return { row, grip, play, speed, stats };
}

function buildPresets(setCamera) {
  const row = element('div', 'flyover-presets');
  const buttons = PRESETS.map(preset => {
    const presetButton = button('flyover-preset', preset.label, { onClick: () => setCamera({ ...preset }) });
    row.append(presetButton);
    return [preset.id, presetButton];
  });
  return { row, buttons };
}

function buildSliders(setCamera) {
  const distance = slider('Distance', {
    min: 0,
    max: DISTANCE_SLIDER_MAX,
    onInput: value => setCamera({ distanceM: sliderToDistance(value), id: FREE_CAMERA_ID })
  });
  const elevation = slider('Hauteur', {
    min: ELEVATION_MIN_DEG,
    max: ELEVATION_MAX_DEG,
    onInput: value => setCamera({ elevationDeg: value, id: FREE_CAMERA_ID })
  });
  const row = element('div', 'flyover-sliders');
  row.append(distance.wrap, elevation.wrap);
  return { row, distance, elevation };
}

function statsText({ track, positionM }) {
  const elevation = elevationAt(track, positionM);
  const slope = slopeAt(track, positionM);
  return [
    `km ${(positionM / KM).toFixed(1)} / ${(track.totalM / KM).toFixed(1)}`,
    elevation != null ? `${Math.round(elevation)} m` : null,
    slope != null ? `${slope > 0 ? '+' : ''}${slope.toFixed(0)} %` : null
  ].filter(Boolean).join(' · ');
}

function followProfileDrag(canvas, startEvent, seekFraction) {
  canvas.setPointerCapture(startEvent.pointerId);
  const fractionOf = event => {
    const bounds = canvas.getBoundingClientRect();
    return Math.min(Math.max((event.clientX - bounds.left) / bounds.width, 0), 1);
  };
  seekFraction(fractionOf(startEvent));
  const onMove = event => seekFraction(fractionOf(event));
  const onUp = () => {
    canvas.removeEventListener('pointermove', onMove);
    canvas.removeEventListener('pointerup', onUp);
  };
  canvas.addEventListener('pointermove', onMove);
  canvas.addEventListener('pointerup', onUp);
}
