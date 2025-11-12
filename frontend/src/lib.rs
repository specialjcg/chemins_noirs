use std::future::Future;

use seed::{prelude::*, *};
use serde_wasm_bindgen::to_value;
use shared::{Coordinate, RouteRequest, RouteResponse};
use wasm_bindgen::prelude::{JsValue, wasm_bindgen};

#[wasm_bindgen(module = "/map.js")]
extern "C" {
    #[wasm_bindgen(js_name = initMap)]
    fn init_map();
    #[wasm_bindgen(js_name = updateRoute)]
    fn update_route_js(coords: JsValue);
}

const API_ROOT: &str = "/api/route";

pub struct Model {
    form: RouteForm,
    pending: bool,
    last_response: Option<RouteResponse>,
    error: Option<String>,
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
    RouteFetched(Result<RouteResponse, String>),
}

pub fn init(_: Url, _orders: &mut impl Orders<Msg>) -> Model {
    Model {
        form: RouteForm {
            start_lat: "45.0005".into(),
            start_lon: "5.0005".into(),
            end_lat: "45.024".into(),
            end_lon: "5.034".into(),
            w_pop: "1.5".into(),
            w_paved: "4.0".into(),
        },
        pending: false,
        last_response: None,
        error: None,
    }
}

pub fn update(msg: Msg, model: &mut Model, orders: &mut impl Orders<Msg>) {
    match msg {
        Msg::StartLatChanged(val) => model.form.start_lat = val,
        Msg::StartLonChanged(val) => model.form.start_lon = val,
        Msg::EndLatChanged(val) => model.form.end_lat = val,
        Msg::EndLonChanged(val) => model.form.end_lon = val,
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
                    model.last_response = Some(route);
                    model.error = None;
                }
                Err(err) => {
                    push_route_to_map(&[]);
                    model.error = Some(err);
                }
            }
        }
    }
}

fn send_route_request(payload: RouteRequest) -> impl Future<Output = Msg> {
    async move {
        let response = match Request::new(API_ROOT).method(Method::Post).json(&payload) {
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
            input![attrs! { At::Value => value }, input_ev(Ev::Input, msg),]
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
        button![
            "Tracer l'itinéraire",
            ev(Ev::Click, |event| {
                event.prevent_default();
                Msg::Submit
            }),
            attrs! { At::Disabled => model.pending.as_at_value() },
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

        let path_points = route
            .path
            .iter()
            .enumerate()
            .map(|(idx, coord)| li![format!("{idx}: {:.5} / {:.5}", coord.lat, coord.lon)]);

        let path_list = ul![C!["path-preview"], path_points];

        let metadata = route
            .metadata
            .as_ref()
            .map(view_metadata)
            .unwrap_or_else(|| empty![]);

        div![C!["preview"], stats, metadata, path_list]
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

fn push_route_to_map(path: &[Coordinate]) {
    if let Ok(value) = to_value(path) {
        update_route_js(value);
    }
}

#[wasm_bindgen(start)]
pub fn start() {
    init_map();
    App::start("app", init, update, view);
}
