use eframe::egui;
pub fn install(ctx: &egui::Context) {
    let mut db = fontdb::Database::new();
    db.load_system_fonts();
    let families = [
        "Hiragino Sans",
        "Hiragino Kaku Gothic ProN",
        "Yu Gothic",
        "Meiryo",
        "Noto Sans CJK JP",
        "Noto Sans JP",
        "IPA Gothic",
        "IPAGothic",
    ];
    let Some(id) = families.iter().find_map(|name| {
        db.query(&fontdb::Query {
            families: &[fontdb::Family::Name(name)],
            ..Default::default()
        })
    }) else {
        return;
    };
    let Some(data) = db.with_face_data(id, |bytes, index| {
        let mut data = egui::FontData::from_owned(bytes.to_vec());
        data.index = index;
        data
    }) else {
        return;
    };
    let mut fonts = egui::FontDefinitions::default();
    fonts.font_data.insert("ja".into(), data.into());
    for fallbacks in fonts.families.values_mut() {
        fallbacks.push("ja".into());
    }
    ctx.set_fonts(fonts);
}
