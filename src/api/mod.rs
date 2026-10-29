//! HTTP API（Poem + poem-openapi）。

pub mod auth;
pub mod dto;
pub mod pool;
pub mod routes;

pub use pool::OcrPool;
pub use routes::IdCardApi;
