#![allow(dead_code)]

rust_i18n::i18n!("locales", fallback = "en");

pub mod app;
pub mod daemon_conn;
pub mod gui_config;
pub mod i18n;
pub mod model;
pub mod spawn;
pub mod subscription;
pub mod theme;
pub mod tray;
pub mod views;

#[cfg(test)]
mod i18n_tests;
