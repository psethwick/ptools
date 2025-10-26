pub mod db;
pub mod models;
pub mod queries;

pub use db::Pool;

pub const SERVICE_NAME: &str = "pstore";