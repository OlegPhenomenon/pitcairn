//! Route modules, one per API area. Each exposes `pub fn router() ->
//! Router<AppState>`; this merge list is extended one line per module by
//! later slices.

use axum::Router;

use crate::AppState;

pub mod admin;
pub mod auth;
pub mod change_requests;
pub mod conversation;
pub mod decisions;
pub mod demo;
pub mod documents;
pub mod notifications;
pub mod projects;
pub mod reviews;
pub mod sites;
pub mod team;
pub mod templates;
pub mod uploads;

pub fn api_router(state: AppState) -> Router<AppState> {
    Router::new()
        .merge(auth::router())
        .merge(uploads::router())
        .merge(projects::router())
        .merge(templates::router())
        .merge(team::router())
        .merge(sites::router())
        .merge(conversation::router())
        .merge(reviews::router())
        .merge(decisions::router())
        .merge(change_requests::router())
        .merge(documents::router())
        .merge(notifications::router())
        .merge(admin::router(state.clone()))
        .merge(demo::router(state.clone()))
}
