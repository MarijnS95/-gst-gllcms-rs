#![warn(
    clippy::nursery,
    clippy::pedantic,
    // clippy::cargo,
    // clippy::restriction,
    nonstandard_style,
    rust_2018_idioms,
    rust_2018_compatibility,
)]
#![allow(
    clippy::default_trait_access,
    clippy::semicolon_if_nothing_returned,
    clippy::shadow_unrelated,
    clippy::todo,
    clippy::too_many_lines,
    clippy::unimplemented,
    clippy::unseparated_literal_suffix,
    clippy::wildcard_imports
)]

use gst::glib;
use gst::subclass::prelude::*;
use gst_gl::gst;

gst::plugin_define!(
    gllcms,
    env!("CARGO_PKG_DESCRIPTION"),
    plugin_init,
    concat!(env!("CARGO_PKG_VERSION"), "-", env!("COMMIT_ID")),
    // "MIT/X11",
    "unknown",
    env!("CARGO_PKG_NAME"),
    env!("CARGO_PKG_NAME"),
    env!("CARGO_PKG_REPOSITORY"),
    env!("BUILD_REL_DATE")
);

mod gllcms;

glib::wrapper! {
    pub struct GlLcms(ObjectSubclass<gllcms::GlLcms>) @extends gst_gl::GLFilter, gst_gl::GLBaseFilter;
}

fn plugin_init(plugin: &gst::Plugin) -> Result<(), glib::BoolError> {
    gst::Element::register(
        Some(plugin),
        gllcms::GlLcms::NAME,
        gst::Rank::None,
        gllcms::GlLcms::get_type(),
    )
}
