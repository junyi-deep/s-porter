mod config;

pub(crate) use config::drawing_data_dir;
pub use config::{AppConfig, QuickCommand, load, save};
