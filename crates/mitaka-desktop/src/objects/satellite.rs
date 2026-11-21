use super::catalog::error;
use mitaka_core::{
    ephemeris::Cf96Ephemeris,
    satellite_catalog::{Dataset, SourceKind},
    satellite_model::{Model, Snapshot},
};
use std::{cell::RefCell, collections::HashMap, io};

struct SourceModel {
    model: Model,
    last: RefCell<Option<Snapshot>>,
}
pub(super) struct Satellites(HashMap<usize, SourceModel>);
impl Satellites {
    pub(super) fn prepare(dataset: &Dataset, warnings: &mut Vec<String>) -> Self {
        let mut systems = HashMap::new();
        for (i, src) in dataset
            .sources
            .iter()
            .enumerate()
            .filter(|(_, s)| s.kind == SourceKind::System)
        {
            match Model::from_source(dataset, i) {
                Ok(model) => {
                    systems.insert(
                        i,
                        SourceModel {
                            model,
                            last: RefCell::new(None),
                        },
                    );
                }
                Err(e) => warnings.push(format!("{} · {e}", src.key)),
            }
        }
        Self(systems)
    }
    pub(super) fn position(
        &self,
        source: usize,
        ordinal: usize,
        system: &Cf96Ephemeris,
        jd: f64,
    ) -> io::Result<[f64; 3]> {
        let model = self
            .0
            .get(&source)
            .ok_or_else(|| error("衛星系の登録順または原軌道を確定できません"))?;
        let mut cache = model.last.borrow_mut();
        if cache.as_ref().is_none_or(|s| s.jd != jd) {
            *cache = Some(model.model.evaluate(system, jd)?);
        }
        cache
            .as_ref()
            .expect("上で同時刻の位置を評価する")
            .position_au(ordinal)
    }
}
