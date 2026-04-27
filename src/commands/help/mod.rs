//! `marshal help` — print marshal's documentation by topic.
//!
//! Strategy + Registry just like `error_hints/`, `modernize/`, and
//! `what_now/`. Each [`topic::HelpTopic`] is a unit struct
//! producing a [`topic::HelpOutput`]; the registry resolves topic
//! names to impls and the [`Help`] command runs the lookup and
//! returns the chosen topic's output.
//!
//! `marshal help` (no arg) defaults to the `overview` topic.
//! `marshal help <topic>` looks up `<topic>` and errors cleanly if
//! it is not registered.

pub mod topic;
pub mod topics;

pub use topic::{HelpOutput, HelpTopic};

use anyhow::{anyhow, Result};
use std::ffi::OsString;

use crate::cli::Command;

/// `Command` impl for `marshal help`. Resolves the requested topic
/// (default `"overview"`) through the registry and produces its
/// [`HelpOutput`]. The dispatcher renders the output via the
/// active `OutputFormat`.
pub struct Help;

impl Command for Help {
    type Output = HelpOutput;

    fn run(&self, args: &[OsString]) -> Result<Self::Output> {
        let topic_name = args.first().and_then(|a| a.to_str()).unwrap_or("overview");
        let registry = Registry::default();
        match registry.lookup(topic_name) {
            Some(topic) => Ok(topic.produce()),
            None => Err(anyhow!(
                "unknown help topic '{topic_name}'. Available topics: {}",
                registry.names().join(", ")
            )),
        }
    }
}

pub struct Registry {
    topics: Vec<Box<dyn HelpTopic>>,
}

impl Registry {
    pub fn new() -> Self {
        Self { topics: Vec::new() }
    }

    pub fn register(&mut self, topic: Box<dyn HelpTopic>) {
        self.topics.push(topic);
    }

    /// Find a topic by name. Lookups are case-sensitive and
    /// exact-match — topic names are kebab-case ASCII so there is
    /// no ambiguity to normalise away.
    pub fn lookup(&self, name: &str) -> Option<&dyn HelpTopic> {
        self.topics
            .iter()
            .find(|t| t.name() == name)
            .map(|b| b.as_ref())
    }

    /// Every registered topic's name, in registration order. Used
    /// by `Help::run` to compose the "available topics" hint when
    /// the user asks for an unknown one.
    pub fn names(&self) -> Vec<&'static str> {
        self.topics.iter().map(|t| t.name()).collect()
    }
}

impl Default for Registry {
    /// The registry seeded with the canonical help topics.
    fn default() -> Self {
        let mut registry = Self::new();
        topics::register_defaults(&mut registry);
        registry
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_registry_resolves_overview() {
        let reg = Registry::default();
        let topic = reg.lookup("overview").expect("overview is registered");
        assert_eq!(topic.name(), "overview");
    }

    #[test]
    fn unknown_topic_yields_none() {
        let reg = Registry::default();
        assert!(reg.lookup("nope").is_none());
    }

    #[test]
    fn names_includes_overview() {
        let reg = Registry::default();
        let names = reg.names();
        assert!(names.contains(&"overview"));
    }

    #[test]
    fn help_with_no_args_returns_overview() {
        let out = Help.run(&[]).expect("overview always present");
        assert_eq!(out.topic, "overview");
    }

    #[test]
    fn help_with_named_topic_resolves_it() {
        let out = Help
            .run(&[OsString::from("overview")])
            .expect("overview is a real topic");
        assert_eq!(out.topic, "overview");
    }

    #[test]
    fn help_with_unknown_topic_errors_with_hint() {
        let err = Help.run(&[OsString::from("nope")]).unwrap_err();
        let msg = format!("{err}");
        assert!(msg.contains("unknown help topic 'nope'"));
        assert!(msg.contains("Available topics"));
        assert!(msg.contains("overview"));
    }
}
