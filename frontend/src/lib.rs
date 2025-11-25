use seed::{prelude::*, virtual_dom::AtValue, *};
use serde::Deserialize;
use serde_wasm_bindgen::to_value;
use shared::{Coordinate, RouteRequest, RouteResponse};
use wasm_bindgen::{
    JsCast,
    prelude::{JsValue, wasm_bindgen},
};

#[wasm_bindgen(module = "/maplibre_map.js")]
extern "C" {
    #[wasm_bindgen(js_name = initMap)]
    fn init_map();
    #[wasm_bindgen(js_name = updateRoute)]
    fn update_route_js(coords: JsValue);
    #[wasm_bindgen(js_name = updateSelectionMarkers)]
    fn update_selection_markers(start: JsValue, end: JsValue);
    #[wasm_bindgen(js_name = toggleSatelliteView)]
    fn toggle_satellite_view(enabled: bool);
    #[wasm_bindgen(js_name = updateBbox)]
    fn update_bbox_js(bounds: JsValue);
    #[wasm_bindgen(js_name = toggleThree3DView)]
    fn toggle_three_3d_view(enabled: bool);
    #[wasm_bindgen(js_name = centerOnMarkers)]
    fn center_on_markers(start: JsValue, end: JsValue);
}

fn api_root() -> String {
    if let Some(url) = option_env!("FRONTEND_API_ROOT") {
        return url.trim_end_matches('/').to_string();
    }
    "http://localhost:8080/api/route".to_string()
}

pub struct Model {
    form: RouteForm,
    pending: bool,
    last_response: Option<RouteResponse>,
    error: Option<String>,
    click_mode: ClickMode,
    map_view_mode: MapViewMode,
    view_mode: ViewMode,
    animation_state: AnimationState,
}

#[derive(Clone, Copy, PartialEq, Debug)]
pub enum ClickMode {
    Start,
    End,
}

#[derive(Clone, Copy, PartialEq, Debug)]
pub enum MapViewMode {
    Standard,
    Satellite,
}

#[derive(Clone, Copy, PartialEq, Debug)]
pub enum ViewMode {
    Map2D,
    Map3D,
}

#[derive(Clone, Copy, PartialEq, Debug)]
pub enum AnimationState {
    Stopped,
    Playing,
    Paused,
}

#[derive(Default, Clone)]
struct RouteForm {
    start_lat: String,
    start_lon: String,
    end_lat: String,
    end_lon: String,
    w_pop: String,
    w_paved: String,
}

impl RouteForm {
    fn to_request(&self) -> Result<RouteRequest, String> {
        let parse = |field: &str, label: &str| {
            field
                .trim()
                .parse::<f64>()
                .map_err(|_| format!("Champ {label} invalide"))
        };
        Ok(RouteRequest {
            start: Coordinate {
                lat: parse(&self.start_lat, "lat départ")?,
                lon: parse(&self.start_lon, "lon départ")?,
            },
            end: Coordinate {
                lat: parse(&self.end_lat, "lat arrivée")?,
                lon: parse(&self.end_lon, "lon arrivée")?,
            },
            w_pop: parse(&self.w_pop, "poids densité")?,
            w_paved: parse(&self.w_paved, "poids bitume")?,
        })
    }
}

pub enum Msg {
    StartLatChanged(String),
    StartLonChanged(String),
    EndLatChanged(String),
    EndLonChanged(String),
    PopWeightChanged(String),
    PavedWeightChanged(String),
    Submit,
    SetClickMode(ClickMode),
    ToggleMapView,
    Toggle3DView,
    PlayAnimation,
    PauseAnimation,
    SaveRoute,
    LoadRoute,
    MapClicked { lat: f64, lon: f64 },
    RouteFetched(Result<RouteResponse, String>),
}

