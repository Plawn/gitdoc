pub mod convert;
pub mod error;
pub mod repos;
pub mod snapshots;
pub mod symbols;
pub mod search;
pub mod analysis;
pub mod converse;
pub mod cheatsheet;
pub mod architect;

pub mod proto {
    tonic::include_proto!("gitdoc.v1");
}
