//! The watch stream's messages, generated from `proto/design.proto` — with the
//! `server` feature, the `Designs` service's server side too — and the
//! conversion of a scene into them and back.

#![allow(clippy::all)]

include!(concat!(env!("OUT_DIR"), "/devpage.design.v1.rs"));