pub fn init(_: Url, orders: &mut impl Orders<Msg>) -> Model {
    orders.stream(streams::window_event(Ev::from("map-click"), |event| {
        let event = event
            .dyn_into::<web_sys::CustomEvent>()
            .expect("map-click event must be CustomEvent");
        let detail = event.detail();
        let payload: MapClickPayload = serde_wasm_bindgen::from_value(detail)
            .unwrap_or(MapClickPayload { lat: 0.0, lon: 0.0 });
        web_sys::console::debug_1(
            &format!(
                "[frontend] map click lat={:.5} lon={:.5}",
                payload.lat, payload.lon
            )
            .into(),
        );
        Msg::MapClicked {
            lat: payload.lat,
            lon: payload.lon,
        }
    }));

    let model = Model {
        form: RouteForm {
            // Combefort, Le Bois-d'Oingt (69620)
            start_lat: "45.9305".into(),
            start_lon: "4.5776".into(),
            end_lat: "45.9399".into(),
            end_lon: "4.5757".into(),
            w_pop: "1.5".into(),
            w_paved: "4.0".into(),
        },
        pending: false,
        last_response: None,
        error: None,
        click_mode: ClickMode::Start,
        map_view_mode: MapViewMode::Standard,
        view_mode: ViewMode::Map2D,
        animation_state: AnimationState::Stopped,
    };

    sync_selection_markers(&model.form);

    // Center map on initial start and end coordinates
    if let (Some(start), Some(end)) = (
        model.form.coordinate_pair(&model.form.start_lat, &model.form.start_lon),
        model.form.coordinate_pair(&model.form.end_lat, &model.form.end_lon),
    ) {
        if let (Ok(start_js), Ok(end_js)) = (to_value(&start), to_value(&end)) {
            center_on_markers(start_js, end_js);
        }
    }

    model
}

pub fn update(msg: Msg, model: &mut Model, orders: &mut impl Orders<Msg>) {
    match msg {
        Msg::StartLatChanged(val) => {
            model.form.start_lat = val;
            sync_selection_markers(&model.form);
        }
        Msg::StartLonChanged(val) => {
            model.form.start_lon = val;
            sync_selection_markers(&model.form);
        }
        Msg::EndLatChanged(val) => {
            model.form.end_lat = val;
            sync_selection_markers(&model.form);
        }
        Msg::EndLonChanged(val) => {
            model.form.end_lon = val;
            sync_selection_markers(&model.form);
        }
        Msg::PopWeightChanged(val) => model.form.w_pop = val,
        Msg::PavedWeightChanged(val) => model.form.w_paved = val,
        Msg::Submit => {
            if model.pending {
                return;
            }
            match model.form.to_request() {
                Ok(payload) => {
                    model.pending = true;
                    model.error = None;
                    orders.perform_cmd(send_route_request(payload));
                }
                Err(err) => model.error = Some(err),
            }
        }
        Msg::RouteFetched(result) => {
            model.pending = false;
            match result {
                Ok(route) => {
                    push_route_to_map(&route.path);

                    // Extract start and end points from the route and update form
                    if let (Some(start), Some(end)) = (route.path.first(), route.path.last()) {
                        model.form.start_lat = format_coord(start.lat);
                        model.form.start_lon = format_coord(start.lon);
                        model.form.end_lat = format_coord(end.lat);
                        model.form.end_lon = format_coord(end.lon);
                    }

                    // Update bbox if metadata is present
                    if let Some(ref metadata) = route.metadata
                        && let Ok(bounds_value) = to_value(&metadata.bounds)
                    {
                        update_bbox_js(bounds_value);
                    }

                    // Maplibre handles terrain automatically, no separate 3D update needed
                    model.last_response = Some(route);
                    model.error = None;
                    sync_selection_markers(&model.form);

                    // Center map on start and end markers
                    if let (Some(start), Some(end)) = (
                        model.form.coordinate_pair(&model.form.start_lat, &model.form.start_lon),
                        model.form.coordinate_pair(&model.form.end_lat, &model.form.end_lon),
                    ) {
                        if let (Ok(start_js), Ok(end_js)) = (to_value(&start), to_value(&end)) {
                            center_on_markers(start_js, end_js);
                        }
                    }
                }
                Err(err) => {
                    push_route_to_map(&[]);
                    model.error = Some(err);
                }
            }
        }
        Msg::SetClickMode(mode) => {
            model.click_mode = mode;
        }
        Msg::ToggleMapView => {
            model.map_view_mode = match model.map_view_mode {
                MapViewMode::Standard => MapViewMode::Satellite,
                MapViewMode::Satellite => MapViewMode::Standard,
            };
            toggle_satellite_view(model.map_view_mode == MapViewMode::Satellite);
        }
        Msg::Toggle3DView => {
            model.view_mode = match model.view_mode {
                ViewMode::Map2D => ViewMode::Map3D,
                ViewMode::Map3D => ViewMode::Map2D,
            };
            // Maplibre toggles terrain view directly
            toggle_three_3d_view(model.view_mode == ViewMode::Map3D);
        }
        Msg::PlayAnimation => {
            // Animation not implemented in Maplibre version yet
            model.animation_state = AnimationState::Playing;
        }
        Msg::PauseAnimation => {
            model.animation_state = AnimationState::Stopped;
        }
        Msg::SaveRoute => {
            if let Some(ref route) = model.last_response {
                save_route_to_disk(route);
            }
        }
        Msg::LoadRoute => {
            orders.perform_cmd(async {
                match load_route_from_disk_async().await {
                    Ok(route) => Msg::RouteFetched(Ok(route)),
                    Err(e) => Msg::RouteFetched(Err(e)),
                }
            });
        }
        Msg::MapClicked { lat, lon } => {
            let lat_str = format_coord(lat);
            let lon_str = format_coord(lon);
            web_sys::console::debug_1(
                &format!(
                    "[frontend] MapClicked mode={:?} lat={lat:.5} lon={lon:.5}",
                    model.click_mode
                )
                .into(),
            );
            match model.click_mode {
                ClickMode::Start => {
                    model.form.start_lat = lat_str;
                    model.form.start_lon = lon_str;
                }
                ClickMode::End => {
                    model.form.end_lat = lat_str;
                    model.form.end_lon = lon_str;
                }
            }
            sync_selection_markers(&model.form);
        }
    }
}

