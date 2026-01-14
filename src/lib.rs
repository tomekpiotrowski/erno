//! Erno Core - Shared infrastructure
//!
//! This crate provides reusable components that can be shared across
//! multiple applications, including job processing, configuration,
//! and other common utilities.

#![allow(missing_docs)]

// Modules will be added as we migrate functionality

pub mod api;
pub mod app;
pub mod app_info;
pub mod auth;
pub mod boot;
pub mod cli;
pub mod commands;
pub mod config;
pub mod console;
pub mod database;
pub mod emails;
pub mod environment;
pub mod job_queue;
pub mod jobs;
pub mod mailer;
pub mod password;
pub mod policy;
pub mod rate_limiting;
pub mod router;
pub mod setup_tracing;
pub mod token;
pub mod websocket;

#[cfg(any(test, feature = "test-utils"))]
pub mod tests;
