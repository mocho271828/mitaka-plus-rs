//! ホストの数値IDとsnapshotキーは Mitaka_plus f9fe624 の ScriptController.h / ScriptWorker.mm に従う。
use mlua::{Lua, Result};

pub const AU_PER_PC: f64 = 648000.0 / std::f64::consts::PI;

#[derive(Clone, Copy, PartialEq)]
pub enum Scalar {
    Bool,
    Text,
    Finite,
    NonNegative,
    FieldOfView,
    Distance,
    Index(i32),
    Speed,
}
use Scalar::*;

pub struct Field {
    pub object: &'static str,
    pub name: &'static str,
    pub key: &'static str,
    pub action: u32,
    pub scalar: Scalar,
    pub toggle: Option<&'static str>,
    pub writable: bool,
}
const fn flag(
    object: &'static str,
    name: &'static str,
    toggle: &'static str,
    key: &'static str,
    action: u32,
    writable: bool,
) -> Field {
    Field {
        object,
        name,
        key,
        action,
        scalar: Bool,
        toggle: Some(toggle),
        writable,
    }
}
const fn field(
    object: &'static str,
    name: &'static str,
    key: &'static str,
    action: u32,
    scalar: Scalar,
) -> Field {
    Field {
        object,
        name,
        key,
        action,
        scalar,
        toggle: None,
        writable: true,
    }
}

#[rustfmt::skip]
pub const FIELDS: &[Field] = &[
    flag("Asteroids", "visible", "toggle", "asteroids_visible", 44, true),
    flag("Asteroids", "labels", "toggleLabels", "asteroids_labels", 45, true),
    flag("Asteroids", "orbits", "toggleOrbits", "asteroids_orbits", 46, true),
    flag("Asteroids", "selectedOnly", "toggleSelectedOnly", "asteroids_selectedOnly", 47, true),
    flag("TNObjects", "visible", "toggle", "tnobjects_visible", 48, true),
    flag("TNObjects", "labels", "toggleLabels", "tnobjects_labels", 49, true),
    flag("TNObjects", "orbits", "toggleOrbits", "tnobjects_orbits", 50, true),
    flag("TNObjects", "selectedOnly", "toggleSelectedOnly", "tnobjects_selectedOnly", 51, true),
    flag("Comets", "visible", "toggle", "comets_visible", 52, true),
    flag("Comets", "labels", "toggleLabels", "comets_labels", 53, true),
    flag("Comets", "orbits", "toggleOrbits", "comets_orbits", 54, true),
    flag("Comets", "selectedOnly", "toggleSelectedOnly", "comets_selectedOnly", 55, true),
    flag("Satellites", "visible", "toggle", "satellites_visible", 56, true),
    flag("Satellites", "labels", "toggleLabels", "satellites_labels", 57, true),
    flag("Satellites", "orbits", "toggleOrbits", "satellites_orbits", 58, true),
    flag("Satellites", "selectedOnly", "toggleSelectedOnly", "satellites_selectedOnly", 59, true),
    flag("Time", "realtime", "toggleRealtime", "realtime", 19, true),
    flag("Time", "timeInfo", "toggleTimeInfo", "timeInfo", 20, true),
    flag("Constels", "lines", "toggleLines", "lines", 5, true),
    flag("Constels", "bounds", "toggleBounds", "bounds", 6, true),
    flag("Constels", "labels", "toggleLabels", "labels", 7, true),
    flag("Constels", "illusts", "toggleIllusts", "illusts", 65, true),
    flag("Stars", "labels", "toggleLabels", "starLabels", 8, true),
    flag("Stars", "selectedLabels", "toggleSelectedLabels", "selectedStarLabels", 66, true),
    flag("Stars", "visible", "toggle", "starsVisible", 23, true),
    flag("Stars", "bayerLabels", "toggleBayerLabels", "bayerLabels", 24, true),
    flag("Stars", "flamsteedLabels", "toggleFlamsteedLabels", "flamsteedLabels", 25, true),
    flag("Stars", "hipLabels", "toggleHipLabels", "hipLabels", 26, true),
    flag("Planets", "visible", "toggle", "planetsVisible", 27, true),
    flag("Planets", "labels", "toggleLabels", "planetLabels", 28, true),
    flag("Planets", "orbits", "toggleOrbits", "planetOrbits", 29, true),
    flag("Planets", "axes", "toggleAxes", "planetAxes", 30, true),
    flag("Planets", "clouds", "toggleClouds", "earthClouds", 69, true),
    flag("Planets", "cityLights", "toggleCityLights", "earthCityLights", 70, true),
    flag("Planets", "shadows", "toggleShadows", "shadows", 79, true),
    flag("Planets", "shadowBorders", "toggleShadowBorders", "shadowBorders", 80, true),
    flag("Planets", "shadowUmbra", "toggleShadowUmbra", "shadowUmbra", 81, true),
    flag("Planets", "shadowPenumbra", "toggleShadowPenumbra", "shadowPenumbra", 82, true),
    flag("Places", "landscape", "toggleLandscape", "landscape", 83, true),
    flag("Grids", "equatorial", "toggleEquatorial", "equatorial", 31, true),
    flag("Grids", "ecliptic", "toggleEcliptic", "ecliptic", 32, true),
    flag("Grids", "galactic", "toggleGalactic", "galactic", 33, true),
    flag("Grids", "horizontal", "toggleHorizontal", "horizontal", 34, true),
    flag("Grids", "circleScales", "toggleCircleScales", "circleScales", 35, true),
    flag("Grids", "squareScales", "toggleSquareScales", "squareScales", 36, true),
    flag("Renderer", "antialias", "toggleAntialias", "antialias", 37, true),
    flag("Renderer", "pointSmooth", "togglePointSmooth", "pointSmooth", 38, true),
    flag("Renderer", "lineSmooth", "toggleLineSmooth", "lineSmooth", 39, true),
    flag("Renderer", "textSmooth", "toggleTextSmooth", "textSmooth", 40, true),
    flag("Galaxies", "NGCs", "toggleNGCs", "nebulae", 9, true),
    flag("Galaxies", "distantGalaxies", "toggleDistantGalaxies", "galaxies", 10, true),
    flag("Galaxies", "SDSS", "toggleSDSS", "galaxies", 10, true),
    flag("Galaxies", "quasars", "toggleQuasars", "quasars", 11, true),
    flag("Galaxies", "QSOs", "toggleQSOs", "quasars", 11, true),
    flag("GUI", "targetBrowser", "toggleTargetBrowser", "targetBrowser", 13, true),
    flag("GUI", "timeSetting", "toggleTimeSetting", "timeSetting", 14, true),
    flag("GUI", "placeSetting", "togglePlaceSetting", "placeSetting", 15, true),
    flag("GUI", "scriptOutput", "toggleScriptOutput", "scriptOutput", 16, true),
    flag("GUI", "dualScreen", "toggleDualScreen", "dualScreen", 17, true),
    field("Time", "JD", "jd", 0, Finite),
    field("Stars","markingType","starMarking",22,Text),
    field("Stars","brightness","starBrightness",60,Finite),
    field("Time", "step", "timeStep", 72, Index(11)),
    field("Time", "stepValue", "timeStepValue", 73, Finite),
    field("Time", "speed", "timeSpeed", 74, Speed),
    field("Camera", "target", "target", 3, Text),
    field("Camera", "duration", "cameraDuration", 91, NonNegative),
    field("Camera", "modeDuration", "cameraModeDuration", 101, NonNegative),
    field("Camera", "mode", "cameraMode", 100, Index(1)),
    field("Camera", "zoomMode", "cameraZoomMode", 102, Index(2)),
    field("Camera", "zoom", "distance", 4, Distance),
    field("Camera", "fov", "cameraFov", 104, FieldOfView),
    field("Camera", "azimuth", "azimuth", 12, Finite),
    field("Camera", "altitude", "altitude", 89, Finite),
    field("Planets", "atmosphere", "atmosphereMode", 71, Index(2)),
    field("GUI", "fullScreen", "fullScreen", 41, Bool),
    field("Targets", "selectMode", "targetSelectionMode", 106, Index(2)),
];