async fn send_route_request(payload: RouteRequest) -> Msg {
    web_sys::console::debug_1(
        &format!(
            "[frontend] sending route request start=({:.5},{:.5}) end=({:.5},{:.5})",
            payload.start.lat, payload.start.lon, payload.end.lat, payload.end.lon
        )
        .into(),
    );
    let response = match Request::new(api_root()).method(Method::Post).json(&payload) {
        Err(err) => Err(format!("{err:?}")),
        Ok(request) => match request.fetch().await {
            Err(err) => Err(format!("{err:?}")),
            Ok(raw) => match raw.check_status() {
                Err(status_err) => Err(format!("{status_err:?}")),
                Ok(resp) => match resp.json::<RouteResponse>().await {
                    Ok(route) => Ok(route),
                    Err(err) => Err(format!("{err:?}")),
                },
            },
        },
    };

    Msg::RouteFetched(response)
}

pub fn view(model: &Model) -> Node<Msg> {
    let header = h1!["Chemins Noirs – générateur GPX anti-bitume"];
    let form = view_form(model);
    let preview = view_preview(model);

    div![C!["app-container"], header, form, preview]
}

fn view_form(model: &Model) -> Node<Msg> {
    let input_field = |label: &str, value: &str, msg: fn(String) -> Msg| {
        div![
            C!["input-field"],
            label![label],
            input![
                attrs! {
                    At::Value => value,
                    At::AutoComplete => "off",
                    At::SpellCheck => "false",
                },
                input_ev(Ev::Input, msg),
            ]
        ]
    };

    form![
        C!["controls"],
        fieldset![
            legend!["Points"],
            input_field(
                "Latitude départ",
                &model.form.start_lat,
                Msg::StartLatChanged
            ),
            input_field(
                "Longitude départ",
                &model.form.start_lon,
                Msg::StartLonChanged
            ),
            input_field("Latitude arrivée", &model.form.end_lat, Msg::EndLatChanged),
            input_field("Longitude arrivée", &model.form.end_lon, Msg::EndLonChanged),
        ],
        fieldset![
            legend!["Poids"],
            input_field(
                "Éviter population",
                &model.form.w_pop,
                Msg::PopWeightChanged
            ),
            input_field(
                "Éviter bitume",
                &model.form.w_paved,
                Msg::PavedWeightChanged
            ),
        ],
        fieldset![
            legend!["Sélection via la carte"],
            div![
                C!["click-mode"],
                label![
                    input![
                        attrs! {
                            At::Type => "radio",
                            At::Name => "click-mode",
                            At::Checked => bool_attr(model.click_mode == ClickMode::Start),
                        },
                        ev(Ev::Change, |_| Msg::SetClickMode(ClickMode::Start)),
                    ],
                    span!["Départ"],
                ],
                label![
                    input![
                        attrs! {
                            At::Type => "radio",
                            At::Name => "click-mode",
                            At::Checked => bool_attr(model.click_mode == ClickMode::End),
                        },
                        ev(Ev::Change, |_| Msg::SetClickMode(ClickMode::End)),
                    ],
                    span!["Arrivée"],
                ],
            ],
            small!["Cliquez sur la carte pour remplir la position sélectionnée."],
        ],
        fieldset![
            legend!["Vue de la carte"],
            button![
                match model.map_view_mode {
                    MapViewMode::Standard => "Vue Satellite",
                    MapViewMode::Satellite => "Vue Standard",
                },
                ev(Ev::Click, |event| {
                    event.prevent_default();
                    Msg::ToggleMapView
                }),
                C!["map-toggle"],
            ],
        ],
        if model.view_mode == ViewMode::Map3D && model.last_response.is_some() {
            let anim_state = model.animation_state;
            fieldset![
                legend!["Animation 3D"],
                button![
                    match anim_state {
                        AnimationState::Stopped | AnimationState::Paused => "▶ Lire",
                        AnimationState::Playing => "⏸ Pause",
                    },
                    ev(Ev::Click, move |event| {
                        event.prevent_default();
                        match anim_state {
                            AnimationState::Stopped | AnimationState::Paused => Msg::PlayAnimation,
                            AnimationState::Playing => Msg::PauseAnimation,
                        }
                    }),
                    C!["animation-toggle"],
                ],
            ]
        } else {
            empty![]
        },
        if model.last_response.is_some() {
            fieldset![
                legend!["Sauvegarder/Charger"],
                button![
                    "💾 Sauvegarder",
                    ev(Ev::Click, |event| {
                        event.prevent_default();
                        Msg::SaveRoute
                    }),
                    C!["save-btn"],
                ],
                button![
                    "📂 Charger",
                    ev(Ev::Click, |event| {
                        event.prevent_default();
                        Msg::LoadRoute
                    }),
                    C!["load-btn"],
                ],
                small!["Le tracé est sauvegardé localement dans votre navigateur."],
            ]
        } else {
            button![
                "📂 Charger tracé sauvegardé",
                ev(Ev::Click, |event| {
                    event.prevent_default();
                    Msg::LoadRoute
                }),
                C!["load-btn"],
            ]
        },
        button![
            "Tracer l'itinéraire",
            ev(Ev::Click, |event| {
                event.prevent_default();
                Msg::Submit
            }),
            attrs! { At::Disabled => bool_attr(model.pending) },
        ],
        if let Some(error) = &model.error {
            p![C!["error"], error]
        } else {
            empty![]
        }
    ]
}

