#![warn(
    clippy::nursery,
    clippy::pedantic,
    // clippy::cargo,
    // clippy::restriction,
    nonstandard_style,
    rust_2018_idioms,
    rust_2018_compatibility,

    // All default-allow lints in Rust 1.56.1, check `rustc -W help`
    absolute_paths_not_starting_with_crate,
    box_pointers,
    deprecated_in_future,
    elided_lifetimes_in_paths,
    explicit_outlives_requirements,
    keyword_idents,
    macro_use_extern_crate,
    meta_variable_misuse,
    missing_abi,
    // missing_copy_implementations,
    // missing_debug_implementations,
    // missing_docs,
    non_ascii_idents,
    noop_method_call,
    pointer_structural_match,
    rust_2021_incompatible_closure_captures,
    rust_2021_incompatible_or_patterns,
    rust_2021_prefixes_incompatible_syntax,
    rust_2021_prelude_collisions,
    single_use_lifetimes,
    trivial_casts,
    trivial_numeric_casts,
    unreachable_pub,
    // unsafe_code,
    unsafe_op_in_unsafe_fn,
    unstable_features,
    unused_crate_dependencies,
    unused_extern_crates,
    unused_import_braces,
    unused_lifetimes,
    unused_qualifications,
    unused_results,
    variant_size_differences,
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
        gllcms::GlLcms::type_(),
    )
}