#[rustfmt::skip]
pub const METHODS: &[(&str, &str, u32)] = &[
    ("Time", "stop", 2),
    ("Time", "forward", 77),
    ("Time", "rewind", 78),
    ("Time", "prevStep", 75),
    ("Time", "nextStep", 76),
    ("GUI", "saveImageDialog", 18),
    ("GUI", "toggleFullScreen", 42),
    ("GUI", "aboutDialog", 43),
    ("Stars", "brighten", 61),
    ("Stars", "darken", 62),
    ("Stars", "defaultBrightness", 63),
];

pub fn constants(lua: &Lua) -> Result<()> {
    let globals = lua.globals();
    let mtk = lua.create_table()?;
    for (key, value) in [
        ("UNIT_AU", 1.0 / AU_PER_PC),
        ("UNIT_KM", 1.0 / (AU_PER_PC * 149597870.7)),
        ("UNIT_LY", 63241.07708426628 / AU_PER_PC),
    ] {
        mtk.set(key, value)?;
        globals.set(key, value)?;
    }
    for (i, name) in [
        "STEP_SEC",
        "STEP_10SEC",
        "STEP_MIN",
        "STEP_10MIN",
        "STEP_HOUR",
        "STEP_DAY",
        "STEP_WEEK",
        "STEP_MONTH",
        "STEP_YEAR",
        "STEP_10YEAR",
        "STEP_100YEAR",
        "STEP_VALUE",
    ]
    .iter()
    .enumerate()
    {
        mtk.set(*name, i)?;
        globals.set(*name, i)?;
    }
    for (key, value) in [
        ("PLANETARIUM", 0),
        ("HIPPARCOS", 1),
        ("KEEP_RADIUS", 0),
        ("KEEP_ORBIT", 1),
        ("ATMOS_NONE", 0),
        ("ATMOS_SINGLE", 1),
        ("ATMOS_MULTIPLE", 2),
        ("ZOOM_NONE", 0),
        ("ZOOM_AUTO", 1),
        ("ZOOM_ALWAYS", 2),
        ("SELECT_NONE", 0),
        ("SELECT_MOUSEOVER", 1),
        ("SELECT_MOUSECLICK", 2),
    ] {
        mtk.set(key, value)?;
    }
    globals.set("mtk", mtk)
}