fn view_preview(model: &Model) -> Node<Msg> {
    if let Some(route) = &model.last_response {
        let stats = div![
            C!["stats"],
            h2!["Dernier tracé"],
            p![format!("{:.2} km parcourus", route.distance_km)],
            small!["Téléchargez le GPX via l'API (payload base64)"],
        ];

        let path_points = route.path.iter().enumerate().map(|(idx, coord)| {
            let elevation = route
                .elevation_profile
                .as_ref()
                .and_then(|profile| profile.elevations.get(idx).cloned().flatten());

            li![format!(
                "{idx}: {:.5} / {:.5}{}",
                coord.lat,
                coord.lon,
                elevation
                    .map(|e| format!(" — {:.1} m", e))
                    .unwrap_or_else(|| "".to_string())
            )]
        });

        let path_list = ul![C!["path-preview"], path_points];

        let metadata = route
            .metadata
            .as_ref()
            .map(view_metadata)
            .unwrap_or_else(|| empty![]);

        let elevation = route
            .elevation_profile
            .as_ref()
            .map(view_elevation_profile)
            .unwrap_or_else(|| empty![]);

        div![C!["preview"], stats, metadata, elevation, path_list]
    } else {
        div![
            C!["preview"],
            h2!["En attente"],
            p!["Soumettez des points pour visualiser un itinéraire."]
        ]
    }
}

