use std::{
    fs::File,
    io::{BufRead, BufReader},
    path::{Path, PathBuf},
};

use thiserror::Error;

#[derive(Debug, Error)]
pub enum DemLoadError {
    #[error("failed to open DEM file {path:?}: {source}")]
    Io {
        source: std::io::Error,
        path: PathBuf,
    },
    #[error("DEM file missing header field `{0}`")]
    MissingHeader(&'static str),
    #[error("DEM file has invalid numeric header for `{field}`: {source}")]
    InvalidHeader {
        field: &'static str,
        #[source]
        source: std::num::ParseFloatError,
    },
    #[error("DEM file has invalid integer header for `{field}`: {source}")]
    InvalidHeaderInt {
        field: &'static str,
        #[source]
        source: std::num::ParseIntError,
    },
    #[error("DEM grid has {expected} cells but file provided {actual}")]
    UnexpectedCellCount { expected: usize, actual: usize },
}

#[derive(Debug)]
pub struct ArcAsciiDem {
    ncols: usize,
    nrows: usize,
    xllcorner: f64,
    yllcorner: f64,
    cellsize: f64,
    nodata: f64,
    lat_max: f64,
    lon_max: f64,
    values: Vec<f64>,
}

impl ArcAsciiDem {
    pub fn from_path(path: impl AsRef<Path>) -> Result<Self, DemLoadError> {
        let path = path.as_ref();
        let file = File::open(path).map_err(|source| DemLoadError::Io {
            source,
            path: path.into(),
        })?;
        let mut reader = BufReader::new(file);
        let mut header_buf = String::new();

        let mut ncols = None;
        let mut nrows = None;
        let mut xllcorner = None;
        let mut yllcorner = None;
        let mut cellsize = None;
        let mut nodata = None;

        for _ in 0..6 {
            header_buf.clear();
            if reader.read_line(&mut header_buf).is_err() {
                break;
            }
            let mut parts = header_buf.split_whitespace();
            let key = match parts.next() {
                Some(k) => k.to_lowercase(),
                None => continue,
            };
            let value = match parts.next() {
                Some(v) => v,
                None => continue,
            };

            match key.as_str() {
                "ncols" => {
                    ncols =
                        Some(
                            value
                                .parse()
                                .map_err(|source| DemLoadError::InvalidHeaderInt {
                                    field: "ncols",
                                    source,
                                })?,
                        )
                }
                "nrows" => {
                    nrows =
                        Some(
                            value
                                .parse()
                                .map_err(|source| DemLoadError::InvalidHeaderInt {
                                    field: "nrows",
                                    source,
                                })?,
                        )
                }
                "xllcorner" | "xllcenter" => {
                    xllcorner =
                        Some(
                            value
                                .parse()
                                .map_err(|source| DemLoadError::InvalidHeader {
                                    field: "xllcorner",
                                    source,
                                })?,
                        )
                }
                "yllcorner" | "yllcenter" => {
                    yllcorner =
                        Some(
                            value
                                .parse()
                                .map_err(|source| DemLoadError::InvalidHeader {
                                    field: "yllcorner",
                                    source,
                                })?,
                        )
                }
                "cellsize" => {
                    cellsize =
                        Some(
                            value
                                .parse()
                                .map_err(|source| DemLoadError::InvalidHeader {
                                    field: "cellsize",
                                    source,
                                })?,
                        )
                }
                "nodata_value" => {
                    nodata = Some(
                        value
                            .parse()
                            .map_err(|source| DemLoadError::InvalidHeader {
                                field: "nodata_value",
                                source,
                            })?,
                    )
                }
                _ => {}
            };
        }

        let ncols = ncols.ok_or(DemLoadError::MissingHeader("ncols"))?;
        let nrows = nrows.ok_or(DemLoadError::MissingHeader("nrows"))?;
        let xllcorner = xllcorner.ok_or(DemLoadError::MissingHeader("xllcorner"))?;
        let yllcorner = yllcorner.ok_or(DemLoadError::MissingHeader("yllcorner"))?;
        let cellsize = cellsize.ok_or(DemLoadError::MissingHeader("cellsize"))?;
        let nodata = nodata.unwrap_or(-9999.0);

        let lat_max = yllcorner + cellsize * ((nrows - 1) as f64);
        let lon_max = xllcorner + cellsize * ((ncols - 1) as f64);

        let mut values = Vec::with_capacity(ncols * nrows);
        for line in reader.lines() {
            let line = line.map_err(|source| DemLoadError::Io {
                source,
                path: path.into(),
            })?;
            for token in line.split_whitespace() {
                if token.is_empty() {
                    continue;
                }
                let value = token
                    .parse::<f64>()
                    .map_err(|source| DemLoadError::InvalidHeader {
                        field: "value",
                        source,
                    })?;
                values.push(value);
            }
        }

        let expected = ncols * nrows;
        if values.len() != expected {
            return Err(DemLoadError::UnexpectedCellCount {
                expected,
                actual: values.len(),
            });
        }

        Ok(Self {
            ncols,
            nrows,
            xllcorner,
            yllcorner,
            cellsize,
            nodata,
            lat_max,
            lon_max,
            values,
        })
    }

