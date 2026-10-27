//! Adaptateurs des ports : Postgres, S3 (Garage), routage HTTP ; et la mise en route de Garage.

pub mod garage;
pub mod postgres;
pub mod routing;
pub mod s3;

#[cfg(test)]
pub mod memory;