fn view_metadata(meta: &shared::RouteMetadata) -> Node<Msg> {
    let card = |label: &str, content: String| {
        div![
            C!["metadata-card"],
            span![C!["label"], label],
            strong![content],
        ]
    };

    div![
        C!["metadata-grid"],
        card("Points", meta.point_count.to_string()),
        card(
            "Départ",
            format!("{:.4} / {:.4}", meta.start.lat, meta.start.lon)
        ),
        card(
            "Arrivée",
            format!("{:.4} / {:.4}", meta.end.lat, meta.end.lon)
        ),
        card(
            "BBox",
            format!(
                "[{:.3}↔{:.3}] lat / [{:.3}↔{:.3}] lon",
                meta.bounds.min_lat, meta.bounds.max_lat, meta.bounds.min_lon, meta.bounds.max_lon
            )
        ),
    ]
}

#[wasm_bindgen(start)]
pub fn start() {
    init_map();
    App::start("app", init, update, view);
}

fn push_route_to_map(path: &[Coordinate]) {
    if let Ok(value) = to_value(path) {
        update_route_js(value);
    }
}

fn sync_selection_markers(form: &RouteForm) {
    let start = form
        .coordinate_pair(&form.start_lat, &form.start_lon)
        .and_then(|coord| to_value(&coord).ok())
        .unwrap_or(JsValue::NULL);
    let end = form
        .coordinate_pair(&form.end_lat, &form.end_lon)
        .and_then(|coord| to_value(&coord).ok())
        .unwrap_or(JsValue::NULL);
    update_selection_markers(start, end);
}

impl RouteForm {
    fn coordinate_pair(&self, lat: &str, lon: &str) -> Option<Coordinate> {
        let lat = lat.trim().parse::<f64>().ok()?;
        let lon = lon.trim().parse::<f64>().ok()?;
        Some(Coordinate { lat, lon })
    }
}

fn bool_attr(value: bool) -> AtValue {
    if value {
        AtValue::Some("true".into())
    } else {
        AtValue::Ignored
    }
}

fn format_coord(value: f64) -> String {
    format!("{value:.5}")
}

fn view_elevation_profile(profile: &shared::ElevationProfile) -> Node<Msg> {
    let card = |label: &str, content: String| {
        div![
            C!["metadata-card"],
            span![C!["label"], label],
            strong![content],
        ]
    };

    let elevation_stats = div![
        C!["metadata-grid"],
        card("Dénivelé +", format!("{:.0} m", profile.total_ascent)),
        card("Dénivelé -", format!("{:.0} m", profile.total_descent)),
        card(
            "Altitude min",
            profile
                .min_elevation
                .map(|e| format!("{:.0} m", e))
                .unwrap_or_else(|| "N/A".to_string())
        ),
        card(
            "Altitude max",
            profile
                .max_elevation
                .map(|e| format!("{:.0} m", e))
                .unwrap_or_else(|| "N/A".to_string())
        ),
    ];

    div![
        C!["elevation-section"],
        h3!["Profil d'élévation"],
        elevation_stats
    ]
}