    pub fn sample(&self, lat: f64, lon: f64) -> Option<f64> {
        // Transform WGS84 (lat/lon) to Lambert 93 if coordinates look like lat/lon
        let (x, y) = if lon.abs() < 180.0 && lat.abs() < 90.0 {
            // These are lat/lon coordinates, transform to Lambert 93
            wgs84_to_lambert93(lat, lon)?
        } else {
            // Already in projected coordinates
            (lon, lat)
        };

        if x < self.xllcorner || x > self.lon_max || y < self.yllcorner || y > self.lat_max {
            return None;
        }
        let col = ((x - self.xllcorner) / self.cellsize).clamp(0.0, (self.ncols - 1) as f64);
        let row = ((self.lat_max - y) / self.cellsize).clamp(0.0, (self.nrows - 1) as f64);

        let x0 = col.floor() as usize;
        let y0 = row.floor() as usize;
        let x1 = (x0 + 1).min(self.ncols - 1);
        let y1 = (y0 + 1).min(self.nrows - 1);

        let q11 = self.value(y0, x0);
        let q21 = self.value(y0, x1);
        let q12 = self.value(y1, x0);
        let q22 = self.value(y1, x1);

        let tx = col - x0 as f64;
        let ty = row - y0 as f64;

        match (q11, q21, q12, q22) {
            (Some(a), Some(b), Some(c), Some(d)) => {
                let top = a * (1.0 - tx) + b * tx;
                let bottom = c * (1.0 - tx) + d * tx;
                Some(top * (1.0 - ty) + bottom * ty)
            }
            _ => {
                let mut sum = 0.0;
                let mut count = 0;
                for val in [q11, q21, q12, q22] {
                    if let Some(v) = val {
                        sum += v;
                        count += 1;
                    }
                }
                if count > 0 {
                    Some(sum / count as f64)
                } else {
                    None
                }
            }
        }
    }

    fn value(&self, row: usize, col: usize) -> Option<f64> {
        if row >= self.nrows || col >= self.ncols {
            return None;
        }
        let idx = row * self.ncols + col;
        let value = self.values.get(idx).copied()?;
        if (value - self.nodata).abs() < f64::EPSILON {
            None
        } else {
            Some(value)
        }
    }
}

/// Transform WGS84 (EPSG:4326) coordinates to Lambert 93 (EPSG:2154)
fn wgs84_to_lambert93(lat: f64, lon: f64) -> Option<(f64, f64)> {
    use std::cell::RefCell;
    thread_local! {
        static PROJ: RefCell<Option<proj::Proj>> = RefCell::new(None);
    }

    PROJ.with(|proj_cell| {
        let mut proj = proj_cell.borrow_mut();
        if proj.is_none() {
            match proj::Proj::new_known_crs("EPSG:4326", "EPSG:2154", None) {
                Ok(p) => *proj = Some(p),
                Err(e) => {
                    tracing::error!("Failed to create projection: {}", e);
                    return None;
                }
            }
        }

        if let Some(p) = proj.as_ref() {
            // proj expects (lon, lat) order for geographic coordinates
            match p.convert((lon, lat)) {
                Ok((x, y)) => Some((x, y)),
                Err(e) => {
                    tracing::warn!("Failed to transform coordinates ({}, {}): {}", lat, lon, e);
                    None
                }
            }
        } else {
            None
        }
    })
}
