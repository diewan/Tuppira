pub mod feed;
/// API server for the Tuppira.
///
/// Provides GraphQL and REST APIs for querying indexed data.
pub mod graphql;
pub mod rest;
pub mod server;

pub use server::ApiServer;