#[derive(Deserialize)]
struct MapClickPayload {
    lat: f64,
    lon: f64,
}

// Save route to disk via API
fn save_route_to_disk(route: &RouteResponse) {
    let route_clone = route.clone();
    spawn_local(async move {
        match Request::new("http://localhost:8080/api/routes/save")
            .method(Method::Post)
            .json(&route_clone)
        {
            Err(err) => {
                web_sys::console::error_1(&format!("Failed to build request: {:?}", err).into());
            }
            Ok(request) => match request.fetch().await {
                Err(err) => {
                    web_sys::console::error_1(&format!("Failed to save route: {:?}", err).into());
                }
                Ok(_) => {
                    web_sys::console::log_1(&"Route sauvegardée sur le disque".into());
                }
            },
        }
    });
}

// Load route from disk via API (async function)
async fn load_route_from_disk_async() -> Result<RouteResponse, String> {
    let request = Request::new("http://localhost:8080/api/routes/load").method(Method::Get);
    match request.fetch().await {
        Err(err) => Err(format!("Failed to fetch: {:?}", err)),
        Ok(raw) => match raw.check_status() {
            Err(status_err) => Err(format!("Status error: {:?}", status_err)),
            Ok(resp) => match resp.json::<RouteResponse>().await {
                Ok(route) => {
                    web_sys::console::log_1(&"Route chargée depuis le disque".into());
                    Ok(route)
                }
                Err(err) => Err(format!("Failed to parse JSON: {:?}", err)),
            },
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_view_mode_toggle() {
        assert_eq!(ViewMode::Map2D, ViewMode::Map2D);
        assert_ne!(ViewMode::Map2D, ViewMode::Map3D);

        let mode_2d = ViewMode::Map2D;
        let toggled = match mode_2d {
            ViewMode::Map2D => ViewMode::Map3D,
            ViewMode::Map3D => ViewMode::Map2D,
        };
        assert_eq!(toggled, ViewMode::Map3D);
    }

    #[test]
    fn test_animation_state_transitions() {
        let stopped = AnimationState::Stopped;
        let playing = AnimationState::Playing;
        let paused = AnimationState::Paused;

        assert_eq!(stopped, AnimationState::Stopped);
        assert_ne!(stopped, playing);
        assert_ne!(playing, paused);
    }

    #[test]
    fn test_click_mode_toggle() {
        assert_eq!(ClickMode::Start, ClickMode::Start);
        assert_ne!(ClickMode::Start, ClickMode::End);
    }

    #[test]
    fn test_map_view_mode_toggle() {
        let standard = MapViewMode::Standard;
        let satellite = MapViewMode::Satellite;

        assert_eq!(standard, MapViewMode::Standard);
        assert_ne!(standard, satellite);
    }

    #[test]
    fn test_route_form_to_request_valid() {
        let form = RouteForm {
            start_lat: "45.93".to_string(),
            start_lon: "4.577".to_string(),
            end_lat: "45.94".to_string(),
            end_lon: "4.575".to_string(),
            w_pop: "1.0".to_string(),
            w_paved: "1.0".to_string(),
        };

        let request = form.to_request();
        assert!(request.is_ok(), "Expected Ok, got: {:?}", request);
        let req = request.unwrap();
        assert_eq!(req.start.lat, 45.93);
        assert_eq!(req.start.lon, 4.577);
        assert_eq!(req.end.lat, 45.94);
        assert_eq!(req.end.lon, 4.575);
        assert_eq!(req.w_pop, 1.0);
        assert_eq!(req.w_paved, 1.0);
    }

    #[test]
    fn test_route_form_to_request_invalid_coords() {
        let form = RouteForm {
            start_lat: "45.93".to_string(),
            start_lon: "invalid".to_string(),
            end_lat: "45.94".to_string(),
            end_lon: "4.575".to_string(),
            w_pop: "1.0".to_string(),
            w_paved: "1.0".to_string(),
        };

        let request = form.to_request();
        assert!(request.is_err(), "Expected error for invalid coordinate");
    }
}
