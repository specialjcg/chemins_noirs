-- Migration: Create saved_routes table
-- Description: Store user-saved routes with metadata
-- Author: Claude Code
-- Date: 2025-01-28

CREATE TABLE IF NOT EXISTS saved_routes (
    id SERIAL PRIMARY KEY,
    name TEXT NOT NULL,
    description TEXT,
    created_at TIMESTAMPTZ NOT NULL DEFAULT CURRENT_TIMESTAMP,
    updated_at TIMESTAMPTZ NOT NULL DEFAULT CURRENT_TIMESTAMP,

    -- Route statistics
    distance_km REAL NOT NULL,
    total_ascent_m REAL,
    total_descent_m REAL,

    -- Route data (full JSON)
    route_data JSONB NOT NULL,

    -- GPX export (base64 encoded)
    gpx_data TEXT,

    -- User metadata
    is_favorite BOOLEAN DEFAULT FALSE,
    tags TEXT[] DEFAULT '{}',

    -- Indexing for performance
    CONSTRAINT saved_routes_distance_check CHECK (distance_km >= 0),
    CONSTRAINT saved_routes_name_not_empty CHECK (length(trim(name)) > 0)
);

-- Indexes for query performance
CREATE INDEX IF NOT EXISTS idx_saved_routes_created_at ON saved_routes(created_at DESC);
CREATE INDEX IF NOT EXISTS idx_saved_routes_name ON saved_routes(name);
CREATE INDEX IF NOT EXISTS idx_saved_routes_favorite ON saved_routes(is_favorite) WHERE is_favorite = TRUE;
CREATE INDEX IF NOT EXISTS idx_saved_routes_tags ON saved_routes USING GIN(tags);

-- Trigger to update updated_at automatically
CREATE OR REPLACE FUNCTION update_updated_at_column()
RETURNS TRIGGER AS $$
BEGIN
    NEW.updated_at = CURRENT_TIMESTAMP;
    RETURN NEW;
END;
$$ language 'plpgsql';

-- Drop trigger if exists to make migration idempotent
DROP TRIGGER IF EXISTS update_saved_routes_updated_at ON saved_routes;

CREATE TRIGGER update_saved_routes_updated_at
    BEFORE UPDATE ON saved_routes
    FOR EACH ROW
    EXECUTE FUNCTION update_updated_at_column();

-- Add original waypoints column (for multi-point route re-tracing)
-- This is idempotent and backward compatible (allows NULL for existing routes)
ALTER TABLE saved_routes
ADD COLUMN IF NOT EXISTS original_waypoints JSONB;

-- Comments for documentation
COMMENT ON TABLE saved_routes IS 'Stores user-saved routes with full metadata and statistics';
COMMENT ON COLUMN saved_routes.route_data IS 'Full RouteResponse JSON (path, metadata, elevation profile)';
COMMENT ON COLUMN saved_routes.gpx_data IS 'Base64-encoded GPX file for GPS devices';
COMMENT ON COLUMN saved_routes.tags IS 'User-defined tags for categorization (e.g., ["hiking", "alpine"])';
COMMENT ON COLUMN saved_routes.original_waypoints IS 'Original waypoints for multi-point routes (NULL for point-to-point routes)';
