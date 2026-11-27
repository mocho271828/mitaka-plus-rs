//! 利用者の天文データを変更せず、合成地形でメニューからの離着陸を検査する。
use crate::{commands::Command, projection::View, solar::RADII_KM, surface::terrain_file, App};
use eframe::egui;
use mitaka_core::{satellite_model::KM_PER_AU, terrain::Amplification};
use std::{
    fs, io,
    path::Path,
    time::{Duration, Instant},
};

pub(crate) fn write_tile(root: &Path, body: usize) -> io::Result<()> {
    let mut tile = Vec::new();
    for f in [1_f32, (RADII_KM[body] * 1000.) as f32] {
        tile.extend(f.to_le_bytes());
    }
    for n in [1_i32, 1] {
        tile.extend(n.to_le_bytes());
    }
    for f in [-2_f32, 2., 2., -2.] {
        tile.extend(f.to_le_bytes());
    }
    for n in [0_i32, 7, 2, 2] {
        tile.extend(n.to_le_bytes());
    }
    for _ in 0..25 {
        tile.extend(400_i16.to_le_bytes());
    }
    fs::write(root.join(terrain_file(body).unwrap()), tile)
}

pub(super) fn check(app: &mut App, ctx: &egui::Context) -> io::Result<()> {
    let dir = tempfile::tempdir()?;
    let root = dir.path().join("data");
    fs::create_dir(&root)?;
    for body in [10, 1] {
        write_tile(&root, body)?;
    }
    let original = std::mem::replace(&mut app.data.as_mut().unwrap().root, root);
    let (lat, lon, mode) = (app.latitude, app.longitude, app.terrain_amplification);
    let result = [10, 1]
        .into_iter()
        .try_for_each(|body| check_body(app, ctx, body));
    app.data.as_mut().unwrap().root = original;
    (app.latitude, app.longitude, app.terrain_amplification) = (lat, lon, mode);
    app.motion = Default::default();
    app.surface = None;
    result
}

fn check_body(app: &mut App, ctx: &egui::Context, body: usize) -> io::Result<()> {
    app.view = View::Solar;
    app.solar.approach(body)?;
    app.motion.mode_duration = 0.5;
    (app.latitude, app.longitude) = (0., 0.);
    app.set_terrain_mode(Amplification::Actual)?;
    app.execute(Command::Surface, ctx);
    if app.view != View::Surface {
        return Err(io::Error::other(format!("{body}: 地形を表示できません")));
    }
    // 地形を開いた後で観測設定が変わっても、離陸元は表示中の地形である。
    (app.latitude, app.longitude) = (30., -40.);
    app.execute(Command::Land, ctx);
    let takeoff = app.motion.site.filter(|s| s.at_location(body, 0., 0.));
    if app.view != View::Solar
        || !takeoff
            .is_some_and(|s| s.heading == app.horizon.longitude && (s.terrain - 0.4).abs() < 1e-6)
    {
        return Err(io::Error::other(format!(
            "{body}: 表示中の地形から離陸できません"
        )));
    }
    let start = Instant::now();
    app.tick_reference_flight(start + Duration::from_millis(350))?;
    app.set_terrain_mode(Amplification::Double)?;
    app.tick_reference_flight(start + Duration::from_millis(500))?;
    let height_above_datum = app.solar.camera.distance * KM_PER_AU - RADII_KM[body];
    if (height_above_datum - 8. * RADII_KM[body]).abs() > 1e-6 {
        return Err(io::Error::other(format!(
            "{body}: 地形倍率で離陸先の高度が変化しました"
        )));
    }
    app.execute(Command::Land, ctx);
    let start = Instant::now();
    let mid = start + Duration::from_millis(150);
    app.tick_reference_flight(mid)?;
    let airborne = app
        .motion
        .site
        .ok_or_else(|| io::Error::other("降下中の地点がありません"))?;
    app.set_terrain_mode(Amplification::Five)?;
    app.tick_reference_flight(mid)?;
    let adjusted = app
        .motion
        .site
        .ok_or_else(|| io::Error::other("高度変更後の地点がありません"))?;
    if (adjusted.height + adjusted.terrain - airborne.height - airborne.terrain).abs() > 1e-6
        || (adjusted.terrain - 2.).abs() > 1e-6
    {
        return Err(io::Error::other(format!(
            "{body}: 降下中の地形倍率をカメラに反映できません"
        )));
    }
    app.tick_reference_flight(start + Duration::from_millis(500))?;
    let landed = app
        .motion
        .site
        .is_some_and(|s| s.height == 0.002 && (s.terrain - 2.).abs() < 1e-6);
    if !landed {
        return Err(io::Error::other(format!(
            "{body}: 到着時刻に新しい地形の2 m上へ着陸できません"
        )));
    }
    (app.latitude, app.longitude) = (0., 0.);
    app.execute(Command::Surface, ctx);
    let immediate = serde_json::json!({"number":1., "duration":0.});
    app.camera_action(100, &immediate, ctx)
        .map_err(io::Error::other)?;
    if app.view != View::Solar || app.motion.site.is_some() {
        return Err(io::Error::other(format!(
            "{body}: Luaの即時離陸が完了しません"
        )));
    }
    Ok(())
}
