//! Route modules, one per API area. Each exposes `pub fn router() ->
//! Router<AppState>`; this merge list is extended one line per module by
//! later slices.

use axum::Router;

use crate::AppState;

pub mod admin;
pub mod auth;
pub mod deliverables;
pub mod demo;
pub mod documents;
pub mod money;
pub mod notifications;
pub mod projects;
pub mod public;
pub mod resources;
pub mod trips;
pub mod uploads;

pub fn api_router(state: AppState) -> Router<AppState> {
    Router::new()
        .merge(auth::router())
        .merge(uploads::router())
        .merge(projects::router())
        .merge(documents::router())
        .merge(notifications::router())
        .merge(deliverables::router())
        .merge(public::router())
        .merge(resources::router())
        .merge(trips::router())
        .merge(money::router())
        .merge(admin::router(state.clone()))
        .merge(demo::router(state.clone()))
}
