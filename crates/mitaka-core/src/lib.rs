//! Mitaka Plus の天文データと座標計算。GUI に依存しない。
#![forbid(unsafe_code)]
pub mod astronomy;
mod binary;
pub mod catalog;
pub mod data_root;
pub mod ephemeris;
pub mod lunar;
pub mod orbit;
pub mod terrain;
