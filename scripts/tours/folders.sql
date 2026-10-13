-- Classement initial des itinéraires en dossiers (une fois).
ALTER TABLE saved_routes ADD COLUMN IF NOT EXISTS folder TEXT;

UPDATE saved_routes SET folder = CASE
    WHEN 'tour-forets-nord-est' = ANY(tags) THEN 'Tours forêts — Nord-Est'
    WHEN 'tour-forets-nord-idf' = ANY(tags) THEN 'Tours forêts — Nord / IDF'
    WHEN 'tour-forets-ouest'    = ANY(tags) THEN 'Tours forêts — Ouest'
    WHEN 'tour-forets-centre'   = ANY(tags) THEN 'Tours forêts — Centre'
    WHEN 'tour-forets-sud'      = ANY(tags) THEN 'Tours forêts — Sud'
    WHEN id IN (19, 20, 21)     THEN 'Sologne / GR31'
    WHEN id BETWEEN 29 AND 32   THEN 'Étapes GR forêts'
    WHEN id IN (40, 41, 47, 48, 51, 52, 54, 55) THEN 'Traversées forêts'
    WHEN id IN (11, 12, 14, 18) THEN 'Chemins noirs'
END
WHERE folder IS NULL;
