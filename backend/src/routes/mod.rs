//! Route modules, one per API area. Each exposes `pub fn router() ->
//! Router<AppState>`; this merge list is extended one line per module by
//! later slices.

use axum::Router;

use crate::AppState;

pub mod auth;
pub mod uploads;

pub fn api_router() -> Router<AppState> {
    Router::new().merge(auth::router()).merge(uploads::router())
}
